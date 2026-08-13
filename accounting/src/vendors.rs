use crate::db;
use crate::ledger::{self, EntryLine, PostTransactionRequest};
use crate::{accounts, tds, AccountId, ExpenseId, PoId, TransactionId, VendorId};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use uuid::Uuid;

#[derive(Debug)]
pub enum ExpenseError {
    NotFound,
    NotPending,
    PoNotFound,
    PoVendorMismatch,
    Post(ledger::PostError),
    Db(sqlx::Error),
}

impl std::fmt::Display for ExpenseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExpenseError::NotFound => write!(f, "Vendor expense not found"),
            ExpenseError::NotPending => write!(f, "This expense has already been reviewed"),
            ExpenseError::PoNotFound => write!(f, "Purchase order not found"),
            ExpenseError::PoVendorMismatch => {
                write!(f, "This purchase order was issued to a different vendor")
            }
            ExpenseError::Post(e) => write!(f, "{}", e),
            ExpenseError::Db(e) => write!(f, "{}", e),
        }
    }
}

impl From<sqlx::Error> for ExpenseError {
    fn from(e: sqlx::Error) -> Self {
        ExpenseError::Db(e)
    }
}

impl From<ledger::PostError> for ExpenseError {
    fn from(e: ledger::PostError) -> Self {
        ExpenseError::Post(e)
    }
}

pub struct SubmitExpenseRequest<'a> {
    pub po_id: Option<PoId>,
    pub vendor_id: VendorId,
    pub account_id: AccountId,
    pub payment_category: &'a str,
    pub vendor_invoice_number: Option<&'a str>,
    pub invoice_document_id: Option<Uuid>,
    pub amount: Decimal,
    pub created_by: i32,
}

/// Submits a vendor bill for approval, computing TDS up front. Posts nothing yet.
pub async fn submit_expense(
    conn: &mut sqlx::PgConnection,
    req: SubmitExpenseRequest<'_>,
    fiscal_year_id: crate::FiscalYearId,
) -> Result<ExpenseId, ExpenseError> {
    if let Some(po_id) = req.po_id {
        let po = db::get_purchase_order(conn, po_id)
            .await?
            .ok_or(ExpenseError::PoNotFound)?;
        if po.vendor_id != req.vendor_id {
            return Err(ExpenseError::PoVendorMismatch);
        }
    }

    let rules = db::list_active_tds_rules(conn).await?;
    let ytd = db::ytd_vendor_amount(conn, req.vendor_id, fiscal_year_id).await?;
    let tds_calc = tds::resolve_tds(&rules, req.payment_category, req.amount, ytd);
    let tds_amount = tds_calc
        .as_ref()
        .map(|c| c.tds_amount)
        .unwrap_or(Decimal::ZERO);
    let tds_rule_id = tds_calc.as_ref().map(|c| c.rule.tds_rule_id);
    let net_payable = req.amount - tds_amount;

    let expense_id = db::insert_vendor_expense(
        conn,
        db::NewVendorExpense {
            po_id: req.po_id,
            vendor_id: req.vendor_id,
            account_id: req.account_id,
            vendor_invoice_number: req.vendor_invoice_number,
            invoice_document_id: req.invoice_document_id,
            amount: req.amount,
            tds_amount,
            net_payable,
            created_by: req.created_by,
        },
    )
    .await?;

    if let Some(tds_rule_id) = tds_rule_id {
        db::insert_tds_record(
            conn,
            db::NewTdsRecord {
                expense_id,
                tds_rule_id,
                vendor_id: req.vendor_id,
                fiscal_year_id,
                gross_amount: req.amount,
                tds_amount,
                deducted_on: chrono::Utc::now().date_naive(),
            },
        )
        .await?;
    }

    Ok(expense_id)
}

/// Approves a pending expense: Dr. Expense / Cr. Vendor Payables / Cr. TDS Payable (if any).
pub async fn approve_expense(
    conn: &mut sqlx::PgConnection,
    expense_id: ExpenseId,
    approved_by: i32,
    approval_date: NaiveDate,
) -> Result<TransactionId, ExpenseError> {
    let expense = db::get_vendor_expense_for_update(conn, expense_id)
        .await?
        .ok_or(ExpenseError::NotFound)?;
    if expense.status != "PENDING_APPROVAL" {
        return Err(ExpenseError::NotPending);
    }

    let vendor_payables = db::get_account_id_by_code(conn, accounts::VENDOR_PAYABLES).await?;

    let mut entries = vec![EntryLine {
        account_id: expense.account_id,
        flat_id: None,
        debit: expense.amount,
        credit: Decimal::ZERO,
    }];
    if expense.tds_amount > Decimal::ZERO {
        let tds_payable = db::get_account_id_by_code(conn, accounts::TDS_PAYABLE).await?;
        entries.push(EntryLine {
            account_id: tds_payable,
            flat_id: None,
            debit: Decimal::ZERO,
            credit: expense.tds_amount,
        });
    }
    entries.push(EntryLine {
        account_id: vendor_payables,
        flat_id: None,
        debit: Decimal::ZERO,
        credit: expense.net_payable,
    });

    let transaction_id = ledger::post_transaction(
        conn,
        PostTransactionRequest {
            transaction_date: approval_date,
            description: &format!(
                "Vendor expense {} ({})",
                expense_id,
                expense.vendor_invoice_number.as_deref().unwrap_or("no ref")
            ),
            source_type: "VENDOR_EXPENSE",
            source_id: Some(expense_id.0),
            reverses_transaction_id: None,
            entries,
            created_by: approved_by,
        },
    )
    .await?;

    db::mark_expense_approved(conn, expense_id, approved_by, transaction_id).await?;
    Ok(transaction_id)
}

pub async fn reject_expense(
    conn: &mut sqlx::PgConnection,
    expense_id: ExpenseId,
    reviewed_by: i32,
    reason: &str,
) -> Result<(), ExpenseError> {
    let expense = db::get_vendor_expense_for_update(conn, expense_id)
        .await?
        .ok_or(ExpenseError::NotFound)?;
    if expense.status != "PENDING_APPROVAL" {
        return Err(ExpenseError::NotPending);
    }
    // A rejected expense never incurred the TDS liability `submit_expense` speculatively recorded.
    db::delete_tds_record_for_expense(conn, expense_id).await?;
    db::mark_expense_rejected(conn, expense_id, reviewed_by, reason).await?;
    Ok(())
}
