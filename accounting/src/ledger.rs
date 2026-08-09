use crate::{db, AccountId, TransactionId};
use chrono::NaiveDate;
use rust_decimal::Decimal;
use uuid::Uuid;

pub struct EntryLine {
    pub account_id: AccountId,
    pub flat_id: Option<i32>,
    pub debit: Decimal,
    pub credit: Decimal,
}

pub struct PostTransactionRequest<'a> {
    pub transaction_date: NaiveDate,
    pub description: &'a str,
    pub source_type: &'a str,
    /// Polymorphic FK (invoice_id, payment_id, ...) whose meaning depends on `source_type`.
    pub source_id: Option<Uuid>,
    pub reverses_transaction_id: Option<TransactionId>,
    pub entries: Vec<EntryLine>,
    pub created_by: i32,
}

#[derive(Debug)]
pub enum PostError {
    TooFewEntries,
    Unbalanced {
        total_debit: Decimal,
        total_credit: Decimal,
    },
    NoFiscalYearForDate(NaiveDate),
    FiscalYearClosed,
    Db(sqlx::Error),
}

impl std::fmt::Display for PostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PostError::TooFewEntries => {
                write!(f, "A transaction needs at least two entry lines")
            }
            PostError::Unbalanced {
                total_debit,
                total_credit,
            } => write!(
                f,
                "Unbalanced transaction: total debit {} != total credit {}",
                total_debit, total_credit
            ),
            PostError::NoFiscalYearForDate(d) => {
                write!(f, "No fiscal year covers {}", d)
            }
            PostError::FiscalYearClosed => {
                write!(f, "This fiscal year is closed to new postings")
            }
            PostError::Db(e) => write!(f, "{}", e),
        }
    }
}

impl From<sqlx::Error> for PostError {
    fn from(e: sqlx::Error) -> Self {
        PostError::Db(e)
    }
}

/// The only function that writes to ledger_transactions/ledger_entries. Rejects
/// unbalanced entries and posting into a closed fiscal year. Caller owns the transaction.
pub async fn post_transaction(
    conn: &mut sqlx::PgConnection,
    req: PostTransactionRequest<'_>,
) -> Result<TransactionId, PostError> {
    if req.entries.len() < 2 {
        return Err(PostError::TooFewEntries);
    }
    let total_debit: Decimal = req.entries.iter().map(|e| e.debit).sum();
    let total_credit: Decimal = req.entries.iter().map(|e| e.credit).sum();
    if total_debit != total_credit {
        return Err(PostError::Unbalanced {
            total_debit,
            total_credit,
        });
    }

    let fiscal_year_id = db::resolve_fiscal_year_id(conn, req.transaction_date)
        .await?
        .ok_or(PostError::NoFiscalYearForDate(req.transaction_date))?;
    if db::is_fiscal_year_closed(conn, fiscal_year_id).await? {
        return Err(PostError::FiscalYearClosed);
    }

    let transaction_id: TransactionId = sqlx::query_scalar(
        "INSERT INTO ledger_transactions \
            (fiscal_year_id, transaction_date, description, source_type, source_id, \
             reverses_transaction_id, created_by) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         RETURNING transaction_id",
    )
    .bind(fiscal_year_id)
    .bind(req.transaction_date)
    .bind(req.description)
    .bind(req.source_type)
    .bind(req.source_id)
    .bind(req.reverses_transaction_id)
    .bind(req.created_by)
    .fetch_one(&mut *conn)
    .await?;

    let account_ids: Vec<AccountId> = req.entries.iter().map(|e| e.account_id).collect();
    let flat_ids: Vec<Option<i32>> = req.entries.iter().map(|e| e.flat_id).collect();
    let debits: Vec<Decimal> = req.entries.iter().map(|e| e.debit).collect();
    let credits: Vec<Decimal> = req.entries.iter().map(|e| e.credit).collect();

    sqlx::query(
        "INSERT INTO ledger_entries (transaction_id, account_id, flat_id, debit, credit) \
         SELECT $1, * FROM UNNEST($2::int[], $3::int[], $4::numeric[], $5::numeric[]) \
             AS t(account_id, flat_id, debit, credit)",
    )
    .bind(transaction_id)
    .bind(&account_ids)
    .bind(&flat_ids)
    .bind(&debits)
    .bind(&credits)
    .execute(&mut *conn)
    .await?;

    Ok(transaction_id)
}
