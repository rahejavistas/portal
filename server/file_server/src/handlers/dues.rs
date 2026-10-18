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
        Data::String(s) => Decimal::from_str(s.trim().replace(',', "").as_str()).ok(),
        _ => None,
    }
}

/// Finds a row like `"AS On 20/10/2026"` and parses its date. Falls back to today
/// if no such row exists or its date doesn't parse, rather than failing the upload.
fn extract_as_of_date(range: &calamine::Range<Data>) -> chrono::NaiveDate {
    range
        .rows()
        .find_map(|row| {
            let text = row.first().map(cell_text)?;
            let idx = text.to_ascii_lowercase().find("as on")?;
            let date_str = text[idx + "as on".len()..].trim();
            chrono::NaiveDate::parse_from_str(date_str, "%d/%m/%Y").ok()
        })
        .unwrap_or_else(|| chrono::Utc::now().date_naive())
}

fn parse_dues_file(
    bytes: &[u8],
) -> Result<(chrono::NaiveDate, Vec<FlatDueRow>, Vec<String>), String> {
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
    let as_of = extract_as_of_date(&range);

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
    let mut skipped = Vec::new();
    for row in rows_iter {
        let flat_code = row.first().map(cell_text).unwrap_or_default();
        if flat_code.is_empty() || flat_code.eq_ignore_ascii_case("total") {
            continue;
        }
        let Ok(flat) = FlatIdentifier::parse_concatenated(&flat_code) else {
            skipped.push(format!("{flat_code}: not a recognizable flat code"));
            continue;
        };
        let Some(total_due) = row.get(total_due_idx).and_then(cell_decimal) else {
            skipped.push(format!("{flat_code}: Total Due did not parse as a number"));
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
    Ok((as_of, out, skipped))
}

#[derive(serde::Serialize)]
pub struct UploadDuesFileResponse {
    pub as_of: chrono::NaiveDate,
    pub rows_parsed: usize,
    pub matched: usize,
    pub unmatched_flat_codes: Vec<String>,
    pub skipped_rows: Vec<String>,
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
                    "Dues file is too large: {} MB, max {} MB",
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

    let (as_of, rows, skipped_rows) = parse_dues_file(&file_bytes).map_err(bad_request)?;
    if rows.is_empty() {
        return Err(bad_request("No flat rows found in the uploaded file"));
    }

    let mut conn = state.pg_pool.acquire().await.map_err(internal_err)?;
    let summary = process::dues_import::upsert_flat_dues(&mut conn, as_of, user.user_id, &rows)
        .await
        .map_err(internal_err)?;

    Ok(Json(UploadDuesFileResponse {
        as_of,
        skipped_rows,
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
    fn cell_decimal_strips_thousands_separators_from_a_text_amount() {
        assert_eq!(
            cell_decimal(&Data::String("7,500.00".to_string())),
            Some(Decimal::from_str("7500.00").unwrap())
        );
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

    #[test]
    fn parse_dues_file_reads_the_as_of_date_from_the_file_itself() {
        let bytes = include_bytes!("../../../../quality/fixtures/no_dues_sample.xlsx");
        let (as_of, _, _) = parse_dues_file(bytes).expect("fixture must parse");
        assert_eq!(
            as_of,
            chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            "the file's own 'AS On 01/01/2026' row must be used, not today's date"
        );
    }

    #[test]
    fn extract_as_of_date_is_case_insensitive_and_trims_trailing_space() {
        use calamine::{Cell, Range};
        let range = Range::from_sparse(vec![Cell::new(
            (0, 0),
            Data::String("as ON 20/10/2026 ".to_string()),
        )]);
        assert_eq!(
            extract_as_of_date(&range),
            chrono::NaiveDate::from_ymd_opt(2026, 10, 20).unwrap()
        );
    }

    #[test]
    fn extract_as_of_date_falls_back_to_today_when_the_row_is_missing_or_malformed() {
        use calamine::{Cell, Range};
        let range = Range::from_sparse(vec![Cell::new(
            (0, 0),
            Data::String("No date row here".to_string()),
        )]);
        assert_eq!(extract_as_of_date(&range), chrono::Utc::now().date_naive());

        let malformed = Range::from_sparse(vec![Cell::new(
            (0, 0),
            Data::String("AS On not-a-date".to_string()),
        )]);
        assert_eq!(
            extract_as_of_date(&malformed),
            chrono::Utc::now().date_naive()
        );
    }

    #[test]
    fn parse_dues_file_reads_the_committed_sample_fixture() {
        let bytes = include_bytes!("../../../../quality/fixtures/no_dues_sample.xlsx");
        let (_, mut rows, skipped) = parse_dues_file(bytes).expect("fixture must parse");
        assert_eq!(
            skipped,
            vec!["Z0004: Total Due did not parse as a number".to_string()],
            "Z0004's non-numeric Total Due must be reported, not silently dropped"
        );
        rows.sort_by_key(|r| r.flat_number.clone());

        assert_eq!(
            rows.len(),
            3,
            "the 'Total' sentinel row must not be parsed as a flat"
        );
        assert!(rows.iter().all(|r| r.wing == "Z"));

        assert_eq!(rows[0].flat_number, "0001");
        assert_eq!(rows[0].total_due, Decimal::from(1000));
        assert_eq!(rows[0].overdue_amount, Decimal::from(0));

        assert_eq!(rows[1].flat_number, "0002");
        assert_eq!(rows[1].total_due, Decimal::from(0));
        assert_eq!(rows[1].overdue_amount, Decimal::from(0));

        assert_eq!(rows[2].flat_number, "0003");
        assert_eq!(
            rows[2].total_due,
            Decimal::from(500),
            "a negative figure in an unrelated column must not corrupt Total Due"
        );
        assert_eq!(rows[2].overdue_amount, Decimal::from(200));
    }
}
