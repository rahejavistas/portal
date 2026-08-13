use crate::db;
use crate::ledger::{self, EntryLine, PostTransactionRequest};
use crate::{accounts, FiscalYearId, TransactionId};
use rust_decimal::Decimal;

#[derive(Debug)]
pub enum FiscalYearError {
    NotFound,
    AlreadyClosed,
    NotClosed,
    Post(ledger::PostError),
    Db(sqlx::Error),
}

impl std::fmt::Display for FiscalYearError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FiscalYearError::NotFound => write!(f, "Fiscal year not found"),
            FiscalYearError::AlreadyClosed => write!(f, "This fiscal year is already closed"),
            FiscalYearError::NotClosed => write!(f, "This fiscal year is not closed"),
            FiscalYearError::Post(e) => write!(f, "{}", e),
            FiscalYearError::Db(e) => write!(f, "{}", e),
        }
    }
}

impl From<sqlx::Error> for FiscalYearError {
    fn from(e: sqlx::Error) -> Self {
        FiscalYearError::Db(e)
    }
}

impl From<ledger::PostError> for FiscalYearError {
    fn from(e: ledger::PostError) -> Self {
        FiscalYearError::Post(e)
    }
}

/// Closes a fiscal year: zeroes every INCOME/EXPENSE balance into Retained Earnings,
/// then locks the period against new postings.
pub async fn close_fiscal_year(
    conn: &mut sqlx::PgConnection,
    fiscal_year_id: FiscalYearId,
    closed_by: i32,
) -> Result<Option<TransactionId>, FiscalYearError> {
    let fy = db::get_fiscal_year_for_update(conn, fiscal_year_id)
        .await?
        .ok_or(FiscalYearError::NotFound)?;
    if fy.status == "CLOSED" {
        return Err(FiscalYearError::AlreadyClosed);
    }

    let balances = db::income_expense_balances_for_fy(conn, fiscal_year_id).await?;
    if balances.is_empty() {
        return close_with_no_activity(conn, fiscal_year_id, closed_by).await;
    }

    let mut entries = Vec::with_capacity(balances.len() + 1);
    let mut total_income = Decimal::ZERO;
    let mut total_expense = Decimal::ZERO;
    for (account_id, account_type, balance) in &balances {
        let is_income = *account_type == "INCOME";
        if is_income {
            total_income += balance;
        } else {
            total_expense += balance;
        }
        // Zero the account by posting the opposite of its balance; flip sides if the
        // balance came back negative, since debit/credit must stay non-negative.
        let (debit, credit) = match (is_income, balance.is_sign_negative()) {
            (true, false) => (*balance, Decimal::ZERO),
            (true, true) => (Decimal::ZERO, -balance),
            (false, false) => (Decimal::ZERO, *balance),
            (false, true) => (-balance, Decimal::ZERO),
        };
        entries.push(EntryLine {
            account_id: *account_id,
            flat_id: None,
            debit,
            credit,
        });
    }

    let retained_earnings = db::get_account_id_by_code(conn, accounts::RETAINED_EARNINGS).await?;
    let net = total_income - total_expense;
    // A zero net must not post a (0, 0) line — one side has to be positive.
    if net > Decimal::ZERO {
        entries.push(EntryLine {
            account_id: retained_earnings,
            flat_id: None,
            debit: Decimal::ZERO,
            credit: net,
        });
    } else if net < Decimal::ZERO {
        entries.push(EntryLine {
            account_id: retained_earnings,
            flat_id: None,
            debit: -net,
            credit: Decimal::ZERO,
        });
    }

    let transaction_id = ledger::post_transaction(
        conn,
        PostTransactionRequest {
            transaction_date: fy.end_date,
            description: &format!("Year-end close: {}", fy.label),
            source_type: "FY_CLOSE",
            source_id: None,
            reverses_transaction_id: None,
            entries,
            created_by: closed_by,
        },
    )
    .await?;

    db::set_fiscal_year_closed(conn, fiscal_year_id, closed_by, transaction_id).await?;
    Ok(Some(transaction_id))
}

/// No income/expense activity to close — just locks the period, no transaction posted.
async fn close_with_no_activity(
    conn: &mut sqlx::PgConnection,
    fiscal_year_id: FiscalYearId,
    closed_by: i32,
) -> Result<Option<TransactionId>, FiscalYearError> {
    sqlx::query("UPDATE fiscal_years SET status = 'CLOSED', closed_at = NOW(), closed_by = $1 WHERE fiscal_year_id = $2")
        .bind(closed_by)
        .bind(fiscal_year_id)
        .execute(conn)
        .await
        .map_err(FiscalYearError::Db)?;
    Ok(None)
}

/// Reopens a closed fiscal year: reverses the closing entry, unlocks the period, logs why.
pub async fn reopen_fiscal_year(
    conn: &mut sqlx::PgConnection,
    fiscal_year_id: FiscalYearId,
    reopened_by: i32,
    reason: &str,
) -> Result<(), FiscalYearError> {
    let fy = db::get_fiscal_year_for_update(conn, fiscal_year_id)
        .await?
        .ok_or(FiscalYearError::NotFound)?;
    if fy.status != "CLOSED" {
        return Err(FiscalYearError::NotClosed);
    }

    db::set_fiscal_year_reopened(conn, fiscal_year_id, reopened_by, reason).await?;

    if let Some(closing_transaction_id) = fy.closing_transaction_id {
        let original_entries =
            db::get_ledger_entries_for_transaction(conn, closing_transaction_id).await?;
        let reversed: Vec<EntryLine> = original_entries
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
                transaction_date: fy.end_date,
                description: &format!("Reopening {}: reversing year-end close", fy.label),
                source_type: "FY_REOPEN",
                source_id: None,
                reverses_transaction_id: Some(closing_transaction_id),
                entries: reversed,
                created_by: reopened_by,
            },
        )
        .await?;
    }

    Ok(())
}
