//! Admin-uploaded dues snapshot — see `db::get_outstanding_dues_for_process`.

use rust_decimal::Decimal;
use std::collections::HashMap;

pub struct FlatDueRow {
    pub wing: String,
    pub flat_number: String,
    pub total_due: Decimal,
    pub overdue_amount: Decimal,
}

pub struct DuesImportSummary {
    pub matched: usize,
    pub unmatched_flat_codes: Vec<String>,
}

/// Upserts each matched flat's dues row, skipping any upload older than what's stored.
pub async fn upsert_flat_dues(
    conn: &mut sqlx::PgConnection,
    as_of: chrono::NaiveDate,
    uploaded_by: i32,
    rows: &[FlatDueRow],
) -> Result<DuesImportSummary, sqlx::Error> {
    if rows.is_empty() {
        return Ok(DuesImportSummary {
            matched: 0,
            unmatched_flat_codes: Vec::new(),
        });
    }

    // ON CONFLICT DO UPDATE can't touch the same row twice in one INSERT. Last
    // occurrence wins.
    let mut deduped: HashMap<(&str, &str), &FlatDueRow> = HashMap::new();
    for row in rows {
        deduped.insert((row.wing.as_str(), row.flat_number.as_str()), row);
    }
    let rows: Vec<&FlatDueRow> = deduped.into_values().collect();

    let wings: Vec<&str> = rows.iter().map(|r| r.wing.as_str()).collect();
    let flat_numbers: Vec<&str> = rows.iter().map(|r| r.flat_number.as_str()).collect();
    let total_dues: Vec<Decimal> = rows.iter().map(|r| r.total_due).collect();
    let overdue_amounts: Vec<Decimal> = rows.iter().map(|r| r.overdue_amount).collect();

    sqlx::query(
        "WITH input(wing, flat_number, total_due, overdue_amount) AS ( \
             SELECT * FROM UNNEST($1::text[], $2::text[], $3::numeric[], $4::numeric[]) \
         ) \
         INSERT INTO flat_dues_from_file (flat_id, total_due, overdue_amount, as_of, uploaded_by, uploaded_at) \
         SELECT f.flat_id, i.total_due, i.overdue_amount, $5, $6, NOW() \
         FROM input i JOIN flats f ON f.wing = i.wing AND f.flat_number = i.flat_number \
         ON CONFLICT (flat_id) DO UPDATE SET \
             total_due = EXCLUDED.total_due, \
             overdue_amount = EXCLUDED.overdue_amount, \
             as_of = EXCLUDED.as_of, \
             uploaded_by = EXCLUDED.uploaded_by, \
             uploaded_at = NOW() \
         WHERE EXCLUDED.as_of >= flat_dues_from_file.as_of",
    )
    .bind(&wings)
    .bind(&flat_numbers)
    .bind(&total_dues)
    .bind(&overdue_amounts)
    .bind(as_of)
    .bind(uploaded_by)
    .execute(&mut *conn)
    .await?;

    let unmatched: Vec<(String, String)> = sqlx::query_as(
        "WITH input(wing, flat_number) AS ( \
             SELECT * FROM UNNEST($1::text[], $2::text[]) \
         ) \
         SELECT i.wing, i.flat_number FROM input i \
         LEFT JOIN flats f ON f.wing = i.wing AND f.flat_number = i.flat_number \
         WHERE f.flat_id IS NULL",
    )
    .bind(&wings)
    .bind(&flat_numbers)
    .fetch_all(&mut *conn)
    .await?;

    let unmatched_flat_codes: Vec<String> = unmatched
        .into_iter()
        .map(|(wing, flat_number)| format!("{}{}", wing, flat_number))
        .collect();
    Ok(DuesImportSummary {
        matched: rows.len() - unmatched_flat_codes.len(),
        unmatched_flat_codes,
    })
}
