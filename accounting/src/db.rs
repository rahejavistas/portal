use crate::{
    AccountId, BankImportId, BankLineId, BillingPlanId, BudgetId, ClaimId, ExpenseId, FiscalYearId,
    FundId, FundRuleId, GstRuleId, InterestRuleId, InvoiceId, PaymentId, PoId, ReceiptId,
    TdsRuleId, TransactionId, VendorId,
};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use uuid::Uuid;

/// Namespace for the advisory lock guarding a flat's credit balance.
const ADVISORY_LOCK_NAMESPACE_FLAT_CREDIT: i32 = 1;

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct BillingPlan {
    pub billing_plan_id: BillingPlanId,
    pub name: String,
    pub billing_type: String,
    pub config: serde_json::Value,
    pub frequency: String,
    pub is_active: bool,
    pub created_by: i32,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct FlatBillingAssignment {
    pub flat_id: i32,
    pub billing_plan_id: BillingPlanId,
    pub area_sqft: Option<Decimal>,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct GstRule {
    pub gst_rule_id: GstRuleId,
    pub per_member_monthly_exempt_threshold: Decimal,
    pub society_turnover_registration_threshold: Decimal,
    pub gst_rate_pct: Decimal,
    pub hsn_sac_code: String,
    pub society_gstin: Option<String>,
    pub created_by: i32,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct InterestRule {
    pub interest_rule_id: InterestRuleId,
    pub annual_rate_pct: Decimal,
    pub grace_period_days: i32,
    pub compounding: String,
    pub created_by: i32,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct Invoice {
    pub invoice_id: InvoiceId,
    pub invoice_number: String,
    pub flat_id: i32,
    pub fiscal_year_id: FiscalYearId,
    pub billing_period_start: NaiveDate,
    pub billing_period_end: NaiveDate,
    pub due_date: NaiveDate,
    pub subtotal: Decimal,
    pub gst_amount: Decimal,
    pub total_amount: Decimal,
    pub amount_paid: Decimal,
    pub status: String,
    pub gstin_applicable: bool,
    pub created_by: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct Payment {
    pub payment_id: PaymentId,
    pub invoice_id: InvoiceId,
    pub amount: Decimal,
    pub payment_method: String,
    pub reference_number: Option<String>,
    pub recorded_by: i32,
    pub confirmed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct Receipt {
    pub receipt_id: ReceiptId,
    pub receipt_number: String,
    pub payment_id: PaymentId,
    pub generated_at: DateTime<Utc>,
}

pub async fn get_account_id_by_code(
    conn: &mut sqlx::PgConnection,
    code: &str,
) -> Result<AccountId, sqlx::Error> {
    sqlx::query_scalar("SELECT account_id FROM chart_of_accounts WHERE code = $1")
        .bind(code)
        .fetch_one(conn)
        .await
}

/// The fiscal year whose [start_date, end_date] contains `date`, if one has been created.
pub async fn resolve_fiscal_year_id(
    conn: &mut sqlx::PgConnection,
    date: NaiveDate,
) -> Result<Option<FiscalYearId>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT fiscal_year_id FROM fiscal_years WHERE start_date <= $1 AND end_date >= $1",
    )
    .bind(date)
    .fetch_optional(conn)
    .await
}

/// Row-locks the fiscal year (`FOR SHARE`) so a concurrent `close_fiscal_year`
/// (`FOR UPDATE`) can't race a posting past this check.
pub async fn is_fiscal_year_closed(
    conn: &mut sqlx::PgConnection,
    fiscal_year_id: FiscalYearId,
) -> Result<bool, sqlx::Error> {
    let status: String =
        sqlx::query_scalar("SELECT status FROM fiscal_years WHERE fiscal_year_id = $1 FOR SHARE")
            .bind(fiscal_year_id)
            .fetch_one(conn)
            .await?;
    Ok(status == "CLOSED")
}

pub async fn next_invoice_number(conn: &mut sqlx::PgConnection) -> Result<String, sqlx::Error> {
    let n: i64 = sqlx::query_scalar("SELECT nextval('invoice_number_seq')")
        .fetch_one(conn)
        .await?;
    Ok(format!("INV-{:06}", n))
}

pub async fn next_receipt_number(conn: &mut sqlx::PgConnection) -> Result<String, sqlx::Error> {
    let n: i64 = sqlx::query_scalar("SELECT nextval('receipt_number_seq')")
        .fetch_one(conn)
        .await?;
    Ok(format!("RCT-{:06}", n))
}

pub async fn get_billing_assignment(
    conn: &mut sqlx::PgConnection,
    flat_id: i32,
) -> Result<Option<FlatBillingAssignment>, sqlx::Error> {
    sqlx::query_as::<_, FlatBillingAssignment>(
        "SELECT fba.flat_id, fba.billing_plan_id, f.area_sqft \
         FROM flat_billing_assignments fba \
         JOIN flats f ON f.flat_id = fba.flat_id \
         WHERE fba.flat_id = $1",
    )
    .bind(flat_id)
    .fetch_optional(conn)
    .await
}

pub async fn get_billing_plan(
    conn: &mut sqlx::PgConnection,
    billing_plan_id: BillingPlanId,
) -> Result<BillingPlan, sqlx::Error> {
    sqlx::query_as::<_, BillingPlan>(
        "SELECT billing_plan_id, name, billing_type, config, frequency, is_active, created_by \
         FROM billing_plans WHERE billing_plan_id = $1",
    )
    .bind(billing_plan_id)
    .fetch_one(conn)
    .await
}

pub async fn get_active_gst_rule(
    conn: &mut sqlx::PgConnection,
    as_of: NaiveDate,
) -> Result<Option<GstRule>, sqlx::Error> {
    sqlx::query_as::<_, GstRule>(
        "SELECT gst_rule_id, per_member_monthly_exempt_threshold, \
                society_turnover_registration_threshold, gst_rate_pct, hsn_sac_code, society_gstin, \
                created_by \
         FROM gst_rules \
         WHERE is_active = TRUE AND effective_from <= $1 \
         ORDER BY effective_from DESC LIMIT 1",
    )
    .bind(as_of)
    .fetch_optional(conn)
    .await
}

pub async fn get_active_interest_rule(
    conn: &mut sqlx::PgConnection,
    as_of: NaiveDate,
) -> Result<Option<InterestRule>, sqlx::Error> {
    sqlx::query_as::<_, InterestRule>(
        "SELECT interest_rule_id, annual_rate_pct, grace_period_days, compounding, created_by \
         FROM interest_rules \
         WHERE is_active = TRUE AND effective_from <= $1 \
         ORDER BY effective_from DESC LIMIT 1",
    )
    .bind(as_of)
    .fetch_optional(conn)
    .await
}

pub struct NewBillingPlan<'a> {
    pub name: &'a str,
    pub billing_type: &'a str,
    pub config: &'a serde_json::Value,
    pub frequency: Option<&'a str>,
    pub created_by: i32,
}

pub async fn insert_billing_plan(
    conn: &mut sqlx::PgConnection,
    plan: NewBillingPlan<'_>,
) -> Result<BillingPlanId, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO billing_plans (name, billing_type, config, frequency, created_by) \
         VALUES ($1, $2, $3, COALESCE($4, 'MONTHLY'), $5) RETURNING billing_plan_id",
    )
    .bind(plan.name)
    .bind(plan.billing_type)
    .bind(plan.config)
    .bind(plan.frequency)
    .bind(plan.created_by)
    .fetch_one(conn)
    .await
}

pub async fn list_billing_plans(
    conn: &mut sqlx::PgConnection,
) -> Result<Vec<BillingPlan>, sqlx::Error> {
    sqlx::query_as::<_, BillingPlan>(
        "SELECT billing_plan_id, name, billing_type, config, frequency, is_active, created_by \
         FROM billing_plans ORDER BY name",
    )
    .fetch_all(conn)
    .await
}

/// `area_sqft`, when given, updates the flat's own area. Callers should wrap this in a transaction.
pub async fn upsert_flat_billing_assignment(
    conn: &mut sqlx::PgConnection,
    flat_id: i32,
    billing_plan_id: BillingPlanId,
    area_sqft: Option<Decimal>,
    assigned_by: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO flat_billing_assignments (flat_id, billing_plan_id, assigned_by) \
         VALUES ($1, $2, $3) \
         ON CONFLICT (flat_id) DO UPDATE SET billing_plan_id = $2, assigned_by = $3",
    )
    .bind(flat_id)
    .bind(billing_plan_id)
    .bind(assigned_by)
    .execute(&mut *conn)
    .await?;
    if let Some(area) = area_sqft {
        sqlx::query("UPDATE flats SET area_sqft = $1 WHERE flat_id = $2")
            .bind(area)
            .bind(flat_id)
            .execute(conn)
            .await?;
    }
    Ok(())
}

pub async fn list_flats_with_billing_assignment(
    conn: &mut sqlx::PgConnection,
) -> Result<Vec<i32>, sqlx::Error> {
    sqlx::query_scalar("SELECT flat_id FROM flat_billing_assignments")
        .fetch_all(conn)
        .await
}

/// Assigns `billing_plan_id` (FLAT_RATE only) to every unassigned flat. Returns how many were newly assigned.
pub async fn bulk_assign_flat_rate_plan_to_unassigned(
    conn: &mut sqlx::PgConnection,
    billing_plan_id: BillingPlanId,
    assigned_by: i32,
) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "INSERT INTO flat_billing_assignments (flat_id, billing_plan_id, assigned_by) \
         SELECT f.flat_id, $1, $2 FROM flats f \
         LEFT JOIN flat_billing_assignments fba ON fba.flat_id = f.flat_id \
         WHERE fba.flat_id IS NULL",
    )
    .bind(billing_plan_id)
    .bind(assigned_by)
    .execute(conn)
    .await?;
    Ok(result.rows_affected())
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct FlatBillingAssignmentRow {
    pub flat_id: i32,
    pub wing: String,
    pub flat_number: String,
    pub billing_plan_id: Option<BillingPlanId>,
    pub billing_plan_name: Option<String>,
    pub billing_type: Option<String>,
    pub area_sqft: Option<Decimal>,
    pub assigned_by: Option<i32>,
}

/// Every flat with whatever billing plan it has assigned, if any (null columns if unassigned).
pub async fn list_all_flat_billing_assignments(
    conn: &mut sqlx::PgConnection,
) -> Result<Vec<FlatBillingAssignmentRow>, sqlx::Error> {
    sqlx::query_as::<_, FlatBillingAssignmentRow>(
        "SELECT f.flat_id, f.wing, f.flat_number, bp.billing_plan_id, bp.name AS billing_plan_name, \
                bp.billing_type, f.area_sqft, fba.assigned_by \
         FROM flats f \
         LEFT JOIN flat_billing_assignments fba ON fba.flat_id = f.flat_id \
         LEFT JOIN billing_plans bp ON bp.billing_plan_id = fba.billing_plan_id \
         ORDER BY f.wing, f.flat_number",
    )
    .fetch_all(conn)
    .await
}

pub struct NewGstRule<'a> {
    pub per_member_monthly_exempt_threshold: Decimal,
    pub society_turnover_registration_threshold: Decimal,
    pub gst_rate_pct: Decimal,
    pub hsn_sac_code: &'a str,
    pub society_gstin: Option<&'a str>,
    pub effective_from: NaiveDate,
    pub created_by: i32,
}

pub async fn insert_gst_rule(
    conn: &mut sqlx::PgConnection,
    rule: NewGstRule<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO gst_rules \
            (per_member_monthly_exempt_threshold, society_turnover_registration_threshold, \
             gst_rate_pct, hsn_sac_code, society_gstin, effective_from, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(rule.per_member_monthly_exempt_threshold)
    .bind(rule.society_turnover_registration_threshold)
    .bind(rule.gst_rate_pct)
    .bind(rule.hsn_sac_code)
    .bind(rule.society_gstin)
    .bind(rule.effective_from)
    .bind(rule.created_by)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn list_active_gst_rules(
    conn: &mut sqlx::PgConnection,
) -> Result<Vec<GstRule>, sqlx::Error> {
    sqlx::query_as::<_, GstRule>(
        "SELECT gst_rule_id, per_member_monthly_exempt_threshold, \
                society_turnover_registration_threshold, gst_rate_pct, hsn_sac_code, society_gstin, \
                created_by \
         FROM gst_rules WHERE is_active = TRUE ORDER BY effective_from DESC",
    )
    .fetch_all(conn)
    .await
}

pub struct NewInterestRule {
    pub annual_rate_pct: Decimal,
    pub grace_period_days: i32,
    pub compounding: String,
    pub effective_from: NaiveDate,
    pub created_by: i32,
}

pub async fn insert_interest_rule(
    conn: &mut sqlx::PgConnection,
    rule: NewInterestRule,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO interest_rules (annual_rate_pct, grace_period_days, compounding, effective_from, created_by) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(rule.annual_rate_pct)
    .bind(rule.grace_period_days)
    .bind(rule.compounding)
    .bind(rule.effective_from)
    .bind(rule.created_by)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn list_active_interest_rules(
    conn: &mut sqlx::PgConnection,
) -> Result<Vec<InterestRule>, sqlx::Error> {
    sqlx::query_as::<_, InterestRule>(
        "SELECT interest_rule_id, annual_rate_pct, grace_period_days, compounding, created_by \
         FROM interest_rules WHERE is_active = TRUE ORDER BY effective_from DESC",
    )
    .fetch_all(conn)
    .await
}

pub struct NewInvoice<'a> {
    pub invoice_number: &'a str,
    pub flat_id: i32,
    pub fiscal_year_id: FiscalYearId,
    pub billing_period_start: NaiveDate,
    pub billing_period_end: NaiveDate,
    pub due_date: NaiveDate,
    pub subtotal: Decimal,
    pub gst_amount: Decimal,
    pub total_amount: Decimal,
    pub gstin_applicable: bool,
    pub created_by: i32,
}

pub async fn insert_invoice(
    conn: &mut sqlx::PgConnection,
    new: NewInvoice<'_>,
) -> Result<InvoiceId, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO invoices \
            (invoice_number, flat_id, fiscal_year_id, billing_period_start, billing_period_end, \
             due_date, subtotal, gst_amount, total_amount, gstin_applicable, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) \
         RETURNING invoice_id",
    )
    .bind(new.invoice_number)
    .bind(new.flat_id)
    .bind(new.fiscal_year_id)
    .bind(new.billing_period_start)
    .bind(new.billing_period_end)
    .bind(new.due_date)
    .bind(new.subtotal)
    .bind(new.gst_amount)
    .bind(new.total_amount)
    .bind(new.gstin_applicable)
    .bind(new.created_by)
    .fetch_one(conn)
    .await
}

pub async fn link_invoice_transaction(
    conn: &mut sqlx::PgConnection,
    invoice_id: InvoiceId,
    transaction_id: crate::TransactionId,
) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE invoices SET ledger_transaction_id = $1 WHERE invoice_id = $2")
        .bind(transaction_id)
        .bind(invoice_id)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn get_invoice_ledger_transaction_id(
    conn: &mut sqlx::PgConnection,
    invoice_id: InvoiceId,
) -> Result<Option<TransactionId>, sqlx::Error> {
    let row: Option<Option<TransactionId>> =
        sqlx::query_scalar("SELECT ledger_transaction_id FROM invoices WHERE invoice_id = $1")
            .bind(invoice_id)
            .fetch_optional(conn)
            .await?;
    Ok(row.flatten())
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_invoice_line_item(
    conn: &mut sqlx::PgConnection,
    invoice_id: InvoiceId,
    description: &str,
    account_id: AccountId,
    hsn_sac_code: Option<&str>,
    amount: Decimal,
    gst_rate_pct: Decimal,
    gst_amount: Decimal,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO invoice_line_items (invoice_id, description, account_id, hsn_sac_code, amount, gst_rate_pct, gst_amount) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(invoice_id)
    .bind(description)
    .bind(account_id)
    .bind(hsn_sac_code)
    .bind(amount)
    .bind(gst_rate_pct)
    .bind(gst_amount)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn get_invoice(
    conn: &mut sqlx::PgConnection,
    invoice_id: InvoiceId,
) -> Result<Option<Invoice>, sqlx::Error> {
    sqlx::query_as::<_, Invoice>(
        "SELECT invoice_id, invoice_number, flat_id, fiscal_year_id, billing_period_start, \
                billing_period_end, due_date, subtotal, gst_amount, total_amount, amount_paid, \
                status, gstin_applicable, created_by, created_at \
         FROM invoices WHERE invoice_id = $1",
    )
    .bind(invoice_id)
    .fetch_optional(conn)
    .await
}

/// Same as `get_invoice`, but row-locks it — required before applying a payment or credit.
pub async fn get_invoice_for_update(
    conn: &mut sqlx::PgConnection,
    invoice_id: InvoiceId,
) -> Result<Option<Invoice>, sqlx::Error> {
    sqlx::query_as::<_, Invoice>(
        "SELECT invoice_id, invoice_number, flat_id, fiscal_year_id, billing_period_start, \
                billing_period_end, due_date, subtotal, gst_amount, total_amount, amount_paid, \
                status, gstin_applicable, created_by, created_at \
         FROM invoices WHERE invoice_id = $1 FOR UPDATE",
    )
    .bind(invoice_id)
    .fetch_optional(conn)
    .await
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct InvoiceListRow {
    pub invoice_id: InvoiceId,
    pub invoice_number: String,
    pub flat_id: i32,
    pub wing: String,
    pub flat_number: String,
    pub fiscal_year_id: FiscalYearId,
    pub billing_period_start: NaiveDate,
    pub billing_period_end: NaiveDate,
    pub due_date: NaiveDate,
    pub subtotal: Decimal,
    pub gst_amount: Decimal,
    pub total_amount: Decimal,
    pub amount_paid: Decimal,
    pub status: String,
    pub gstin_applicable: bool,
    pub created_by: i32,
    pub created_at: DateTime<Utc>,
}

/// The admin-facing invoice list, enriched with wing/flat_number.
pub async fn list_invoices(
    conn: &mut sqlx::PgConnection,
    flat_id: Option<i32>,
    status: Option<&str>,
) -> Result<Vec<InvoiceListRow>, sqlx::Error> {
    sqlx::query_as::<_, InvoiceListRow>(
        "SELECT i.invoice_id, i.invoice_number, i.flat_id, f.wing, f.flat_number, \
                i.fiscal_year_id, i.billing_period_start, i.billing_period_end, i.due_date, \
                i.subtotal, i.gst_amount, i.total_amount, i.amount_paid, i.status, \
                i.gstin_applicable, i.created_by, i.created_at \
         FROM invoices i \
         JOIN flats f ON f.flat_id = i.flat_id \
         WHERE ($1::INT IS NULL OR i.flat_id = $1) AND ($2::VARCHAR IS NULL OR i.status = $2) \
         ORDER BY i.created_at DESC",
    )
    .bind(flat_id)
    .bind(status)
    .fetch_all(conn)
    .await
}

/// Every invoice for any of `flat_ids` — the resident-facing "my bills" query.
pub async fn list_invoices_for_flats(
    conn: &mut sqlx::PgConnection,
    flat_ids: &[i32],
) -> Result<Vec<Invoice>, sqlx::Error> {
    sqlx::query_as::<_, Invoice>(
        "SELECT invoice_id, invoice_number, flat_id, fiscal_year_id, billing_period_start, \
                billing_period_end, due_date, subtotal, gst_amount, total_amount, amount_paid, \
                status, gstin_applicable, created_by, created_at \
         FROM invoices WHERE flat_id = ANY($1) ORDER BY created_at DESC",
    )
    .bind(flat_ids)
    .fetch_all(conn)
    .await
}

pub async fn void_invoice(
    conn: &mut sqlx::PgConnection,
    invoice_id: InvoiceId,
    reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE invoices SET status = 'VOID', voided_at = NOW(), void_reason = $1 \
         WHERE invoice_id = $2",
    )
    .bind(reason)
    .bind(invoice_id)
    .execute(conn)
    .await?;
    Ok(())
}

/// Overdue invoices with no interest charge posted today yet (safe to re-run same-day).
pub async fn find_chargeable_overdue_invoices(
    conn: &mut sqlx::PgConnection,
    as_of: NaiveDate,
) -> Result<Vec<Invoice>, sqlx::Error> {
    sqlx::query_as::<_, Invoice>(
        "SELECT i.invoice_id, i.invoice_number, i.flat_id, i.fiscal_year_id, \
                i.billing_period_start, i.billing_period_end, i.due_date, i.subtotal, \
                i.gst_amount, i.total_amount, i.amount_paid, i.status, i.gstin_applicable, \
                i.created_by, i.created_at \
         FROM invoices i \
         WHERE i.status IN ('UNPAID', 'PARTIALLY_PAID') \
           AND i.due_date < $1 \
           AND NOT EXISTS (
               SELECT 1 FROM interest_charges ic \
               WHERE ic.invoice_id = i.invoice_id AND ic.charged_date = $1
           )",
    )
    .bind(as_of)
    .fetch_all(conn)
    .await
}

/// Total interest already posted for this invoice across all prior charge runs.
pub async fn sum_interest_charged(
    conn: &mut sqlx::PgConnection,
    invoice_id: InvoiceId,
) -> Result<Decimal, sqlx::Error> {
    sqlx::query_scalar::<_, Decimal>(
        "SELECT COALESCE(SUM(interest_amount), 0) FROM interest_charges WHERE invoice_id = $1",
    )
    .bind(invoice_id)
    .fetch_one(conn)
    .await
}

/// Every ledger transaction a prior interest run posted for this invoice.
pub async fn get_interest_charge_transaction_ids(
    conn: &mut sqlx::PgConnection,
    invoice_id: InvoiceId,
) -> Result<Vec<TransactionId>, sqlx::Error> {
    sqlx::query_scalar::<_, TransactionId>(
        "SELECT ledger_transaction_id FROM interest_charges WHERE invoice_id = $1 ORDER BY charged_date",
    )
    .bind(invoice_id)
    .fetch_all(conn)
    .await
}

pub struct NewInterestCharge {
    pub invoice_id: InvoiceId,
    pub interest_rule_id: InterestRuleId,
    pub overdue_amount: Decimal,
    pub days_overdue: i32,
    pub interest_amount: Decimal,
    pub charged_date: NaiveDate,
    pub ledger_transaction_id: crate::TransactionId,
}

pub async fn insert_interest_charge(
    conn: &mut sqlx::PgConnection,
    charge: NewInterestCharge,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO interest_charges \
            (invoice_id, interest_rule_id, overdue_amount, days_overdue, interest_amount, \
             charged_date, ledger_transaction_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(charge.invoice_id)
    .bind(charge.interest_rule_id)
    .bind(charge.overdue_amount)
    .bind(charge.days_overdue)
    .bind(charge.interest_amount)
    .bind(charge.charged_date)
    .bind(charge.ledger_transaction_id)
    .execute(conn)
    .await?;
    Ok(())
}

/// Catches a duplicate reference-less (cash) payment: an identical-looking one recorded within `window_seconds`.
pub async fn find_recent_similar_reference_less_payment(
    conn: &mut sqlx::PgConnection,
    invoice_id: InvoiceId,
    amount: Decimal,
    payment_method: &str,
    recorded_by: i32,
    window_seconds: i64,
) -> Result<Option<Payment>, sqlx::Error> {
    sqlx::query_as::<_, Payment>(
        "SELECT payment_id, invoice_id, amount, payment_method, reference_number, recorded_by, confirmed_at \
         FROM payments \
         WHERE invoice_id = $1 AND amount = $2 AND payment_method = $3 AND recorded_by = $4 \
           AND reference_number IS NULL \
           AND confirmed_at > NOW() - make_interval(secs => $5) \
         ORDER BY confirmed_at DESC LIMIT 1",
    )
    .bind(invoice_id)
    .bind(amount)
    .bind(payment_method)
    .bind(recorded_by)
    .bind(window_seconds as f64)
    .fetch_optional(conn)
    .await
}

/// `None` reference_number never matches — cash payments have nothing to de-duplicate against.
pub async fn find_payment_by_reference(
    conn: &mut sqlx::PgConnection,
    payment_method: &str,
    reference_number: &str,
) -> Result<Option<Payment>, sqlx::Error> {
    sqlx::query_as::<_, Payment>(
        "SELECT payment_id, invoice_id, amount, payment_method, reference_number, recorded_by, confirmed_at \
         FROM payments WHERE payment_method = $1 AND reference_number = $2",
    )
    .bind(payment_method)
    .bind(reference_number)
    .fetch_optional(conn)
    .await
}

pub struct NewPayment<'a> {
    pub invoice_id: InvoiceId,
    pub amount: Decimal,
    pub payment_method: &'a str,
    pub reference_number: Option<&'a str>,
    pub recorded_by: i32,
    pub ledger_transaction_id: crate::TransactionId,
}

pub async fn insert_payment(
    conn: &mut sqlx::PgConnection,
    payment: NewPayment<'_>,
) -> Result<PaymentId, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO payments (invoice_id, amount, payment_method, reference_number, recorded_by, ledger_transaction_id) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING payment_id",
    )
    .bind(payment.invoice_id)
    .bind(payment.amount)
    .bind(payment.payment_method)
    .bind(payment.reference_number)
    .bind(payment.recorded_by)
    .bind(payment.ledger_transaction_id)
    .fetch_one(conn)
    .await
}

/// Guarded on `status <> 'VOID'` as defense-in-depth. Returns whether a row was updated.
pub async fn apply_payment_to_invoice(
    conn: &mut sqlx::PgConnection,
    invoice_id: InvoiceId,
    amount: Decimal,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE invoices SET \
            amount_paid = amount_paid + $1, \
            status = CASE \
                WHEN amount_paid + $1 >= total_amount THEN 'PAID' \
                ELSE 'PARTIALLY_PAID' \
            END \
         WHERE invoice_id = $2 AND status <> 'VOID'",
    )
    .bind(amount)
    .bind(invoice_id)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn insert_receipt(
    conn: &mut sqlx::PgConnection,
    receipt_number: &str,
    payment_id: PaymentId,
) -> Result<ReceiptId, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO receipts (receipt_number, payment_id) VALUES ($1, $2) RETURNING receipt_id",
    )
    .bind(receipt_number)
    .bind(payment_id)
    .fetch_one(conn)
    .await
}

pub async fn get_receipt_by_payment(
    conn: &mut sqlx::PgConnection,
    payment_id: PaymentId,
) -> Result<Option<Receipt>, sqlx::Error> {
    sqlx::query_as::<_, Receipt>(
        "SELECT receipt_id, receipt_number, payment_id, generated_at \
         FROM receipts WHERE payment_id = $1",
    )
    .bind(payment_id)
    .fetch_optional(conn)
    .await
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct PaymentWithReceipt {
    pub payment_id: PaymentId,
    pub invoice_id: InvoiceId,
    pub invoice_number: String,
    pub flat_id: i32,
    pub amount: Decimal,
    pub payment_method: String,
    pub reference_number: Option<String>,
    pub confirmed_at: DateTime<Utc>,
    pub receipt_number: String,
}

pub async fn list_payments(
    conn: &mut sqlx::PgConnection,
    invoice_id: Option<InvoiceId>,
    flat_id: Option<i32>,
) -> Result<Vec<PaymentWithReceipt>, sqlx::Error> {
    sqlx::query_as::<_, PaymentWithReceipt>(
        "SELECT p.payment_id, p.invoice_id, i.invoice_number, i.flat_id, p.amount, \
                p.payment_method, p.reference_number, p.confirmed_at, r.receipt_number \
         FROM payments p \
         JOIN invoices i ON i.invoice_id = p.invoice_id \
         JOIN receipts r ON r.payment_id = p.payment_id \
         WHERE ($1::UUID IS NULL OR p.invoice_id = $1) \
           AND ($2::INT IS NULL OR i.flat_id = $2) \
         ORDER BY p.confirmed_at DESC",
    )
    .bind(invoice_id)
    .bind(flat_id)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct PaymentClaim {
    pub claim_id: ClaimId,
    pub invoice_id: InvoiceId,
    pub invoice_number: String,
    pub flat_id: i32,
    pub wing: String,
    pub flat_number: String,
    pub claimed_by: i32,
    pub amount: Decimal,
    pub payment_method: String,
    pub reference_number: String,
    pub note: Option<String>,
    pub status: String,
    pub submitted_at: DateTime<Utc>,
    pub reviewed_by: Option<i32>,
    pub reviewed_at: Option<DateTime<Utc>>,
    pub review_note: Option<String>,
    pub payment_id: Option<PaymentId>,
}

const PAYMENT_CLAIM_COLUMNS: &str = "c.claim_id, c.invoice_id, i.invoice_number, i.flat_id, \
     f.wing, f.flat_number, \
     c.claimed_by, c.amount, c.payment_method, c.reference_number, c.note, c.status, \
     c.submitted_at, c.reviewed_by, c.reviewed_at, c.review_note, c.payment_id";

pub struct NewPaymentClaim<'a> {
    pub invoice_id: InvoiceId,
    pub claimed_by: i32,
    pub amount: Decimal,
    pub payment_method: &'a str,
    pub reference_number: &'a str,
    pub note: Option<&'a str>,
}

pub async fn insert_payment_claim(
    conn: &mut sqlx::PgConnection,
    claim: NewPaymentClaim<'_>,
) -> Result<ClaimId, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO payment_claims (invoice_id, claimed_by, amount, payment_method, reference_number, note) \
         VALUES ($1, $2, $3, $4, $5, $6) RETURNING claim_id",
    )
    .bind(claim.invoice_id)
    .bind(claim.claimed_by)
    .bind(claim.amount)
    .bind(claim.payment_method)
    .bind(claim.reference_number)
    .bind(claim.note)
    .fetch_one(conn)
    .await
}

/// Looks up an already-PENDING claim with the same reference, for `submit_claim`'s idempotency check.
pub async fn find_pending_claim_by_reference(
    conn: &mut sqlx::PgConnection,
    payment_method: &str,
    reference_number: &str,
) -> Result<Option<(ClaimId, InvoiceId)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT claim_id, invoice_id FROM payment_claims \
         WHERE payment_method = $1 AND reference_number = $2 AND status = 'PENDING'",
    )
    .bind(payment_method)
    .bind(reference_number)
    .fetch_optional(conn)
    .await
}

pub async fn get_payment_claim(
    conn: &mut sqlx::PgConnection,
    claim_id: ClaimId,
) -> Result<Option<PaymentClaim>, sqlx::Error> {
    sqlx::query_as::<_, PaymentClaim>(&format!(
        "SELECT {PAYMENT_CLAIM_COLUMNS} FROM payment_claims c \
         JOIN invoices i ON i.invoice_id = c.invoice_id \
         JOIN flats f ON f.flat_id = i.flat_id WHERE c.claim_id = $1",
    ))
    .bind(claim_id)
    .fetch_optional(conn)
    .await
}

/// Same as `get_payment_claim`, but row-locks it — required before a confirm/reject decision.
pub async fn get_payment_claim_for_update(
    conn: &mut sqlx::PgConnection,
    claim_id: ClaimId,
) -> Result<Option<PaymentClaim>, sqlx::Error> {
    sqlx::query_as::<_, PaymentClaim>(&format!(
        "SELECT {PAYMENT_CLAIM_COLUMNS} FROM payment_claims c \
         JOIN invoices i ON i.invoice_id = c.invoice_id \
         JOIN flats f ON f.flat_id = i.flat_id WHERE c.claim_id = $1 FOR UPDATE OF c",
    ))
    .bind(claim_id)
    .fetch_optional(conn)
    .await
}

/// `claimed_by` scopes to one resident's claims; `None` lists everyone's (admin queue).
pub async fn list_payment_claims(
    conn: &mut sqlx::PgConnection,
    status: Option<&str>,
    claimed_by: Option<i32>,
) -> Result<Vec<PaymentClaim>, sqlx::Error> {
    sqlx::query_as::<_, PaymentClaim>(&format!(
        "SELECT {PAYMENT_CLAIM_COLUMNS} FROM payment_claims c \
         JOIN invoices i ON i.invoice_id = c.invoice_id \
         JOIN flats f ON f.flat_id = i.flat_id \
         WHERE ($1::VARCHAR IS NULL OR c.status = $1) \
           AND ($2::INT IS NULL OR c.claimed_by = $2) \
         ORDER BY c.submitted_at DESC",
    ))
    .bind(status)
    .bind(claimed_by)
    .fetch_all(conn)
    .await
}

/// Guarded on `status = 'PENDING'` as defense-in-depth. Returns whether it updated a row.
pub async fn mark_claim_confirmed(
    conn: &mut sqlx::PgConnection,
    claim_id: ClaimId,
    reviewed_by: i32,
    payment_id: PaymentId,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE payment_claims SET status = 'CONFIRMED', reviewed_by = $1, reviewed_at = NOW(), \
            payment_id = $2 \
         WHERE claim_id = $3 AND status = 'PENDING'",
    )
    .bind(reviewed_by)
    .bind(payment_id)
    .bind(claim_id)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Guarded on `status = 'PENDING'` — see `mark_claim_confirmed`.
pub async fn mark_claim_rejected(
    conn: &mut sqlx::PgConnection,
    claim_id: ClaimId,
    reviewed_by: i32,
    review_note: &str,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE payment_claims SET status = 'REJECTED', reviewed_by = $1, reviewed_at = NOW(), \
            review_note = $2 \
         WHERE claim_id = $3 AND status = 'PENDING'",
    )
    .bind(reviewed_by)
    .bind(review_note)
    .bind(claim_id)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() > 0)
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct PaymentInstructions {
    pub account_holder_name: String,
    pub bank_name: String,
    pub account_number: String,
    pub ifsc_code: String,
    pub branch: Option<String>,
    pub contact_email: Option<String>,
}

pub async fn get_payment_instructions(
    conn: &mut sqlx::PgConnection,
) -> Result<Option<PaymentInstructions>, sqlx::Error> {
    sqlx::query_as::<_, PaymentInstructions>(
        "SELECT account_holder_name, bank_name, account_number, ifsc_code, branch, contact_email \
         FROM payment_instructions ORDER BY payment_instructions_id DESC LIMIT 1",
    )
    .fetch_optional(conn)
    .await
}

pub struct SetPaymentInstructions<'a> {
    pub account_holder_name: &'a str,
    pub bank_name: &'a str,
    pub account_number: &'a str,
    pub ifsc_code: &'a str,
    pub branch: Option<&'a str>,
    pub contact_email: Option<&'a str>,
    pub updated_by: i32,
}

pub async fn set_payment_instructions(
    conn: &mut sqlx::PgConnection,
    instructions: SetPaymentInstructions<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO payment_instructions \
            (account_holder_name, bank_name, account_number, ifsc_code, branch, contact_email, updated_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(instructions.account_holder_name)
    .bind(instructions.bank_name)
    .bind(instructions.account_number)
    .bind(instructions.ifsc_code)
    .bind(instructions.branch)
    .bind(instructions.contact_email)
    .bind(instructions.updated_by)
    .execute(conn)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct SocietyProfile {
    pub name: String,
    pub registration_number: Option<String>,
    pub address: String,
    pub state: Option<String>,
    pub phone: Option<String>,
}

pub async fn get_society_profile(
    conn: &mut sqlx::PgConnection,
) -> Result<Option<SocietyProfile>, sqlx::Error> {
    sqlx::query_as::<_, SocietyProfile>(
        "SELECT name, registration_number, address, state, phone \
         FROM society_profile ORDER BY society_profile_id DESC LIMIT 1",
    )
    .fetch_optional(conn)
    .await
}

pub struct SetSocietyProfile<'a> {
    pub name: &'a str,
    pub registration_number: Option<&'a str>,
    pub address: &'a str,
    pub state: Option<&'a str>,
    pub phone: Option<&'a str>,
    pub updated_by: i32,
}

pub async fn set_society_profile(
    conn: &mut sqlx::PgConnection,
    profile: SetSocietyProfile<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO society_profile (name, registration_number, address, state, phone, updated_by) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(profile.name)
    .bind(profile.registration_number)
    .bind(profile.address)
    .bind(profile.state)
    .bind(profile.phone)
    .bind(profile.updated_by)
    .execute(conn)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct FlatIdentity {
    pub wing: String,
    pub flat_number: String,
}

pub async fn get_flat_identity(
    conn: &mut sqlx::PgConnection,
    flat_id: i32,
) -> Result<Option<FlatIdentity>, sqlx::Error> {
    sqlx::query_as::<_, FlatIdentity>("SELECT wing, flat_number FROM flats WHERE flat_id = $1")
        .bind(flat_id)
        .fetch_optional(conn)
        .await
}

pub async fn user_owns_flat(
    conn: &mut sqlx::PgConnection,
    user_id: i32,
    flat_id: i32,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM flat_ownerships WHERE flat_id = $1 AND user_id = $2 AND is_active = TRUE)",
    )
    .bind(flat_id)
    .bind(user_id)
    .fetch_one(conn)
    .await
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct InvoiceLineItem {
    pub description: String,
    pub amount: Decimal,
    pub gst_rate_pct: Decimal,
    pub hsn_sac_code: Option<String>,
    pub gst_amount: Decimal,
}

pub async fn get_invoice_line_items(
    conn: &mut sqlx::PgConnection,
    invoice_id: InvoiceId,
) -> Result<Vec<InvoiceLineItem>, sqlx::Error> {
    sqlx::query_as::<_, InvoiceLineItem>(
        "SELECT description, amount, gst_rate_pct, hsn_sac_code, gst_amount \
         FROM invoice_line_items WHERE invoice_id = $1",
    )
    .bind(invoice_id)
    .fetch_all(conn)
    .await
}

pub async fn get_payment(
    conn: &mut sqlx::PgConnection,
    payment_id: PaymentId,
) -> Result<Option<Payment>, sqlx::Error> {
    sqlx::query_as::<_, Payment>(
        "SELECT payment_id, invoice_id, amount, payment_method, reference_number, recorded_by, confirmed_at \
         FROM payments WHERE payment_id = $1",
    )
    .bind(payment_id)
    .fetch_optional(conn)
    .await
}

pub async fn has_outstanding_dues(
    conn: &mut sqlx::PgConnection,
    flat_id: i32,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM invoices WHERE flat_id = $1 AND status IN ('UNPAID', 'PARTIALLY_PAID'))",
    )
    .bind(flat_id)
    .fetch_one(conn)
    .await
}

/// The current active owner's email for a flat, for display on documents (NOC).
pub async fn get_flat_owner_email(
    conn: &mut sqlx::PgConnection,
    flat_id: i32,
) -> Result<Option<String>, sqlx::Error> {
    let email: Option<Option<String>> = sqlx::query_scalar(
        "SELECT u.email FROM flat_ownerships fo JOIN users u ON u.user_id = fo.user_id \
         WHERE fo.flat_id = $1 AND fo.is_active = TRUE LIMIT 1",
    )
    .bind(flat_id)
    .fetch_optional(conn)
    .await?;
    Ok(email.flatten())
}

// ===========================================================================
// Funds & contribution rules
// ===========================================================================

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct Fund {
    pub fund_id: FundId,
    pub code: String,
    pub name: String,
    pub is_restricted: bool,
}

pub async fn list_funds(conn: &mut sqlx::PgConnection) -> Result<Vec<Fund>, sqlx::Error> {
    sqlx::query_as::<_, Fund>("SELECT fund_id, code, name, is_restricted FROM funds ORDER BY code")
        .fetch_all(conn)
        .await
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct FundContributionRule {
    pub rule_id: FundRuleId,
    pub fund_id: FundId,
    pub contribution_type: String,
    pub amount_or_pct: Decimal,
    pub effective_from: NaiveDate,
}

pub struct NewFundContributionRule {
    pub fund_id: FundId,
    pub contribution_type: String,
    pub amount_or_pct: Decimal,
    pub effective_from: NaiveDate,
}

pub async fn insert_fund_contribution_rule(
    conn: &mut sqlx::PgConnection,
    rule: NewFundContributionRule,
) -> Result<FundRuleId, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO fund_contribution_rules (fund_id, contribution_type, amount_or_pct, effective_from) \
         VALUES ($1, $2, $3, $4) RETURNING rule_id",
    )
    .bind(rule.fund_id)
    .bind(rule.contribution_type)
    .bind(rule.amount_or_pct)
    .bind(rule.effective_from)
    .fetch_one(conn)
    .await
}

pub async fn assign_flat_to_fund(
    conn: &mut sqlx::PgConnection,
    flat_id: i32,
    fund_id: FundId,
) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO flat_fund_assignments (flat_id, fund_id) VALUES ($1, $2) ON CONFLICT DO NOTHING")
        .bind(flat_id)
        .bind(fund_id)
        .execute(conn)
        .await?;
    Ok(())
}

/// Active contribution rules for whichever fund(s) `flat_id` is assigned to, as of `as_of`.
pub async fn get_fund_contribution_rules_for_flat(
    conn: &mut sqlx::PgConnection,
    flat_id: i32,
    as_of: NaiveDate,
) -> Result<Vec<FundContributionRule>, sqlx::Error> {
    sqlx::query_as::<_, FundContributionRule>(
        "SELECT DISTINCT ON (r.fund_id) r.rule_id, r.fund_id, r.contribution_type, \
                r.amount_or_pct, r.effective_from \
         FROM fund_contribution_rules r \
         JOIN flat_fund_assignments a ON a.fund_id = r.fund_id \
         WHERE a.flat_id = $1 AND r.is_active = TRUE AND r.effective_from <= $2 \
         ORDER BY r.fund_id, r.effective_from DESC",
    )
    .bind(flat_id)
    .bind(as_of)
    .fetch_all(conn)
    .await
}

/// Every active fund contribution rule, regardless of which flats it's assigned to.
pub async fn list_all_fund_contribution_rules(
    conn: &mut sqlx::PgConnection,
) -> Result<Vec<FundContributionRule>, sqlx::Error> {
    sqlx::query_as::<_, FundContributionRule>(
        "SELECT rule_id, fund_id, contribution_type, amount_or_pct, effective_from \
         FROM fund_contribution_rules WHERE is_active = TRUE ORDER BY fund_id, effective_from DESC",
    )
    .fetch_all(conn)
    .await
}

/// The income account a fund's contributions post to (code `"6" || fund_id`), created lazily.
pub async fn get_or_create_fund_income_account(
    conn: &mut sqlx::PgConnection,
    fund_id: FundId,
    fund_name: &str,
) -> Result<AccountId, sqlx::Error> {
    let code = format!("6{}", fund_id.0);
    if let Some(id) = sqlx::query_scalar::<_, AccountId>(
        "SELECT account_id FROM chart_of_accounts WHERE code = $1",
    )
    .bind(&code)
    .fetch_optional(&mut *conn)
    .await?
    {
        return Ok(id);
    }
    sqlx::query_scalar(
        "INSERT INTO chart_of_accounts (code, name, account_type, normal_balance, fund_id, is_system) \
         VALUES ($1, $2, 'INCOME', 'CREDIT', $3, TRUE) RETURNING account_id",
    )
    .bind(&code)
    .bind(format!("{} Contributions", fund_name))
    .bind(fund_id)
    .fetch_one(conn)
    .await
}

// ===========================================================================
// TDS
// ===========================================================================

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct TdsRule {
    pub tds_rule_id: TdsRuleId,
    pub section: String,
    pub payment_category: String,
    pub rate_pct: Decimal,
    pub threshold_single_payment: Decimal,
    pub threshold_annual_aggregate: Decimal,
    pub created_by: i32,
}

pub async fn list_active_tds_rules(
    conn: &mut sqlx::PgConnection,
) -> Result<Vec<TdsRule>, sqlx::Error> {
    sqlx::query_as::<_, TdsRule>(
        "SELECT tds_rule_id, section, payment_category, rate_pct, threshold_single_payment, \
                threshold_annual_aggregate, created_by \
         FROM tds_rules WHERE is_active = TRUE ORDER BY effective_from DESC",
    )
    .fetch_all(conn)
    .await
}

pub struct NewTdsRule<'a> {
    pub section: &'a str,
    pub payment_category: &'a str,
    pub rate_pct: Decimal,
    pub threshold_single_payment: Decimal,
    pub threshold_annual_aggregate: Decimal,
    pub effective_from: NaiveDate,
    pub created_by: i32,
}

pub async fn insert_tds_rule(
    conn: &mut sqlx::PgConnection,
    rule: NewTdsRule<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO tds_rules \
            (section, payment_category, rate_pct, threshold_single_payment, threshold_annual_aggregate, effective_from, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(rule.section)
    .bind(rule.payment_category)
    .bind(rule.rate_pct)
    .bind(rule.threshold_single_payment)
    .bind(rule.threshold_annual_aggregate)
    .bind(rule.effective_from)
    .bind(rule.created_by)
    .execute(conn)
    .await?;
    Ok(())
}

/// Sum of gross vendor-expense amounts already recorded against this vendor this fiscal year.
pub async fn ytd_vendor_amount(
    conn: &mut sqlx::PgConnection,
    vendor_id: VendorId,
    fiscal_year_id: FiscalYearId,
) -> Result<Decimal, sqlx::Error> {
    let total: Option<Decimal> = sqlx::query_scalar(
        "SELECT SUM(tr.gross_amount) FROM tds_records tr \
         WHERE tr.vendor_id = $1 AND tr.fiscal_year_id = $2",
    )
    .bind(vendor_id)
    .bind(fiscal_year_id)
    .fetch_one(conn)
    .await?;
    Ok(total.unwrap_or(Decimal::ZERO))
}

pub struct NewTdsRecord {
    pub expense_id: ExpenseId,
    pub tds_rule_id: TdsRuleId,
    pub vendor_id: VendorId,
    pub fiscal_year_id: FiscalYearId,
    pub gross_amount: Decimal,
    pub tds_amount: Decimal,
    pub deducted_on: NaiveDate,
}

pub async fn insert_tds_record(
    conn: &mut sqlx::PgConnection,
    record: NewTdsRecord,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO tds_records (expense_id, tds_rule_id, vendor_id, fiscal_year_id, gross_amount, tds_amount, deducted_on) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(record.expense_id)
    .bind(record.tds_rule_id)
    .bind(record.vendor_id)
    .bind(record.fiscal_year_id)
    .bind(record.gross_amount)
    .bind(record.tds_amount)
    .bind(record.deducted_on)
    .execute(conn)
    .await?;
    Ok(())
}

/// Deletes the TDS record `submit_expense` speculatively created, if the expense is rejected.
pub async fn delete_tds_record_for_expense(
    conn: &mut sqlx::PgConnection,
    expense_id: ExpenseId,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM tds_records WHERE expense_id = $1")
        .bind(expense_id)
        .execute(conn)
        .await?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct TdsSummaryRow {
    pub section: String,
    pub gross_amount: Decimal,
    pub tds_amount: Decimal,
    pub deducted_on: NaiveDate,
    pub deposited_challan_number: Option<String>,
}

pub async fn tds_summary_for_vendor(
    conn: &mut sqlx::PgConnection,
    vendor_id: VendorId,
    fiscal_year_id: FiscalYearId,
) -> Result<Vec<TdsSummaryRow>, sqlx::Error> {
    sqlx::query_as::<_, TdsSummaryRow>(
        "SELECT r.section, tr.gross_amount, tr.tds_amount, tr.deducted_on, tr.deposited_challan_number \
         FROM tds_records tr \
         JOIN tds_rules r ON r.tds_rule_id = tr.tds_rule_id \
         WHERE tr.vendor_id = $1 AND tr.fiscal_year_id = $2 \
         ORDER BY tr.deducted_on",
    )
    .bind(vendor_id)
    .bind(fiscal_year_id)
    .fetch_all(conn)
    .await
}

// ===========================================================================
// Vendors, purchase orders, expenses
// ===========================================================================

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct Vendor {
    pub vendor_id: VendorId,
    pub name: String,
    pub category: String,
    pub gstin: Option<String>,
    pub pan: Option<String>,
    pub is_active: bool,
}

pub struct NewVendor<'a> {
    pub name: &'a str,
    pub category: &'a str,
    pub gstin: Option<&'a str>,
    pub pan: Option<&'a str>,
}

pub async fn insert_vendor(
    conn: &mut sqlx::PgConnection,
    vendor: NewVendor<'_>,
) -> Result<VendorId, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO vendors (name, category, gstin, pan) VALUES ($1, $2, $3, $4) RETURNING vendor_id",
    )
    .bind(vendor.name)
    .bind(vendor.category)
    .bind(vendor.gstin)
    .bind(vendor.pan)
    .fetch_one(conn)
    .await
}

pub async fn list_vendors(conn: &mut sqlx::PgConnection) -> Result<Vec<Vendor>, sqlx::Error> {
    sqlx::query_as::<_, Vendor>(
        "SELECT vendor_id, name, category, gstin, pan, is_active FROM vendors WHERE is_active = TRUE ORDER BY name",
    )
    .fetch_all(conn)
    .await
}

pub async fn get_vendor(
    conn: &mut sqlx::PgConnection,
    vendor_id: VendorId,
) -> Result<Option<Vendor>, sqlx::Error> {
    sqlx::query_as::<_, Vendor>(
        "SELECT vendor_id, name, category, gstin, pan, is_active FROM vendors WHERE vendor_id = $1",
    )
    .bind(vendor_id)
    .fetch_optional(conn)
    .await
}

pub async fn next_po_number(conn: &mut sqlx::PgConnection) -> Result<String, sqlx::Error> {
    let n: i64 = sqlx::query_scalar("SELECT nextval('po_number_seq')")
        .fetch_one(conn)
        .await?;
    Ok(format!("PO-{:06}", n))
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct PurchaseOrder {
    pub po_id: PoId,
    pub po_number: String,
    pub vendor_id: VendorId,
    pub description: String,
    pub amount: Decimal,
    pub status: String,
}

pub struct NewPurchaseOrder<'a> {
    pub po_number: &'a str,
    pub vendor_id: VendorId,
    pub description: &'a str,
    pub amount: Decimal,
    pub created_by: i32,
}

pub async fn insert_purchase_order(
    conn: &mut sqlx::PgConnection,
    po: NewPurchaseOrder<'_>,
) -> Result<PoId, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO purchase_orders (po_number, vendor_id, description, amount, status, created_by) \
         VALUES ($1, $2, $3, $4, 'ISSUED', $5) RETURNING po_id",
    )
    .bind(po.po_number)
    .bind(po.vendor_id)
    .bind(po.description)
    .bind(po.amount)
    .bind(po.created_by)
    .fetch_one(conn)
    .await
}

pub async fn get_purchase_order(
    conn: &mut sqlx::PgConnection,
    po_id: PoId,
) -> Result<Option<PurchaseOrder>, sqlx::Error> {
    sqlx::query_as::<_, PurchaseOrder>(
        "SELECT po_id, po_number, vendor_id, description, amount, status FROM purchase_orders \
         WHERE po_id = $1",
    )
    .bind(po_id)
    .fetch_optional(conn)
    .await
}

pub async fn list_purchase_orders(
    conn: &mut sqlx::PgConnection,
    vendor_id: Option<VendorId>,
) -> Result<Vec<PurchaseOrder>, sqlx::Error> {
    sqlx::query_as::<_, PurchaseOrder>(
        "SELECT po_id, po_number, vendor_id, description, amount, status FROM purchase_orders \
         WHERE ($1::INT IS NULL OR vendor_id = $1) ORDER BY created_at DESC",
    )
    .bind(vendor_id)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct VendorExpense {
    pub expense_id: ExpenseId,
    pub po_id: Option<PoId>,
    pub vendor_id: VendorId,
    pub account_id: AccountId,
    pub vendor_invoice_number: Option<String>,
    pub amount: Decimal,
    pub tds_amount: Decimal,
    pub net_payable: Decimal,
    pub status: String,
    pub created_by: i32,
    pub created_at: DateTime<Utc>,
}

pub struct NewVendorExpense<'a> {
    pub po_id: Option<PoId>,
    pub vendor_id: VendorId,
    pub account_id: AccountId,
    pub vendor_invoice_number: Option<&'a str>,
    pub invoice_document_id: Option<Uuid>,
    pub amount: Decimal,
    pub tds_amount: Decimal,
    pub net_payable: Decimal,
    pub created_by: i32,
}

pub async fn insert_vendor_expense(
    conn: &mut sqlx::PgConnection,
    expense: NewVendorExpense<'_>,
) -> Result<ExpenseId, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO vendor_expenses \
            (po_id, vendor_id, account_id, vendor_invoice_number, invoice_document_id, amount, tds_amount, net_payable, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING expense_id",
    )
    .bind(expense.po_id)
    .bind(expense.vendor_id)
    .bind(expense.account_id)
    .bind(expense.vendor_invoice_number)
    .bind(expense.invoice_document_id)
    .bind(expense.amount)
    .bind(expense.tds_amount)
    .bind(expense.net_payable)
    .bind(expense.created_by)
    .fetch_one(conn)
    .await
}

pub async fn get_vendor_expense(
    conn: &mut sqlx::PgConnection,
    expense_id: ExpenseId,
) -> Result<Option<VendorExpense>, sqlx::Error> {
    sqlx::query_as::<_, VendorExpense>(
        "SELECT expense_id, po_id, vendor_id, account_id, vendor_invoice_number, amount, \
                tds_amount, net_payable, status, created_by, created_at \
         FROM vendor_expenses WHERE expense_id = $1",
    )
    .bind(expense_id)
    .fetch_optional(conn)
    .await
}

/// Same as `get_vendor_expense`, but row-locks it — required before flipping `status`.
pub async fn get_vendor_expense_for_update(
    conn: &mut sqlx::PgConnection,
    expense_id: ExpenseId,
) -> Result<Option<VendorExpense>, sqlx::Error> {
    sqlx::query_as::<_, VendorExpense>(
        "SELECT expense_id, po_id, vendor_id, account_id, vendor_invoice_number, amount, \
                tds_amount, net_payable, status, created_by, created_at \
         FROM vendor_expenses WHERE expense_id = $1 FOR UPDATE",
    )
    .bind(expense_id)
    .fetch_optional(conn)
    .await
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct VendorExpenseListRow {
    pub expense_id: ExpenseId,
    pub po_id: Option<PoId>,
    pub vendor_id: VendorId,
    pub vendor_name: String,
    pub account_id: AccountId,
    pub vendor_invoice_number: Option<String>,
    pub amount: Decimal,
    pub tds_amount: Decimal,
    pub net_payable: Decimal,
    pub status: String,
    pub created_by: i32,
    pub created_at: DateTime<Utc>,
}

/// The admin-facing expense list, enriched with the vendor's name.
pub async fn list_vendor_expenses(
    conn: &mut sqlx::PgConnection,
    status: Option<&str>,
) -> Result<Vec<VendorExpenseListRow>, sqlx::Error> {
    sqlx::query_as::<_, VendorExpenseListRow>(
        "SELECT e.expense_id, e.po_id, e.vendor_id, v.name AS vendor_name, e.account_id, \
                e.vendor_invoice_number, e.amount, e.tds_amount, e.net_payable, e.status, \
                e.created_by, e.created_at \
         FROM vendor_expenses e \
         JOIN vendors v ON v.vendor_id = e.vendor_id \
         WHERE ($1::VARCHAR IS NULL OR e.status = $1) ORDER BY e.created_at DESC",
    )
    .bind(status)
    .fetch_all(conn)
    .await
}

pub async fn mark_expense_approved(
    conn: &mut sqlx::PgConnection,
    expense_id: ExpenseId,
    reviewed_by: i32,
    ledger_transaction_id: TransactionId,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE vendor_expenses SET status = 'APPROVED', reviewed_by = $1, reviewed_at = NOW(), \
            ledger_transaction_id = $2 \
         WHERE expense_id = $3",
    )
    .bind(reviewed_by)
    .bind(ledger_transaction_id)
    .bind(expense_id)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn mark_expense_rejected(
    conn: &mut sqlx::PgConnection,
    expense_id: ExpenseId,
    reviewed_by: i32,
    reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE vendor_expenses SET status = 'REJECTED', reviewed_by = $1, reviewed_at = NOW(), \
            rejection_reason = $2 \
         WHERE expense_id = $3",
    )
    .bind(reviewed_by)
    .bind(reason)
    .bind(expense_id)
    .execute(conn)
    .await?;
    Ok(())
}

// ===========================================================================
// Bank reconciliation
// ===========================================================================

pub async fn insert_bank_import(
    conn: &mut sqlx::PgConnection,
    imported_by: i32,
    source_filename: Option<&str>,
) -> Result<BankImportId, sqlx::Error> {
    sqlx::query_scalar(
        "INSERT INTO bank_statement_imports (imported_by, source_filename) VALUES ($1, $2) RETURNING import_id",
    )
    .bind(imported_by)
    .bind(source_filename)
    .fetch_one(conn)
    .await
}

pub async fn insert_bank_statement_lines(
    conn: &mut sqlx::PgConnection,
    import_id: BankImportId,
    dates: &[NaiveDate],
    descriptions: &[String],
    amounts: &[Decimal],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO bank_statement_lines (import_id, transaction_date, description, amount) \
         SELECT $1, * FROM UNNEST($2::date[], $3::text[], $4::numeric[]) \
             AS t(transaction_date, description, amount)",
    )
    .bind(import_id)
    .bind(dates)
    .bind(descriptions)
    .bind(amounts)
    .execute(conn)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct BankStatementLine {
    pub line_id: BankLineId,
    pub import_id: BankImportId,
    pub transaction_date: NaiveDate,
    pub description: String,
    pub amount: Decimal,
    pub matched_ledger_transaction_id: Option<TransactionId>,
    pub match_status: String,
}

pub async fn list_bank_statement_lines(
    conn: &mut sqlx::PgConnection,
    import_id: Option<BankImportId>,
    match_status: Option<&str>,
) -> Result<Vec<BankStatementLine>, sqlx::Error> {
    sqlx::query_as::<_, BankStatementLine>(
        "SELECT line_id, import_id, transaction_date, description, amount, \
                matched_ledger_transaction_id, match_status \
         FROM bank_statement_lines \
         WHERE ($1::UUID IS NULL OR import_id = $1) AND ($2::VARCHAR IS NULL OR match_status = $2) \
         ORDER BY transaction_date DESC",
    )
    .bind(import_id)
    .bind(match_status)
    .fetch_all(conn)
    .await
}

pub async fn get_bank_statement_line(
    conn: &mut sqlx::PgConnection,
    line_id: BankLineId,
) -> Result<Option<BankStatementLine>, sqlx::Error> {
    sqlx::query_as::<_, BankStatementLine>(
        "SELECT line_id, import_id, transaction_date, description, amount, \
                matched_ledger_transaction_id, match_status \
         FROM bank_statement_lines WHERE line_id = $1",
    )
    .bind(line_id)
    .fetch_optional(conn)
    .await
}

/// Ledger transactions on `date` totaling `amount` on the bank account, not yet matched.
pub async fn find_unmatched_ledger_transactions_for_amount(
    conn: &mut sqlx::PgConnection,
    transaction_date: NaiveDate,
    amount: Decimal,
) -> Result<Vec<TransactionId>, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT lt.transaction_id \
         FROM ledger_transactions lt \
         JOIN ledger_entries le ON le.transaction_id = lt.transaction_id \
         JOIN chart_of_accounts a ON a.account_id = le.account_id AND a.code = $3 \
         WHERE lt.transaction_date = $1 AND (le.debit - le.credit) = $2::NUMERIC \
           AND NOT EXISTS ( \
               SELECT 1 FROM bank_statement_lines b WHERE b.matched_ledger_transaction_id = lt.transaction_id \
           )",
    )
    .bind(transaction_date)
    .bind(amount)
    .bind(crate::accounts::BANK_OPERATING)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct BankMatchCandidate {
    pub transaction_id: TransactionId,
    pub transaction_date: NaiveDate,
    pub description: String,
    pub amount: Decimal,
}

/// Unmatched bank-account ledger transactions within 10 days of `near_date`, closest date first.
pub async fn list_bank_match_candidates(
    conn: &mut sqlx::PgConnection,
    near_date: NaiveDate,
) -> Result<Vec<BankMatchCandidate>, sqlx::Error> {
    sqlx::query_as::<_, BankMatchCandidate>(
        "SELECT lt.transaction_id, lt.transaction_date, lt.description, \
                (le.debit - le.credit) AS amount \
         FROM ledger_transactions lt \
         JOIN ledger_entries le ON le.transaction_id = lt.transaction_id \
         JOIN chart_of_accounts a ON a.account_id = le.account_id AND a.code = $2 \
         WHERE lt.transaction_date BETWEEN $1::DATE - INTERVAL '10 days' \
                                       AND $1::DATE + INTERVAL '10 days' \
           AND NOT EXISTS ( \
               SELECT 1 FROM bank_statement_lines b WHERE b.matched_ledger_transaction_id = lt.transaction_id \
           ) \
         ORDER BY ABS(lt.transaction_date - $1::DATE), lt.transaction_date DESC \
         LIMIT 20",
    )
    .bind(near_date)
    .bind(crate::accounts::BANK_OPERATING)
    .fetch_all(conn)
    .await
}

/// Only moves a line out of UNMATCHED — a no-op if already matched/ignored. Returns whether it changed anything.
pub async fn mark_line_matched(
    conn: &mut sqlx::PgConnection,
    line_id: BankLineId,
    transaction_id: TransactionId,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE bank_statement_lines SET match_status = 'MATCHED', matched_ledger_transaction_id = $1 \
         WHERE line_id = $2 AND match_status = 'UNMATCHED'",
    )
    .bind(transaction_id)
    .bind(line_id)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Same UNMATCHED-only guard as `mark_line_matched`.
pub async fn mark_line_ignored(
    conn: &mut sqlx::PgConnection,
    line_id: BankLineId,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE bank_statement_lines SET match_status = 'IGNORED' \
         WHERE line_id = $1 AND match_status = 'UNMATCHED'",
    )
    .bind(line_id)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() > 0)
}

// ===========================================================================
// Budgets
// ===========================================================================

pub async fn get_or_create_budget(
    conn: &mut sqlx::PgConnection,
    fiscal_year_id: FiscalYearId,
    created_by: i32,
) -> Result<BudgetId, sqlx::Error> {
    if let Some(id) =
        sqlx::query_scalar::<_, BudgetId>("SELECT budget_id FROM budgets WHERE fiscal_year_id = $1")
            .bind(fiscal_year_id)
            .fetch_optional(&mut *conn)
            .await?
    {
        return Ok(id);
    }
    sqlx::query_scalar(
        "INSERT INTO budgets (fiscal_year_id, created_by) VALUES ($1, $2) RETURNING budget_id",
    )
    .bind(fiscal_year_id)
    .bind(created_by)
    .fetch_one(conn)
    .await
}

pub async fn set_budget_lines(
    conn: &mut sqlx::PgConnection,
    budget_id: BudgetId,
    account_ids: &[AccountId],
    budgeted_amounts: &[Decimal],
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO budget_lines (budget_id, account_id, budgeted_amount) \
         SELECT $1, * FROM UNNEST($2::int[], $3::numeric[]) AS t(account_id, budgeted_amount) \
         ON CONFLICT (budget_id, account_id) DO UPDATE SET budgeted_amount = EXCLUDED.budgeted_amount",
    )
    .bind(budget_id)
    .bind(account_ids)
    .bind(budgeted_amounts)
    .execute(conn)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct BudgetVarianceRow {
    pub account_id: AccountId,
    pub account_code: String,
    pub account_name: String,
    pub account_type: String,
    pub budgeted_amount: Decimal,
    pub actual_amount: Decimal,
}

/// Budgeted vs. actual per account for a fiscal year, `actual_amount` signed by the account's normal balance.
pub async fn budget_variance(
    conn: &mut sqlx::PgConnection,
    fiscal_year_id: FiscalYearId,
) -> Result<Vec<BudgetVarianceRow>, sqlx::Error> {
    sqlx::query_as::<_, BudgetVarianceRow>(
        "SELECT a.account_id, a.code AS account_code, a.name AS account_name, \
                a.account_type::TEXT AS account_type, \
                COALESCE(bl.budgeted_amount, 0) AS budgeted_amount, \
                COALESCE(actuals.actual_amount, 0) AS actual_amount \
         FROM chart_of_accounts a \
         LEFT JOIN budgets b ON b.fiscal_year_id = $1 \
         LEFT JOIN budget_lines bl ON bl.budget_id = b.budget_id AND bl.account_id = a.account_id \
         LEFT JOIN ( \
             SELECT le.account_id, \
                    SUM(CASE WHEN a2.normal_balance = 'DEBIT' THEN le.debit - le.credit \
                             ELSE le.credit - le.debit END) AS actual_amount \
             FROM ledger_entries le \
             JOIN ledger_transactions lt ON lt.transaction_id = le.transaction_id \
             JOIN chart_of_accounts a2 ON a2.account_id = le.account_id \
             WHERE lt.fiscal_year_id = $1 \
             GROUP BY le.account_id \
         ) actuals ON actuals.account_id = a.account_id \
         WHERE a.account_type IN ('INCOME', 'EXPENSE') \
           AND (bl.budgeted_amount IS NOT NULL OR actuals.actual_amount IS NOT NULL) \
         ORDER BY a.code",
    )
    .bind(fiscal_year_id)
    .fetch_all(conn)
    .await
}

// ===========================================================================
// Member advance / credit balance (flat-scoped, ledger-derived)
// ===========================================================================

/// Advisory lock held for the rest of the transaction, since credit balance is a `SUM` with no row to lock.
pub async fn lock_flat_credit(
    conn: &mut sqlx::PgConnection,
    flat_id: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock($1, $2)")
        .bind(ADVISORY_LOCK_NAMESPACE_FLAT_CREDIT)
        .bind(flat_id)
        .execute(conn)
        .await?;
    Ok(())
}

pub async fn get_flat_credit_balance(
    conn: &mut sqlx::PgConnection,
    flat_id: i32,
) -> Result<Decimal, sqlx::Error> {
    let balance: Option<Decimal> = sqlx::query_scalar(
        "SELECT SUM(le.credit - le.debit) \
         FROM ledger_entries le \
         JOIN chart_of_accounts a ON a.account_id = le.account_id \
         WHERE le.flat_id = $1 AND a.code = $2",
    )
    .bind(flat_id)
    .bind(crate::accounts::MEMBER_ADVANCE)
    .fetch_one(conn)
    .await?;
    Ok(balance.unwrap_or(Decimal::ZERO))
}

// ===========================================================================
// Fiscal years — close/reopen
// ===========================================================================

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct FiscalYear {
    pub fiscal_year_id: FiscalYearId,
    pub label: String,
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub status: String,
    pub closing_transaction_id: Option<TransactionId>,
}

pub async fn list_fiscal_years(
    conn: &mut sqlx::PgConnection,
) -> Result<Vec<FiscalYear>, sqlx::Error> {
    sqlx::query_as::<_, FiscalYear>(
        "SELECT fiscal_year_id, label, start_date, end_date, status, closing_transaction_id \
         FROM fiscal_years ORDER BY start_date DESC",
    )
    .fetch_all(conn)
    .await
}

pub async fn get_fiscal_year(
    conn: &mut sqlx::PgConnection,
    fiscal_year_id: FiscalYearId,
) -> Result<Option<FiscalYear>, sqlx::Error> {
    sqlx::query_as::<_, FiscalYear>(
        "SELECT fiscal_year_id, label, start_date, end_date, status, closing_transaction_id \
         FROM fiscal_years WHERE fiscal_year_id = $1",
    )
    .bind(fiscal_year_id)
    .fetch_optional(conn)
    .await
}

/// Same as `get_fiscal_year`, but row-locks it — required before flipping `status`.
pub async fn get_fiscal_year_for_update(
    conn: &mut sqlx::PgConnection,
    fiscal_year_id: FiscalYearId,
) -> Result<Option<FiscalYear>, sqlx::Error> {
    sqlx::query_as::<_, FiscalYear>(
        "SELECT fiscal_year_id, label, start_date, end_date, status, closing_transaction_id \
         FROM fiscal_years WHERE fiscal_year_id = $1 FOR UPDATE",
    )
    .bind(fiscal_year_id)
    .fetch_optional(conn)
    .await
}

/// Net INCOME/EXPENSE account balances for the fiscal year, signed by normal balance.
pub async fn income_expense_balances_for_fy(
    conn: &mut sqlx::PgConnection,
    fiscal_year_id: FiscalYearId,
) -> Result<Vec<(AccountId, String, Decimal)>, sqlx::Error> {
    sqlx::query_as(
        "SELECT a.account_id, a.account_type::TEXT, \
                SUM(CASE WHEN a.account_type = 'EXPENSE' THEN le.debit - le.credit \
                         ELSE le.credit - le.debit END) AS balance \
         FROM ledger_entries le \
         JOIN ledger_transactions lt ON lt.transaction_id = le.transaction_id \
         JOIN chart_of_accounts a ON a.account_id = le.account_id \
         WHERE lt.fiscal_year_id = $1 AND a.account_type IN ('INCOME', 'EXPENSE') \
         GROUP BY a.account_id, a.account_type \
         HAVING SUM(CASE WHEN a.account_type = 'EXPENSE' THEN le.debit - le.credit \
                         ELSE le.credit - le.debit END) != 0",
    )
    .bind(fiscal_year_id)
    .fetch_all(conn)
    .await
}

pub async fn set_fiscal_year_closed(
    conn: &mut sqlx::PgConnection,
    fiscal_year_id: FiscalYearId,
    closed_by: i32,
    closing_transaction_id: TransactionId,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE fiscal_years SET status = 'CLOSED', closed_at = NOW(), closed_by = $1, \
            closing_transaction_id = $2 \
         WHERE fiscal_year_id = $3",
    )
    .bind(closed_by)
    .bind(closing_transaction_id)
    .bind(fiscal_year_id)
    .execute(conn)
    .await?;
    Ok(())
}

pub async fn set_fiscal_year_reopened(
    conn: &mut sqlx::PgConnection,
    fiscal_year_id: FiscalYearId,
    reopened_by: i32,
    reason: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE fiscal_years SET status = 'OPEN', closing_transaction_id = NULL WHERE fiscal_year_id = $1",
    )
    .bind(fiscal_year_id)
    .execute(&mut *conn)
    .await?;
    sqlx::query(
        "INSERT INTO fiscal_year_reopenings (fiscal_year_id, reopened_by, reason) VALUES ($1, $2, $3)",
    )
    .bind(fiscal_year_id)
    .bind(reopened_by)
    .bind(reason)
    .execute(conn)
    .await?;
    Ok(())
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct LedgerEntryRow {
    pub account_id: AccountId,
    pub flat_id: Option<i32>,
    pub debit: Decimal,
    pub credit: Decimal,
}

/// Reads back a posted transaction's entries, to build a reversing transaction.
pub async fn get_ledger_entries_for_transaction(
    conn: &mut sqlx::PgConnection,
    transaction_id: TransactionId,
) -> Result<Vec<LedgerEntryRow>, sqlx::Error> {
    sqlx::query_as::<_, LedgerEntryRow>(
        "SELECT account_id, flat_id, debit, credit FROM ledger_entries WHERE transaction_id = $1",
    )
    .bind(transaction_id)
    .fetch_all(conn)
    .await
}

// ===========================================================================
// Chart of accounts — dynamic income/expense heads. Asset/liability/equity/system
// accounts are structural (the ledger's own plumbing) and are never created here.
// ===========================================================================

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct Account {
    pub account_id: AccountId,
    pub code: String,
    pub name: String,
    pub account_type: String,
    pub normal_balance: String,
    pub is_system: bool,
    pub is_active: bool,
    pub created_by: Option<i32>,
}

/// `account_type` must be `"INCOME"` or `"EXPENSE"` — the caller already rejects anything else.
pub async fn insert_account(
    conn: &mut sqlx::PgConnection,
    code: &str,
    name: &str,
    account_type: &str,
    created_by: i32,
) -> Result<AccountId, sqlx::Error> {
    let normal_balance = if account_type == "INCOME" {
        "CREDIT"
    } else {
        "DEBIT"
    };
    sqlx::query_scalar(
        "INSERT INTO chart_of_accounts (code, name, account_type, normal_balance, is_system, created_by) \
         VALUES ($1, $2, $3::account_type, $4::normal_balance, FALSE, $5) RETURNING account_id",
    )
    .bind(code)
    .bind(name)
    .bind(account_type)
    .bind(normal_balance)
    .bind(created_by)
    .fetch_one(conn)
    .await
}

pub async fn list_accounts(
    conn: &mut sqlx::PgConnection,
    account_type: Option<&str>,
    active_only: bool,
) -> Result<Vec<Account>, sqlx::Error> {
    sqlx::query_as::<_, Account>(
        "SELECT account_id, code, name, account_type::TEXT, normal_balance::TEXT, is_system, is_active, created_by \
         FROM chart_of_accounts \
         WHERE ($1::TEXT IS NULL OR account_type::TEXT = $1) \
           AND (NOT $2 OR is_active = TRUE) \
         ORDER BY code",
    )
    .bind(account_type)
    .bind(active_only)
    .fetch_all(conn)
    .await
}

pub async fn get_account(
    conn: &mut sqlx::PgConnection,
    account_id: AccountId,
) -> Result<Option<Account>, sqlx::Error> {
    sqlx::query_as::<_, Account>(
        "SELECT account_id, code, name, account_type::TEXT, normal_balance::TEXT, is_system, is_active, created_by \
         FROM chart_of_accounts WHERE account_id = $1",
    )
    .bind(account_id)
    .fetch_optional(conn)
    .await
}

/// `false` if the account is a protected system account or didn't exist.
pub async fn set_account_active(
    conn: &mut sqlx::PgConnection,
    account_id: AccountId,
    is_active: bool,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query(
        "UPDATE chart_of_accounts SET is_active = $1 WHERE account_id = $2 AND is_system = FALSE",
    )
    .bind(is_active)
    .bind(account_id)
    .execute(conn)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// Resolves a flat's numeric id from wing + flat_number.
pub async fn resolve_flat_id(
    conn: &mut sqlx::PgConnection,
    wing: &str,
    flat_number: &str,
) -> Result<Option<i32>, sqlx::Error> {
    sqlx::query_scalar("SELECT flat_id FROM flats WHERE wing = $1 AND flat_number = $2")
        .bind(wing)
        .bind(flat_number)
        .fetch_optional(conn)
        .await
}
