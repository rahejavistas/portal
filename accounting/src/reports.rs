use crate::{AccountId, InvoiceId};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;

// ---------------------------------------------------------------------------
// 1. Invoice register
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct InvoiceRegisterRow {
    pub invoice_id: InvoiceId,
    pub invoice_number: String,
    pub flat_id: i32,
    pub wing: String,
    pub flat_number: String,
    pub billing_period_start: NaiveDate,
    pub billing_period_end: NaiveDate,
    pub due_date: NaiveDate,
    pub subtotal: Decimal,
    pub gst_amount: Decimal,
    pub total_amount: Decimal,
    pub amount_paid: Decimal,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

pub async fn invoice_register(
    conn: &mut sqlx::PgConnection,
    date_from: NaiveDate,
    date_to: NaiveDate,
    status: Option<&str>,
    flat_id: Option<i32>,
) -> Result<Vec<InvoiceRegisterRow>, sqlx::Error> {
    sqlx::query_as::<_, InvoiceRegisterRow>(
        "SELECT i.invoice_id, i.invoice_number, i.flat_id, f.wing, f.flat_number, \
                i.billing_period_start, i.billing_period_end, i.due_date, i.subtotal, \
                i.gst_amount, i.total_amount, i.amount_paid, i.status, i.created_at \
         FROM invoices i \
         JOIN flats f ON f.flat_id = i.flat_id \
         WHERE i.billing_period_start >= $1 AND i.billing_period_start <= $2 \
           AND ($3::VARCHAR IS NULL OR i.status = $3) \
           AND ($4::INT IS NULL OR i.flat_id = $4) \
         ORDER BY i.billing_period_start, f.wing, f.flat_number",
    )
    .bind(date_from)
    .bind(date_to)
    .bind(status)
    .bind(flat_id)
    .fetch_all(conn)
    .await
}

// ---------------------------------------------------------------------------
// 4. GST summary
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct GstSummaryRow {
    pub gst_rate_pct: Decimal,
    pub invoice_count: i64,
    pub taxable_value: Decimal,
    pub gst_collected: Decimal,
}

pub async fn gst_summary(
    conn: &mut sqlx::PgConnection,
    date_from: NaiveDate,
    date_to: NaiveDate,
) -> Result<Vec<GstSummaryRow>, sqlx::Error> {
    sqlx::query_as::<_, GstSummaryRow>(
        "SELECT li.gst_rate_pct, COUNT(DISTINCT li.invoice_id) AS invoice_count, \
                SUM(li.amount) AS taxable_value, SUM(li.gst_amount) AS gst_collected \
         FROM invoice_line_items li \
         JOIN invoices i ON i.invoice_id = li.invoice_id \
         WHERE i.billing_period_start >= $1 AND i.billing_period_start <= $2 \
           AND i.status != 'VOID' \
         GROUP BY li.gst_rate_pct \
         ORDER BY li.gst_rate_pct",
    )
    .bind(date_from)
    .bind(date_to)
    .fetch_all(conn)
    .await
}

// ---------------------------------------------------------------------------
// 5. Interest report
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct InterestReportRow {
    pub invoice_number: String,
    pub flat_id: i32,
    pub wing: String,
    pub flat_number: String,
    pub overdue_amount: Decimal,
    pub days_overdue: i32,
    pub interest_amount: Decimal,
    pub annual_rate_pct: Decimal,
    pub charged_date: NaiveDate,
}

pub async fn interest_report(
    conn: &mut sqlx::PgConnection,
    date_from: NaiveDate,
    date_to: NaiveDate,
) -> Result<Vec<InterestReportRow>, sqlx::Error> {
    sqlx::query_as::<_, InterestReportRow>(
        "SELECT i.invoice_number, i.flat_id, f.wing, f.flat_number, ic.overdue_amount, \
                ic.days_overdue, ic.interest_amount, r.annual_rate_pct, ic.charged_date \
         FROM interest_charges ic \
         JOIN invoices i ON i.invoice_id = ic.invoice_id \
         JOIN flats f ON f.flat_id = i.flat_id \
         JOIN interest_rules r ON r.interest_rule_id = ic.interest_rule_id \
         WHERE ic.charged_date >= $1 AND ic.charged_date <= $2 \
         ORDER BY ic.charged_date DESC",
    )
    .bind(date_from)
    .bind(date_to)
    .fetch_all(conn)
    .await
}

// ---------------------------------------------------------------------------
// 6/7. Outstanding dues & ageing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct OutstandingDueRow {
    pub invoice_id: InvoiceId,
    pub invoice_number: String,
    pub flat_id: i32,
    pub wing: String,
    pub flat_number: String,
    pub due_date: NaiveDate,
    pub outstanding_amount: Decimal,
    pub days_overdue: i32,
    pub ageing_bucket: String,
}

pub async fn outstanding_dues(
    conn: &mut sqlx::PgConnection,
    as_of: NaiveDate,
) -> Result<Vec<OutstandingDueRow>, sqlx::Error> {
    sqlx::query_as::<_, OutstandingDueRow>(
        "SELECT i.invoice_id, i.invoice_number, i.flat_id, f.wing, f.flat_number, i.due_date, \
                (i.total_amount - i.amount_paid) AS outstanding_amount, \
                GREATEST(($1::DATE - i.due_date), 0) AS days_overdue, \
                CASE \
                    WHEN $1::DATE <= i.due_date THEN 'not_due' \
                    WHEN $1::DATE - i.due_date <= 30 THEN '0-30' \
                    WHEN $1::DATE - i.due_date <= 60 THEN '31-60' \
                    WHEN $1::DATE - i.due_date <= 90 THEN '61-90' \
                    ELSE '90+' \
                END AS ageing_bucket \
         FROM invoices i \
         JOIN flats f ON f.flat_id = i.flat_id \
         WHERE i.status IN ('UNPAID', 'PARTIALLY_PAID') \
         ORDER BY days_overdue DESC",
    )
    .bind(as_of)
    .fetch_all(conn)
    .await
}

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct AgeingBucketRow {
    pub ageing_bucket: String,
    pub unit_count: i64,
    pub total_outstanding: Decimal,
}

pub async fn ageing_summary(
    conn: &mut sqlx::PgConnection,
    as_of: NaiveDate,
) -> Result<Vec<AgeingBucketRow>, sqlx::Error> {
    sqlx::query_as::<_, AgeingBucketRow>(
        "SELECT ageing_bucket, COUNT(*) AS unit_count, SUM(outstanding_amount) AS total_outstanding \
         FROM ( \
             SELECT i.flat_id, (i.total_amount - i.amount_paid) AS outstanding_amount, \
                    CASE \
                        WHEN $1::DATE <= i.due_date THEN 'not_due' \
                        WHEN $1::DATE - i.due_date <= 30 THEN '0-30' \
                        WHEN $1::DATE - i.due_date <= 60 THEN '31-60' \
                        WHEN $1::DATE - i.due_date <= 90 THEN '61-90' \
                        ELSE '90+' \
                    END AS ageing_bucket \
             FROM invoices i \
             WHERE i.status IN ('UNPAID', 'PARTIALLY_PAID') \
         ) buckets \
         GROUP BY ageing_bucket",
    )
    .bind(as_of)
    .fetch_all(conn)
    .await
}

// ---------------------------------------------------------------------------
// 8. Collection summary
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct CollectionSummary {
    pub total_billed: Decimal,
    pub total_collected: Decimal,
    pub collection_efficiency_pct: Decimal,
}

pub async fn collection_summary(
    conn: &mut sqlx::PgConnection,
    date_from: NaiveDate,
    date_to: NaiveDate,
) -> Result<CollectionSummary, sqlx::Error> {
    let total_billed: Option<Decimal> = sqlx::query_scalar(
        "SELECT SUM(total_amount) FROM invoices \
         WHERE billing_period_start >= $1 AND billing_period_start <= $2 AND status != 'VOID'",
    )
    .bind(date_from)
    .bind(date_to)
    .fetch_one(&mut *conn)
    .await?;

    let total_collected: Option<Decimal> = sqlx::query_scalar(
        "SELECT SUM(amount) FROM payments WHERE confirmed_at::DATE >= $1 AND confirmed_at::DATE <= $2",
    )
    .bind(date_from)
    .bind(date_to)
    .fetch_one(&mut *conn)
    .await?;

    let total_billed = total_billed.unwrap_or(Decimal::ZERO);
    let total_collected = total_collected.unwrap_or(Decimal::ZERO);
    let efficiency = if total_billed > Decimal::ZERO {
        (total_collected / total_billed * Decimal::from(100)).round_dp(2)
    } else {
        Decimal::ZERO
    };

    Ok(CollectionSummary {
        total_billed,
        total_collected,
        collection_efficiency_pct: efficiency,
    })
}

// ---------------------------------------------------------------------------
// 10. Defaulters (legal/notice action)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct DefaulterRow {
    pub flat_id: i32,
    pub wing: String,
    pub flat_number: String,
    pub owner_email: Option<String>,
    pub total_outstanding: Decimal,
    pub max_days_overdue: i32,
    pub unpaid_invoice_count: i64,
}

pub async fn defaulters(
    conn: &mut sqlx::PgConnection,
    as_of: NaiveDate,
    min_days_overdue: i32,
) -> Result<Vec<DefaulterRow>, sqlx::Error> {
    // Aggregate per flat before joining the owner email, so the owner join can't duplicate rows.
    sqlx::query_as::<_, DefaulterRow>(
        "WITH per_flat AS ( \
             SELECT i.flat_id, \
                    SUM(i.total_amount - i.amount_paid) AS total_outstanding, \
                    MAX(GREATEST(($1::DATE - i.due_date), 0)) AS max_days_overdue, \
                    COUNT(*) AS unpaid_invoice_count \
             FROM invoices i \
             WHERE i.status IN ('UNPAID', 'PARTIALLY_PAID') \
             GROUP BY i.flat_id \
         ) \
         SELECT f.flat_id, f.wing, f.flat_number, \
                (SELECT string_agg(u.email, ', ' ORDER BY u.email) FROM flat_ownerships fo \
                   JOIN flat_ownership_users fou ON fou.ownership_id = fo.ownership_id AND fou.is_active = TRUE \
                   JOIN users u ON u.user_id = fou.user_id \
                  WHERE fo.flat_id = f.flat_id AND fo.is_active = TRUE) AS owner_email, \
                p.total_outstanding, p.max_days_overdue, p.unpaid_invoice_count \
         FROM per_flat p \
         JOIN flats f ON f.flat_id = p.flat_id \
         WHERE p.max_days_overdue >= $2 \
         ORDER BY p.max_days_overdue DESC",
    )
    .bind(as_of)
    .bind(min_days_overdue)
    .fetch_all(conn)
    .await
}

// ---------------------------------------------------------------------------
// 11. Receipt register
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct ReceiptRegisterRow {
    pub receipt_number: String,
    pub invoice_number: String,
    pub flat_id: i32,
    pub wing: String,
    pub flat_number: String,
    pub amount: Decimal,
    pub payment_method: String,
    pub reference_number: Option<String>,
    pub generated_at: DateTime<Utc>,
}

pub async fn receipt_register(
    conn: &mut sqlx::PgConnection,
    date_from: NaiveDate,
    date_to: NaiveDate,
) -> Result<Vec<ReceiptRegisterRow>, sqlx::Error> {
    sqlx::query_as::<_, ReceiptRegisterRow>(
        "SELECT r.receipt_number, i.invoice_number, i.flat_id, f.wing, f.flat_number, \
                p.amount, p.payment_method, p.reference_number, r.generated_at \
         FROM receipts r \
         JOIN payments p ON p.payment_id = r.payment_id \
         JOIN invoices i ON i.invoice_id = p.invoice_id \
         JOIN flats f ON f.flat_id = i.flat_id \
         WHERE r.generated_at::DATE >= $1 AND r.generated_at::DATE <= $2 \
         ORDER BY r.receipt_number",
    )
    .bind(date_from)
    .bind(date_to)
    .fetch_all(conn)
    .await
}

// ---------------------------------------------------------------------------
// 12. Payment mode summary
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct PaymentModeSummaryRow {
    pub payment_method: String,
    pub payment_count: i64,
    pub total_amount: Decimal,
}

pub async fn payment_mode_summary(
    conn: &mut sqlx::PgConnection,
    date_from: NaiveDate,
    date_to: NaiveDate,
) -> Result<Vec<PaymentModeSummaryRow>, sqlx::Error> {
    sqlx::query_as::<_, PaymentModeSummaryRow>(
        "SELECT payment_method, COUNT(*) AS payment_count, SUM(amount) AS total_amount \
         FROM payments \
         WHERE confirmed_at::DATE >= $1 AND confirmed_at::DATE <= $2 \
         GROUP BY payment_method \
         ORDER BY total_amount DESC",
    )
    .bind(date_from)
    .bind(date_to)
    .fetch_all(conn)
    .await
}

// ---------------------------------------------------------------------------
// 17. Income summary (income-side only — expense side needs the P1 vendor module)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct IncomeSummaryRow {
    pub account_id: AccountId,
    pub account_code: String,
    pub account_name: String,
    pub total_income: Decimal,
}

pub async fn income_summary(
    conn: &mut sqlx::PgConnection,
    date_from: NaiveDate,
    date_to: NaiveDate,
) -> Result<Vec<IncomeSummaryRow>, sqlx::Error> {
    sqlx::query_as::<_, IncomeSummaryRow>(
        "SELECT a.account_id, a.code AS account_code, a.name AS account_name, \
                SUM(le.credit - le.debit) AS total_income \
         FROM ledger_entries le \
         JOIN ledger_transactions lt ON lt.transaction_id = le.transaction_id \
         JOIN chart_of_accounts a ON a.account_id = le.account_id \
         WHERE a.account_type = 'INCOME' \
           AND lt.transaction_date >= $1 AND lt.transaction_date <= $2 \
         GROUP BY a.account_id, a.code, a.name \
         ORDER BY total_income DESC",
    )
    .bind(date_from)
    .bind(date_to)
    .fetch_all(conn)
    .await
}

// ---------------------------------------------------------------------------
// 3. Bill generation preview — read-only, nothing written
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct BillPreviewRow {
    pub flat_id: i32,
    pub wing: String,
    pub flat_number: String,
    pub billing_plan_name: String,
    pub base_amount: Decimal,
    pub gst_applicable: bool,
    pub gst_amount: Decimal,
    pub total_amount: Decimal,
    pub error: Option<String>,
}

pub async fn preview_bill_generation(
    conn: &mut sqlx::PgConnection,
    period_start: NaiveDate,
) -> Result<Vec<BillPreviewRow>, sqlx::Error> {
    let assigned_flats: Vec<(i32, String, String)> = sqlx::query_as(
        "SELECT f.flat_id, f.wing, f.flat_number FROM flat_billing_assignments fba \
         JOIN flats f ON f.flat_id = fba.flat_id",
    )
    .fetch_all(&mut *conn)
    .await?;

    let mut rows = Vec::with_capacity(assigned_flats.len());
    for (flat_id, wing, flat_number) in assigned_flats {
        match crate::billing::compute_bill_for_flat(conn, flat_id, period_start).await {
            Ok(bill) => rows.push(BillPreviewRow {
                flat_id,
                wing,
                flat_number,
                billing_plan_name: bill.plan.name,
                base_amount: bill.base_amount,
                gst_applicable: bill.gst_applicable,
                gst_amount: bill.gst_amount,
                total_amount: bill.total,
                error: None,
            }),
            Err(e) => rows.push(BillPreviewRow {
                flat_id,
                wing,
                flat_number,
                billing_plan_name: String::new(),
                base_amount: Decimal::ZERO,
                gst_applicable: false,
                gst_amount: Decimal::ZERO,
                total_amount: Decimal::ZERO,
                error: Some(e.to_string()),
            }),
        }
    }
    Ok(rows)
}

// ---------------------------------------------------------------------------
// 14. Member statement of account — ledger-backed, already accurate in P0 (see the
// note in the reports design doc correcting the "needs P1 ledger" assumption).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize, sqlx::FromRow)]
pub struct MemberStatementRow {
    pub transaction_date: NaiveDate,
    pub description: String,
    pub debit: Decimal,
    pub credit: Decimal,
    pub running_balance: Decimal,
}

pub async fn member_statement(
    conn: &mut sqlx::PgConnection,
    flat_id: i32,
    date_from: NaiveDate,
    date_to: NaiveDate,
) -> Result<Vec<MemberStatementRow>, sqlx::Error> {
    sqlx::query_as::<_, MemberStatementRow>(
        "WITH opening AS ( \
             SELECT COALESCE(SUM(le.debit - le.credit), 0) AS balance \
             FROM ledger_entries le \
             JOIN ledger_transactions lt ON lt.transaction_id = le.transaction_id \
             JOIN chart_of_accounts a ON a.account_id = le.account_id \
             WHERE le.flat_id = $1 AND a.code = $4 AND lt.transaction_date < $2 \
         ) \
         SELECT lt.transaction_date, lt.description, le.debit, le.credit, \
                opening.balance + SUM(le.debit - le.credit) \
                    OVER (ORDER BY lt.transaction_date, lt.created_at, le.entry_id) AS running_balance \
         FROM ledger_entries le \
         JOIN ledger_transactions lt ON lt.transaction_id = le.transaction_id \
         JOIN chart_of_accounts a ON a.account_id = le.account_id \
         CROSS JOIN opening \
         WHERE le.flat_id = $1 AND a.code = $4 \
           AND lt.transaction_date >= $2 AND lt.transaction_date <= $3 \
         ORDER BY lt.transaction_date, lt.created_at, le.entry_id",
    )
    .bind(flat_id)
    .bind(date_from)
    .bind(date_to)
    .bind(crate::accounts::MEMBER_DUES_RECEIVABLE)
    .fetch_all(conn)
    .await
}
