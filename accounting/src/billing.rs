use crate::db::{self, BillingPlan};
use crate::ledger::{self, EntryLine, PostTransactionRequest};
use crate::{accounts, interest, InvoiceId, TransactionId};
use chrono::NaiveDate;
use rust_decimal::Decimal;

#[derive(Debug)]
pub enum BillingError {
    NoBillingPlanAssigned(i32),
    MissingAreaForPlan(i32),
    InvalidSlabConfig(i32),
    AlreadyBilledForPeriod,
    FundNotFound(crate::FundId),
    Post(ledger::PostError),
    Db(sqlx::Error),
}

impl std::fmt::Display for BillingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BillingError::NoBillingPlanAssigned(flat_id) => {
                write!(f, "Flat {} has no billing plan assigned", flat_id)
            }
            BillingError::MissingAreaForPlan(flat_id) => write!(
                f,
                "Flat {}'s billing plan needs area_sqft, but none is set",
                flat_id
            ),
            BillingError::InvalidSlabConfig(flat_id) => write!(
                f,
                "Flat {}'s slab billing plan config is invalid or has no matching slab",
                flat_id
            ),
            BillingError::AlreadyBilledForPeriod => {
                write!(
                    f,
                    "This flat already has a non-void invoice for this period"
                )
            }
            BillingError::FundNotFound(fund_id) => write!(f, "Fund {} not found", fund_id),
            BillingError::Post(e) => write!(f, "{}", e),
            BillingError::Db(e) => write!(f, "{}", e),
        }
    }
}

impl From<sqlx::Error> for BillingError {
    fn from(e: sqlx::Error) -> Self {
        BillingError::Db(e)
    }
}

impl From<ledger::PostError> for BillingError {
    fn from(e: ledger::PostError) -> Self {
        BillingError::Post(e)
    }
}

#[derive(serde::Deserialize)]
struct FlatRateConfig {
    amount: Decimal,
}

#[derive(serde::Deserialize)]
struct PerSqftConfig {
    rate_per_sqft: Decimal,
}

#[derive(serde::Deserialize)]
struct Slab {
    max_sqft: Decimal,
    amount: Decimal,
}

#[derive(serde::Deserialize)]
struct SlabConfig {
    slabs: Vec<Slab>,
}

fn compute_base_amount(
    flat_id: i32,
    plan: &BillingPlan,
    area_sqft: Option<Decimal>,
) -> Result<Decimal, BillingError> {
    match plan.billing_type.as_str() {
        "FLAT_RATE" => {
            let cfg: FlatRateConfig = serde_json::from_value(plan.config.clone())
                .map_err(|_| BillingError::InvalidSlabConfig(flat_id))?;
            Ok(cfg.amount)
        }
        "PER_SQFT" => {
            let area = area_sqft.ok_or(BillingError::MissingAreaForPlan(flat_id))?;
            let cfg: PerSqftConfig = serde_json::from_value(plan.config.clone())
                .map_err(|_| BillingError::InvalidSlabConfig(flat_id))?;
            Ok((area * cfg.rate_per_sqft).round_dp(2))
        }
        "SLAB" => {
            let area = area_sqft.ok_or(BillingError::MissingAreaForPlan(flat_id))?;
            let cfg: SlabConfig = serde_json::from_value(plan.config.clone())
                .map_err(|_| BillingError::InvalidSlabConfig(flat_id))?;
            // Smallest covering slab, not just the first match — config order isn't guaranteed sorted.
            cfg.slabs
                .iter()
                .filter(|s| area <= s.max_sqft)
                .min_by(|a, b| a.max_sqft.cmp(&b.max_sqft))
                .map(|s| s.amount)
                .ok_or(BillingError::InvalidSlabConfig(flat_id))
        }
        _ => Err(BillingError::InvalidSlabConfig(flat_id)),
    }
}

pub struct ComputedBill {
    pub plan: BillingPlan,
    pub base_amount: Decimal,
    pub gst_rule: Option<db::GstRule>,
    pub gst_applicable: bool,
    pub gst_amount: Decimal,
    /// One line per active fund contribution rule the flat is assigned to.
    pub fund_contributions: Vec<(db::FundContributionRule, Decimal)>,
    pub total: Decimal,
}

/// Read-only: what a flat would be billed for `period_start`. Shared by
/// `generate_invoice_for_flat` and the preview report so they can't drift apart.
pub async fn compute_bill_for_flat(
    conn: &mut sqlx::PgConnection,
    flat_id: i32,
    period_start: NaiveDate,
) -> Result<ComputedBill, BillingError> {
    let assignment = db::get_billing_assignment(conn, flat_id)
        .await?
        .ok_or(BillingError::NoBillingPlanAssigned(flat_id))?;
    let plan = db::get_billing_plan(conn, assignment.billing_plan_id).await?;
    let base_amount = compute_base_amount(flat_id, &plan, assignment.area_sqft)?;

    let gst_rule = db::get_active_gst_rule(conn, period_start).await?;
    let (gst_applicable, gst_amount) = match &gst_rule {
        Some(rule) => crate::gst::compute_gst(rule, base_amount),
        None => (false, Decimal::ZERO),
    };

    let fund_rules = db::get_fund_contribution_rules_for_flat(conn, flat_id, period_start).await?;
    let fund_contributions: Vec<(db::FundContributionRule, Decimal)> = fund_rules
        .into_iter()
        .map(|rule| {
            let amount = crate::funds::compute_fund_contribution(&rule, base_amount);
            (rule, amount)
        })
        .collect();
    let fund_total: Decimal = fund_contributions.iter().map(|(_, amount)| *amount).sum();

    let total = base_amount + gst_amount + fund_total;

    Ok(ComputedBill {
        plan,
        base_amount,
        gst_rule,
        gst_applicable,
        gst_amount,
        fund_contributions,
        total,
    })
}

/// Posts one invoice for `flat_id`: Dr. Receivable / Cr. Income (+ GST if applicable).
/// Idempotent per (flat_id, billing_period_start) via the unique index.
pub async fn generate_invoice_for_flat(
    conn: &mut sqlx::PgConnection,
    flat_id: i32,
    period_start: NaiveDate,
    period_end: NaiveDate,
    due_date: NaiveDate,
    created_by: i32,
) -> Result<InvoiceId, BillingError> {
    let ComputedBill {
        plan,
        base_amount,
        gst_rule,
        gst_applicable,
        gst_amount,
        fund_contributions,
        total,
    } = compute_bill_for_flat(conn, flat_id, period_start).await?;

    let invoice_number = db::next_invoice_number(conn).await?;
    let fiscal_year_id = db::resolve_fiscal_year_id(conn, period_start)
        .await?
        .ok_or(ledger::PostError::NoFiscalYearForDate(period_start))?;

    let invoice_id = db::insert_invoice(
        conn,
        db::NewInvoice {
            invoice_number: &invoice_number,
            flat_id,
            fiscal_year_id,
            billing_period_start: period_start,
            billing_period_end: period_end,
            due_date,
            subtotal: base_amount,
            gst_amount,
            total_amount: total,
            gstin_applicable: gst_applicable,
            created_by,
        },
    )
    .await
    .map_err(|e| match &e {
        sqlx::Error::Database(db_err)
            if db_err.constraint() == Some("idx_invoices_flat_period") =>
        {
            BillingError::AlreadyBilledForPeriod
        }
        _ => BillingError::Db(e),
    })?;

    let receivable_account =
        db::get_account_id_by_code(conn, accounts::MEMBER_DUES_RECEIVABLE).await?;
    let income_account = db::get_account_id_by_code(conn, accounts::MAINTENANCE_INCOME).await?;

    db::insert_invoice_line_item(
        conn,
        invoice_id,
        &plan.name,
        income_account,
        gst_rule.as_ref().map(|r| r.hsn_sac_code.as_str()),
        base_amount,
        gst_rule
            .as_ref()
            .map(|r| r.gst_rate_pct)
            .unwrap_or(Decimal::ZERO),
        gst_amount,
    )
    .await?;

    let mut entries = vec![
        EntryLine {
            account_id: receivable_account,
            flat_id: Some(flat_id),
            debit: total,
            credit: Decimal::ZERO,
        },
        EntryLine {
            account_id: income_account,
            flat_id: Some(flat_id),
            debit: Decimal::ZERO,
            credit: base_amount,
        },
    ];
    if gst_applicable {
        let gst_account = db::get_account_id_by_code(conn, accounts::GST_PAYABLE).await?;
        entries.push(EntryLine {
            account_id: gst_account,
            flat_id: Some(flat_id),
            debit: Decimal::ZERO,
            credit: gst_amount,
        });
    }

    for (rule, amount) in &fund_contributions {
        if *amount <= Decimal::ZERO {
            continue;
        }
        let fund = db::list_funds(conn)
            .await?
            .into_iter()
            .find(|f| f.fund_id == rule.fund_id)
            .ok_or(BillingError::FundNotFound(rule.fund_id))?;
        let fund_account =
            db::get_or_create_fund_income_account(conn, rule.fund_id, &fund.name).await?;
        db::insert_invoice_line_item(
            conn,
            invoice_id,
            &format!("{} Contribution", fund.name),
            fund_account,
            None,
            *amount,
            Decimal::ZERO,
            Decimal::ZERO,
        )
        .await?;
        entries.push(EntryLine {
            account_id: fund_account,
            flat_id: Some(flat_id),
            debit: Decimal::ZERO,
            credit: *amount,
        });
    }

    let transaction_id = ledger::post_transaction(
        conn,
        PostTransactionRequest {
            transaction_date: period_start,
            description: &format!("Invoice {} for flat {}", invoice_number, flat_id),
            source_type: "INVOICE",
            source_id: Some(invoice_id.0),
            reverses_transaction_id: None,
            entries,
            created_by,
        },
    )
    .await?;

    db::link_invoice_transaction(conn, invoice_id, transaction_id).await?;
    Ok(invoice_id)
}

/// Posts the exact reverse of `transaction_id` (debits/credits swapped), linked via
/// `reverses_transaction_id`. Shared by invoice void and interest-charge reversal.
async fn reverse_transaction(
    conn: &mut sqlx::PgConnection,
    transaction_id: TransactionId,
    description: &str,
    source_type: &str,
    source_id: uuid::Uuid,
    as_of: NaiveDate,
    created_by: i32,
) -> Result<(), BillingError> {
    let entries = db::get_ledger_entries_for_transaction(conn, transaction_id)
        .await
        .map_err(BillingError::Db)?;
    let reversed: Vec<EntryLine> = entries
        .into_iter()
        .map(|e| EntryLine {
            account_id: e.account_id,
            flat_id: e.flat_id,
            debit: e.credit,
            credit: e.debit,
        })
        .collect();
    ledger::post_transaction(
        conn,
        PostTransactionRequest {
            transaction_date: as_of,
            description,
            source_type,
            source_id: Some(source_id),
            reverses_transaction_id: Some(transaction_id),
            entries: reversed,
            created_by,
        },
    )
    .await
    .map_err(BillingError::Post)?;
    Ok(())
}

/// Reverses the invoice's original entries and any interest charged against it.
pub async fn void_invoice(
    conn: &mut sqlx::PgConnection,
    invoice_id: InvoiceId,
    reason: &str,
    as_of: NaiveDate,
    created_by: i32,
) -> Result<(), BillingError> {
    if let Some(original_transaction_id) = db::get_invoice_ledger_transaction_id(conn, invoice_id)
        .await
        .map_err(BillingError::Db)?
    {
        reverse_transaction(
            conn,
            original_transaction_id,
            &format!("Voiding invoice: {}", reason),
            "INVOICE_VOID",
            invoice_id.0,
            as_of,
            created_by,
        )
        .await?;
    }

    for interest_transaction_id in db::get_interest_charge_transaction_ids(conn, invoice_id)
        .await
        .map_err(BillingError::Db)?
    {
        reverse_transaction(
            conn,
            interest_transaction_id,
            &format!("Reversing interest charge on voided invoice: {}", reason),
            "INTEREST_VOID",
            invoice_id.0,
            as_of,
            created_by,
        )
        .await?;
    }

    db::void_invoice(conn, invoice_id, reason)
        .await
        .map_err(BillingError::Db)?;
    Ok(())
}

/// Posts interest for one overdue invoice as its own transaction (Dr. Receivable /
/// Cr. Interest Income), separate from the invoice's own total_amount.
pub async fn charge_interest_for_invoice(
    conn: &mut sqlx::PgConnection,
    invoice: &db::Invoice,
    rule: &db::InterestRule,
    as_of: NaiveDate,
    created_by: i32,
) -> Result<Option<TransactionId>, BillingError> {
    let overdue_amount = invoice.total_amount - invoice.amount_paid;
    if overdue_amount <= Decimal::ZERO {
        return Ok(None);
    }
    let days_overdue = (as_of - invoice.due_date).num_days() as i32;
    // Recompute the full-period total, then charge only the delta not yet posted —
    // keeps re-runs correct after partial payments without double-charging.
    let total_interest_accrued = interest::compute_interest(rule, overdue_amount, days_overdue);
    let already_charged = db::sum_interest_charged(conn, invoice.invoice_id).await?;
    let interest_amount = total_interest_accrued - already_charged;
    if interest_amount <= Decimal::ZERO {
        return Ok(None);
    }

    let receivable_account =
        db::get_account_id_by_code(conn, accounts::MEMBER_DUES_RECEIVABLE).await?;
    let interest_income_account =
        db::get_account_id_by_code(conn, accounts::LATE_PAYMENT_INTEREST_INCOME).await?;

    let transaction_id = ledger::post_transaction(
        conn,
        PostTransactionRequest {
            transaction_date: as_of,
            description: &format!(
                "Late payment interest on invoice {}",
                invoice.invoice_number
            ),
            source_type: "INTEREST",
            source_id: Some(invoice.invoice_id.0),
            reverses_transaction_id: None,
            entries: vec![
                EntryLine {
                    account_id: receivable_account,
                    flat_id: Some(invoice.flat_id),
                    debit: interest_amount,
                    credit: Decimal::ZERO,
                },
                EntryLine {
                    account_id: interest_income_account,
                    flat_id: Some(invoice.flat_id),
                    debit: Decimal::ZERO,
                    credit: interest_amount,
                },
            ],
            created_by,
        },
    )
    .await?;

    db::insert_interest_charge(
        conn,
        db::NewInterestCharge {
            invoice_id: invoice.invoice_id,
            interest_rule_id: rule.interest_rule_id,
            overdue_amount,
            days_overdue,
            interest_amount,
            charged_date: as_of,
            ledger_transaction_id: transaction_id,
        },
    )
    .await?;

    Ok(Some(transaction_id))
}

/// Runs the interest job for every currently-overdue invoice.
pub async fn run_interest_for_all_overdue(
    conn: &mut sqlx::PgConnection,
    as_of: NaiveDate,
    created_by: i32,
) -> Result<usize, BillingError> {
    let Some(rule) = db::get_active_interest_rule(conn, as_of).await? else {
        return Ok(0);
    };
    let overdue = db::find_chargeable_overdue_invoices(conn, as_of).await?;
    let mut charged = 0;
    for invoice in &overdue {
        if charge_interest_for_invoice(conn, invoice, &rule, as_of, created_by)
            .await?
            .is_some()
        {
            charged += 1;
        }
    }
    Ok(charged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn slab_plan(slabs_json: serde_json::Value) -> BillingPlan {
        BillingPlan {
            billing_plan_id: crate::BillingPlanId(1),
            name: "Slab Plan".to_string(),
            billing_type: "SLAB".to_string(),
            config: serde_json::json!({ "slabs": slabs_json }),
            frequency: "MONTHLY".to_string(),
            is_active: true,
            created_by: 1,
        }
    }

    /// Selection must pick the smallest covering slab regardless of config order.
    #[test]
    fn slab_selection_is_correct_even_when_config_is_not_sorted_ascending() {
        let unsorted = slab_plan(serde_json::json!([
            { "max_sqft": 999999, "amount": 7000 },
            { "max_sqft": 800, "amount": 3500 },
            { "max_sqft": 1200, "amount": 5000 },
        ]));

        assert_eq!(
            compute_base_amount(1, &unsorted, Some(dec!(650))).unwrap(),
            dec!(3500),
            "650 sqft must land in the 0-800 band regardless of array order"
        );
        assert_eq!(
            compute_base_amount(1, &unsorted, Some(dec!(1000))).unwrap(),
            dec!(5000),
            "1000 sqft must land in the 800-1200 band, not the first (largest) one listed"
        );
        assert_eq!(
            compute_base_amount(1, &unsorted, Some(dec!(1500))).unwrap(),
            dec!(7000)
        );
    }

    #[test]
    fn slab_selection_rejects_area_above_every_slab() {
        let plan = slab_plan(serde_json::json!([{ "max_sqft": 800, "amount": 3500 }]));
        let result = compute_base_amount(1, &plan, Some(dec!(5000)));
        assert!(matches!(result, Err(BillingError::InvalidSlabConfig(1))));
    }
}
