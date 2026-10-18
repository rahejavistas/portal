//! Admin-uploaded dues snapshot (the "No Dues" compliance file) that feeds the
//! `NoDues` process field — independent of the accounting module entirely.

use crate::extractors::SessionUser;
use crate::handlers::error::{bad_request, internal_err, ApiError};
use crate::state::AppState;
use axum::extract::{Multipart, State};
use axum::Json;
use base::flat::FlatIdentifier;
use calamine::{open_workbook_from_rs, Data, Reader, Xlsx};
use process::dues_import::FlatDueRow;
use rust_decimal::Decimal;
use std::io::Cursor;
use std::str::FromStr;

/// Well above any legitimate compliance export, well under the app-wide body limit.
const MAX_DUES_FILE_BYTES: usize = 10 * 1024 * 1024;

fn cell_text(cell: &Data) -> String {
    match cell {
        Data::String(s) => s.trim().to_string(),
        Data::Float(f) => {
            if f.fract() == 0.0 {
                format!("{}", *f as i64)
            } else {
                f.to_string()
            }
        }
        Data::Int(i) => i.to_string(),
        _ => String::new(),
    }
}

fn cell_decimal(cell: &Data) -> Option<Decimal> {
    match cell {
        Data::Float(f) => Decimal::from_str(&f.to_string()).ok(),
        Data::Int(i) => Some(Decimal::from(*i)),
        Data::String(s) => Decimal::from_str(s.trim()).ok(),
        _ => None,
    }
}

fn parse_dues_file(bytes: &[u8]) -> Result<Vec<FlatDueRow>, String> {
    let cursor = Cursor::new(bytes);
    let mut workbook: Xlsx<_> =
        open_workbook_from_rs(cursor).map_err(|e| format!("Could not open file: {e}"))?;
    let sheet_name = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or_else(|| "Workbook has no sheets".to_string())?;
    let range = workbook
        .worksheet_range(&sheet_name)
        .map_err(|e| format!("Could not read sheet: {e}"))?;

    let mut rows_iter = range.rows();
    let header_row = rows_iter
        .by_ref()
        .find(|row| {
            row.first()
                .map(|c| cell_text(c).eq_ignore_ascii_case("flat"))
                .unwrap_or(false)
        })
        .ok_or_else(|| "Could not find a header row starting with 'FLAT'".to_string())?;

    let total_due_idx = header_row
        .iter()
        .position(|c| cell_text(c).eq_ignore_ascii_case("total due"))
        .ok_or_else(|| "Header row is missing a 'Total Due' column".to_string())?;
    let overdue_idx = header_row
        .iter()
        .position(|c| cell_text(c).eq_ignore_ascii_case("over due"));

    let mut out = Vec::new();
    for row in rows_iter {
        let flat_code = row.first().map(cell_text).unwrap_or_default();
        if flat_code.is_empty() || flat_code.eq_ignore_ascii_case("total") {
            continue;
        }
        let Ok(flat) = FlatIdentifier::parse_concatenated(&flat_code) else {
            continue;
        };
        let Some(total_due) = row.get(total_due_idx).and_then(cell_decimal) else {
            continue;
        };
        let overdue_amount = overdue_idx
            .and_then(|i| row.get(i))
            .and_then(cell_decimal)
            .unwrap_or_default();
        out.push(FlatDueRow {
            wing: flat.wing().to_string(),
            flat_number: flat.flat_number().to_string(),
            total_due,
            overdue_amount,
        });
    }
    Ok(out)
}

#[derive(serde::Serialize)]
pub struct UploadDuesFileResponse {
    pub rows_parsed: usize,
    pub matched: usize,
    pub unmatched_flat_codes: Vec<String>,
}

/// POST /api/v1/admin/process-templates/no-dues/upload
pub async fn upload_no_dues_file(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<Json<UploadDuesFileResponse>, ApiError> {
    let mut file_bytes = Vec::new();
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| bad_request(e.to_string()))?
    {
        if field.name() == Some("file") {
            file_bytes = field
                .bytes()
                .await
                .map_err(|e| bad_request(e.to_string()))?
                .to_vec();
            if file_bytes.len() > MAX_DUES_FILE_BYTES {
                return Err(bad_request(format!(
                    "Dues file is too large - {} MB, max {} MB",
                    file_bytes.len() / (1024 * 1024),
                    MAX_DUES_FILE_BYTES / (1024 * 1024)
                )));
            }
            break;
        }
    }
    if file_bytes.is_empty() {
        return Err(bad_request("No file uploaded"));
    }

    let rows = parse_dues_file(&file_bytes).map_err(bad_request)?;
    if rows.is_empty() {
        return Err(bad_request("No flat rows found in the uploaded file"));
    }

    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let summary = process::dues_import::upsert_flat_dues(
        &mut conn,
        chrono::Utc::now().date_naive(),
        user.user_id,
        &rows,
    )
    .await
    .map_err(internal_err)?;

    Ok(Json(UploadDuesFileResponse {
        rows_parsed: rows.len(),
        matched: summary.matched,
        unmatched_flat_codes: summary.unmatched_flat_codes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_decimal_handles_every_numeric_cell_shape() {
        assert_eq!(
            cell_decimal(&Data::Float(45846.0)),
            Some(Decimal::from(45846))
        );
        assert_eq!(cell_decimal(&Data::Int(9823)), Some(Decimal::from(9823)));
        assert_eq!(
            cell_decimal(&Data::String("600.50".to_string())),
            Some(Decimal::from_str("600.50").unwrap())
        );
        assert_eq!(cell_decimal(&Data::Empty), None);
    }

    #[test]
    fn cell_decimal_preserves_a_negative_amount() {
        // The real compliance file has at least one row with a negative figure
        // (a credit note netted into a column) - must not silently become positive.
        assert_eq!(
            cell_decimal(&Data::Float(-660.0)),
            Some(Decimal::from(-660))
        );
    }

    #[test]
    fn cell_text_matches_header_names_case_insensitively() {
        assert_eq!(
            cell_text(&Data::String("Total Due".to_string())),
            "Total Due"
        );
        assert!(cell_text(&Data::String(" FLAT ".to_string())).eq_ignore_ascii_case("flat"));
    }
}
