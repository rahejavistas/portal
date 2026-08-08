use crate::db;
use crate::{BankImportId, BankLineId, TransactionId};
use chrono::NaiveDate;
use rust_decimal::Decimal;

/// One parsed row from an uploaded bank statement CSV. Parsing itself stays in the
/// handler (this crate has no CSV dependency) — this is what the handler hands over
/// after parsing.
pub struct StatementLine {
    pub transaction_date: NaiveDate,
    pub description: String,
    pub amount: Decimal,
}

pub async fn import_statement(
    conn: &mut sqlx::PgConnection,
    imported_by: i32,
    source_filename: Option<&str>,
    lines: Vec<StatementLine>,
) -> Result<(BankImportId, usize), sqlx::Error> {
    let import_id = db::insert_bank_import(conn, imported_by, source_filename).await?;
    let dates: Vec<NaiveDate> = lines.iter().map(|l| l.transaction_date).collect();
    let descriptions: Vec<String> = lines.iter().map(|l| l.description.clone()).collect();
    let amounts: Vec<Decimal> = lines.iter().map(|l| l.amount).collect();
    db::insert_bank_statement_lines(conn, import_id, &dates, &descriptions, &amounts).await?;
    Ok((import_id, lines.len()))
}

pub struct AutoMatchResult {
    pub matched: usize,
    pub unmatched: usize,
}

/// Auto-matches every UNMATCHED line in `import_id` against ledger transactions on the
/// bank account: same date, same signed amount, not already claimed by another line.
/// Ambiguous matches (more than one candidate) are left UNMATCHED for manual review
/// rather than guessed at.
pub async fn auto_match_import(
    conn: &mut sqlx::PgConnection,
    import_id: BankImportId,
) -> Result<AutoMatchResult, sqlx::Error> {
    let lines = db::list_bank_statement_lines(conn, Some(import_id), Some("UNMATCHED")).await?;
    let mut matched = 0;
    let mut unmatched = 0;
    for line in lines {
        let candidates = db::find_unmatched_ledger_transactions_for_amount(
            conn,
            line.transaction_date,
            line.amount,
        )
        .await?;
        match candidates.as_slice() {
            [only] => {
                db::mark_line_matched(conn, line.line_id, *only).await?;
                matched += 1;
            }
            _ => unmatched += 1, // zero or ambiguous — leave for manual review either way
        }
    }
    Ok(AutoMatchResult { matched, unmatched })
}

/// Returns `false` if the line wasn't in UNMATCHED state (already matched/ignored) —
/// the caller should surface that as a client error, not silently succeed.
pub async fn match_line_manually(
    conn: &mut sqlx::PgConnection,
    line_id: BankLineId,
    transaction_id: TransactionId,
) -> Result<bool, sqlx::Error> {
    db::mark_line_matched(conn, line_id, transaction_id).await
}

/// Same "false means it wasn't UNMATCHED" contract as `match_line_manually`.
pub async fn ignore_line(
    conn: &mut sqlx::PgConnection,
    line_id: BankLineId,
) -> Result<bool, sqlx::Error> {
    db::mark_line_ignored(conn, line_id).await
}
