use crate::extractors::SessionUser;
use crate::handlers::error::{bad_request, internal_err, ApiError};
use crate::state::AppState;
use accounting::db::{BankMatchCandidate, BankStatementLine};
use accounting::{BankImportId, BankLineId, TransactionId};
use axum::extract::{Multipart, State};
use axum::{http::StatusCode, Json};

use super::not_found;

/// Prefers an exact header match; falls back to substring only if exactly one qualifies.
fn find_column(
    headers: &csv::StringRecord,
    exact_names: &[&str],
    substr: &str,
    column_label: &str,
) -> Result<usize, String> {
    let normalized: Vec<String> = headers.iter().map(|h| h.trim().to_lowercase()).collect();
    for exact in exact_names {
        if let Some(idx) = normalized.iter().position(|h| h == exact) {
            return Ok(idx);
        }
    }
    let matches: Vec<usize> = normalized
        .iter()
        .enumerate()
        .filter(|(_, h)| h.contains(substr))
        .map(|(i, _)| i)
        .collect();
    match matches.as_slice() {
        [idx] => Ok(*idx),
        [] => Err(format!("CSV must contain a {column_label} column")),
        _ => Err(format!(
            "CSV has multiple columns that could be {column_label} ({}) — rename the \
             intended one to an exact match like \"{}\"",
            matches
                .iter()
                .map(|i| headers[*i].to_string())
                .collect::<Vec<_>>()
                .join(", "),
            exact_names.first().unwrap_or(&column_label)
        )),
    }
}

fn parse_statement_csv(
    bytes: &[u8],
) -> Result<Vec<accounting::bank_reconciliation::StatementLine>, String> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(bytes);
    let headers = reader
        .headers()
        .map_err(|_| "Failed to read CSV headers")?
        .clone();

    let d = find_column(&headers, &["date", "transaction date"], "date", "Date")?;
    let desc = find_column(
        &headers,
        &["description", "narration"],
        "desc",
        "Description",
    )?;
    let a = find_column(&headers, &["amount"], "amount", "Amount")?;

    let mut lines = Vec::new();
    for result in reader.records() {
        let row = result.map_err(|e| e.to_string())?;
        let transaction_date = row
            .get(d)
            .and_then(|s| chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok())
            .ok_or_else(|| format!("Invalid date in row: {:?}", row))?;
        let amount: rust_decimal::Decimal = row
            .get(a)
            .and_then(|s| s.trim().parse().ok())
            .ok_or_else(|| format!("Invalid amount in row: {:?}", row))?;
        let description = row.get(desc).unwrap_or("").to_string();
        lines.push(accounting::bank_reconciliation::StatementLine {
            transaction_date,
            description,
            amount,
        });
    }
    Ok(lines)
}

#[derive(serde::Serialize)]
pub struct ImportStatementResponse {
    pub import_id: BankImportId,
    pub lines_imported: usize,
}

/// Well above any legitimate statement CSV, well under the app-wide body limit.
const MAX_STATEMENT_CSV_BYTES: usize = 10 * 1024 * 1024;

pub async fn import_statement(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<ImportStatementResponse>), ApiError> {
    let mut file_bytes = Vec::new();
    let mut filename = None;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| bad_request(e.to_string()))?
    {
        if field.name() == Some("file") {
            filename = field.file_name().map(|s| s.to_string());
            file_bytes = field
                .bytes()
                .await
                .map_err(|e| bad_request(e.to_string()))?
                .to_vec();
            if file_bytes.len() > MAX_STATEMENT_CSV_BYTES {
                return Err(bad_request(format!(
                    "Statement file is too large — {} MB, max {} MB",
                    file_bytes.len() / (1024 * 1024),
                    MAX_STATEMENT_CSV_BYTES / (1024 * 1024)
                )));
            }
            // Don't let a second "file" field override the first.
            break;
        }
    }
    if file_bytes.is_empty() {
        return Err(bad_request("No file uploaded"));
    }

    let lines = parse_statement_csv(&file_bytes).map_err(bad_request)?;
    if lines.is_empty() {
        return Err(bad_request("CSV contained no parseable rows"));
    }

    let mut tx = state.pg_pool.begin().await.map_err(internal_err)?;
    let (import_id, count) = accounting::bank_reconciliation::import_statement(
        &mut tx,
        user.user_id,
        filename.as_deref(),
        lines,
    )
    .await
    .map_err(internal_err)?;
    tx.commit().await.map_err(internal_err)?;

    Ok((
        StatusCode::CREATED,
        Json(ImportStatementResponse {
            import_id,
            lines_imported: count,
        }),
    ))
}

#[derive(serde::Serialize)]
pub struct AutoMatchResponse {
    pub matched: usize,
    pub unmatched: usize,
}

pub async fn auto_match(
    State(state): State<AppState>,
    axum::extract::Path(import_id): axum::extract::Path<BankImportId>,
) -> Result<Json<AutoMatchResponse>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let result = accounting::bank_reconciliation::auto_match_import(&mut conn, import_id)
        .await
        .map_err(internal_err)?;
    Ok(Json(AutoMatchResponse {
        matched: result.matched,
        unmatched: result.unmatched,
    }))
}

#[derive(serde::Deserialize)]
pub struct ListLinesRequest {
    pub import_id: Option<BankImportId>,
    pub match_status: Option<String>,
}

pub async fn list_lines(
    State(state): State<AppState>,
    Json(payload): Json<ListLinesRequest>,
) -> Result<Json<Vec<BankStatementLine>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let lines = accounting::db::list_bank_statement_lines(
        &mut conn,
        payload.import_id,
        payload.match_status.as_deref(),
    )
    .await
    .map_err(internal_err)?;
    Ok(Json(lines))
}

/// Candidate ledger transactions to manually match a statement line against.
pub async fn match_candidates(
    State(state): State<AppState>,
    axum::extract::Path(line_id): axum::extract::Path<BankLineId>,
) -> Result<Json<Vec<BankMatchCandidate>>, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let line = accounting::db::get_bank_statement_line(&mut conn, line_id)
        .await
        .map_err(internal_err)?
        .ok_or_else(|| not_found("Statement line not found"))?;
    let candidates = accounting::db::list_bank_match_candidates(&mut conn, line.transaction_date)
        .await
        .map_err(internal_err)?;
    Ok(Json(candidates))
}

#[derive(serde::Deserialize)]
pub struct MatchLineRequest {
    pub transaction_id: TransactionId,
}

pub async fn match_line(
    State(state): State<AppState>,
    axum::extract::Path(line_id): axum::extract::Path<BankLineId>,
    Json(payload): Json<MatchLineRequest>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let changed = accounting::bank_reconciliation::match_line_manually(
        &mut conn,
        line_id,
        payload.transaction_id,
    )
    .await
    .map_err(internal_err)?;
    if !changed {
        return Err(bad_request(
            "This line is no longer unmatched — it was already matched or ignored",
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}

pub async fn ignore_line(
    State(state): State<AppState>,
    axum::extract::Path(line_id): axum::extract::Path<BankLineId>,
) -> Result<StatusCode, ApiError> {
    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let changed = accounting::bank_reconciliation::ignore_line(&mut conn, line_id)
        .await
        .map_err(internal_err)?;
    if !changed {
        return Err(bad_request(
            "This line is no longer unmatched — it was already matched or ignored",
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}
