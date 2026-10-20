use crate::state::AppState;
use axum::extract::Multipart;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use base::flat::FlatIdentifier;
use calamine::{open_workbook_from_rs, Data, Reader, Xlsx};
use std::collections::BTreeSet;

#[derive(serde::Serialize)]
pub struct FailedUser {
    pub email: String,
    pub reason: String,
}

#[derive(serde::Serialize)]
pub struct UploadUsersResult {
    pub total_rows: usize,
    pub successfully_onboarded: usize,
    pub flats_created: usize,
    pub failed_users: Vec<FailedUser>,
}

struct ParsedUser {
    email: String,
    flat: FlatIdentifier,
}

/// `flats` is every well-formed flat in the file, whether or not its row yielded a
/// usable user, so the wing/flat catalog reflects the file even when some rows fail.
struct ParsedFile {
    users: Vec<ParsedUser>,
    failed_users: Vec<FailedUser>,
    total_rows: usize,
    flats: Vec<FlatIdentifier>,
}

/// A flat claimed by more than one row is dropped from both parsers — the same
/// email recurring across *different* flats (a resident owning multiple units)
/// is fine and untouched, since this is keyed by flat, not by email.
fn dedupe_ambiguous_flats(
    parsed_users: Vec<ParsedUser>,
    failed_users: &mut Vec<FailedUser>,
) -> Vec<ParsedUser> {
    let mut flat_counts = std::collections::HashMap::new();
    for user in &parsed_users {
        *flat_counts.entry(user.flat.to_string()).or_insert(0) += 1;
    }

    let mut valid_users = Vec::new();
    for user in parsed_users {
        let flat_key = user.flat.to_string();
        if flat_counts.get(&flat_key).copied().unwrap_or(0) > 1 {
            failed_users.push(FailedUser {
                email: user.email,
                reason: format!("Ambiguous: Multiple rows claim flat {}", flat_key),
            });
        } else {
            valid_users.push(user);
        }
    }
    valid_users
}

fn parse_csv(csv_data: &[u8]) -> Result<ParsedFile, String> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .from_reader(csv_data);

    let headers = reader
        .headers()
        .map_err(|_| "Failed to read CSV headers")?
        .clone();

    let email_idx = headers
        .iter()
        .position(|s| s.to_lowercase().contains("email"));
    let wing_idx = headers
        .iter()
        .position(|s| s.to_lowercase().contains("wing"));
    let flat_idx = headers
        .iter()
        .position(|s| s.to_lowercase().contains("flat"));

    let (Some(e_idx), Some(w_idx), Some(f_idx)) = (email_idx, wing_idx, flat_idx) else {
        return Err("CSV must contain 'Email', 'Wing', and 'Flat' columns".to_string());
    };

    let mut failed_users = Vec::new();
    let mut total_rows = 0;
    let mut parsed_users = Vec::new();

    for result in reader.records() {
        total_rows += 1;

        let row = match result {
            Ok(r) => r,
            Err(e) => {
                failed_users.push(FailedUser {
                    email: format!("(row {})", total_rows),
                    reason: format!("Could not parse this CSV row: {}", e),
                });
                continue;
            }
        };

        let email = row.get(e_idx).unwrap_or_default().trim().to_lowercase();
        let wing = row.get(w_idx).unwrap_or_default().trim().to_uppercase();
        let flat_raw = row.get(f_idx).unwrap_or_default().trim().to_string();

        if email.is_empty() && wing.is_empty() && flat_raw.is_empty() {
            total_rows -= 1; // Ignore completely empty rows
            continue;
        }

        if !email.contains('@') || !email.contains('.') {
            failed_users.push(FailedUser {
                email: email.clone(),
                reason: "Invalid email format.".to_string(),
            });
            continue;
        }

        let flat_num: i32 = match flat_raw.parse() {
            Ok(n) => n,
            Err(_) => {
                failed_users.push(FailedUser {
                    email: email.clone(),
                    reason: format!("Invalid Flat format: {}. Must be a number.", flat_raw),
                });
                continue;
            }
        };

        if flat_num <= 200 || flat_num >= 2100 {
            failed_users.push(FailedUser {
                email: email.clone(),
                reason: format!("Invalid Flat: {}. Must be > 200 and < 2100.", flat_raw),
            });
            continue;
        }

        let last_two = flat_num % 100;
        if !(1..=10).contains(&last_two) {
            failed_users.push(FailedUser {
                email: email.clone(),
                reason: format!(
                    "Invalid Flat {}: Last two digits must be between 01 and 10.",
                    flat_raw
                ),
            });
            continue;
        }

        let allowed_wings = ["A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K"];
        if !allowed_wings.contains(&wing.as_str()) {
            failed_users.push(FailedUser {
                email: email.clone(),
                reason: format!("Invalid Wing: {}. Must be A-K.", wing),
            });
            continue;
        }

        let flat_str = format!("{:04}", flat_num);
        let flat = match FlatIdentifier::new(&wing, &flat_str) {
            Ok(f) => f,
            Err(e) => {
                failed_users.push(FailedUser { email, reason: e });
                continue;
            }
        };

        parsed_users.push(ParsedUser { email, flat });
    }

    let flats = parsed_users.iter().map(|u| u.flat.clone()).collect();
    let users = dedupe_ambiguous_flats(parsed_users, &mut failed_users);
    Ok(ParsedFile {
        users,
        failed_users,
        total_rows,
        flats,
    })
}

fn open_first_sheet(bytes: &[u8]) -> Result<calamine::Range<Data>, String> {
    let cursor = std::io::Cursor::new(bytes);
    let mut workbook: Xlsx<_> =
        open_workbook_from_rs(cursor).map_err(|e| format!("Could not open file: {e}"))?;
    let sheet_name = workbook
        .sheet_names()
        .first()
        .cloned()
        .ok_or_else(|| "Workbook has no sheets".to_string())?;
    workbook
        .worksheet_range(&sheet_name)
        .map_err(|e| format!("Could not read sheet: {e}"))
}

fn column_index(header_row: &[Data], name: &str) -> Option<usize> {
    header_row
        .iter()
        .position(|c| cell_text(c).eq_ignore_ascii_case(name))
}

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

/// The "member data" export every onboarding batch actually comes as: one row per
/// flat, headers `UnitNo | MemberName | MobileNo | EmailID | CCMail`. `CCMail` (a
/// secondary contact, not a login) is intentionally not onboarded — this schema
/// only allows one active owner per flat (`idx_active_flat_ownership`), so a second
/// account on the same unit would just fail on conflict.
fn parse_member_xlsx(bytes: &[u8]) -> Result<ParsedFile, String> {
    let range = open_first_sheet(bytes)?;
    let mut rows_iter = range.rows();
    let header_row = rows_iter
        .by_ref()
        .find(|row| column_index(row, "unitno").is_some())
        .ok_or_else(|| "Could not find a header row with a 'UnitNo' column".to_string())?;

    let unit_idx =
        column_index(header_row, "unitno").expect("header row was selected for having UnitNo");
    let email_idx = column_index(header_row, "emailid")
        .ok_or_else(|| "Header row is missing an 'EmailID' column".to_string())?;

    let mut failed_users = Vec::new();
    let mut parsed_users = Vec::new();
    let mut flats = Vec::new();
    let mut total_rows = 0;

    for row in rows_iter {
        let unit_raw = row.get(unit_idx).map(cell_text).unwrap_or_default();
        let email = row
            .get(email_idx)
            .map(cell_text)
            .unwrap_or_default()
            .trim()
            .to_lowercase();
        let flat = match FlatIdentifier::parse_concatenated(&unit_raw) {
            Ok(f) => f,
            Err(_) => {
                // A genuine title/footer row (e.g. a trailing "XXXX" summary line) has
                // no real data at all - skip it silently. A row with a real EmailID but
                // a broken UnitNo is an actual failed row, not a footer to ignore.
                if email.is_empty() {
                    continue;
                }
                total_rows += 1;
                failed_users.push(FailedUser {
                    email,
                    reason: format!("Invalid UnitNo: {unit_raw}."),
                });
                continue;
            }
        };
        total_rows += 1;
        flats.push(flat.clone());

        if email.is_empty() {
            failed_users.push(FailedUser {
                email: format!("(flat {})", flat),
                reason: "Missing EmailID.".to_string(),
            });
            continue;
        }
        if !email.contains('@') || !email.contains('.') {
            failed_users.push(FailedUser {
                email,
                reason: "Invalid email format.".to_string(),
            });
            continue;
        }
        parsed_users.push(ParsedUser { email, flat });
    }

    let users = dedupe_ambiguous_flats(parsed_users, &mut failed_users);
    Ok(ParsedFile {
        users,
        failed_users,
        total_rows,
        flats,
    })
}

/// Registers every wing and flat in the file. Idempotent: existing wings and
/// flats are left alone. Returns how many flats were newly created.
async fn register_flats(
    state: &AppState,
    flats: &[FlatIdentifier],
    created_by: i32,
) -> Result<usize, String> {
    let wings: BTreeSet<&str> = flats.iter().map(FlatIdentifier::wing).collect();
    for wing in wings {
        state
            .permissions_manager
            .ensure_wing_exists(wing, Some(created_by))
            .await?;
    }

    let (wings, flat_numbers): (Vec<&str>, Vec<&str>) =
        flats.iter().map(|f| (f.wing(), f.flat_number())).unzip();
    let result = sqlx::query(
        "INSERT INTO flats (wing, flat_number) \
         SELECT * FROM UNNEST($1::text[], $2::text[]) \
         ON CONFLICT (wing, flat_number) DO NOTHING",
    )
    .bind(&wings)
    .bind(&flat_numbers)
    .execute(&state.pg_pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(result.rows_affected() as usize)
}

pub async fn upload_users(
    State(state): State<AppState>,
    crate::extractors::SessionUser(caller): crate::extractors::SessionUser,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut file_bytes = Vec::new();

    while let Some(field) = multipart.next_field().await.unwrap_or(None) {
        if field.name() == Some("file") {
            if let Ok(bytes) = field.bytes().await {
                file_bytes = bytes.to_vec();
            }
        }
    }

    if file_bytes.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"status": "error", "message": "No file uploaded"})),
        )
            .into_response();
    }

    // XLSX files are ZIP archives ("PK" signature); anything else is treated as CSV.
    let parsed = if file_bytes.starts_with(b"PK") {
        parse_member_xlsx(&file_bytes)
    } else {
        parse_csv(&file_bytes)
    };

    let ParsedFile {
        users: valid_users,
        mut failed_users,
        total_rows,
        flats,
    } = match parsed {
        Ok(res) => res,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"status": "error", "message": e})),
            )
                .into_response()
        }
    };

    let flats_created = match register_flats(&state, &flats, caller.user_id).await {
        Ok(n) => n,
        Err(e) => {
            eprintln!(
                "Bulk upload: registering {} flats failed: {}",
                flats.len(),
                e
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"status": "error", "message": "Could not register wings and flats from the file"})),
            )
                .into_response();
        }
    };

    let mut successfully_onboarded = 0;

    // No welcome/invite email is sent here for any row - bulk-created accounts
    // get onboarded silently; residents set their password via the normal
    // forgot-password flow whenever they first log in.
    for user in valid_users {
        match state
            .permissions_manager
            .create_user_with_credentials(
                &user.email,
                None,
                user.flat.wing(),
                user.flat.flat_number(),
                Some(caller.user_id),
            )
            .await
        {
            Ok(_) => successfully_onboarded += 1,
            Err(e) => failed_users.push(FailedUser {
                email: user.email,
                reason: e,
            }),
        }
    }

    (
        StatusCode::OK,
        Json(UploadUsersResult {
            total_rows,
            successfully_onboarded,
            flats_created,
            failed_users,
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(email: &str, wing: &str, flat_number: &str) -> ParsedUser {
        ParsedUser {
            email: email.to_string(),
            flat: FlatIdentifier::new(wing, flat_number).unwrap(),
        }
    }

    #[test]
    fn dedupe_keeps_the_same_email_across_different_flats() {
        // A real resident owning multiple units - must not be treated as ambiguous,
        // since the check is keyed by flat, not by email.
        let mut failed = Vec::new();
        let valid = dedupe_ambiguous_flats(
            vec![
                parsed("owner@vistas.com", "A", "0301"),
                parsed("owner@vistas.com", "K", "2002"),
            ],
            &mut failed,
        );
        assert_eq!(valid.len(), 2);
        assert!(failed.is_empty());
    }

    #[test]
    fn dedupe_rejects_two_different_emails_claiming_the_same_flat() {
        let mut failed = Vec::new();
        let valid = dedupe_ambiguous_flats(
            vec![
                parsed("first@vistas.com", "A", "0301"),
                parsed("second@vistas.com", "A", "0301"),
            ],
            &mut failed,
        );
        assert!(valid.is_empty());
        assert_eq!(failed.len(), 2);
    }

    #[test]
    fn parse_member_xlsx_skips_title_and_trailer_rows_without_a_real_flat_code() {
        // "XXXX" (a trailer summary row) and a blank title row have no digit at
        // all for FlatIdentifier::parse_concatenated to split on.
        assert!(FlatIdentifier::parse_concatenated("XXXX").is_err());
        assert!(
            FlatIdentifier::parse_concatenated("RAHEJA VISTAS CO-OP HOUSING SOCIETY LTD").is_err()
        );
    }

    #[test]
    fn cell_text_reads_every_cell_shape_the_real_file_can_contain() {
        assert_eq!(cell_text(&Data::String("A0301".to_string())), "A0301");
        assert_eq!(cell_text(&Data::String(" UnitNo ".to_string())), "UnitNo");
        assert!(cell_text(&Data::Empty).is_empty());
    }

    #[test]
    fn parse_member_xlsx_reads_the_committed_sample_fixture() {
        let bytes = include_bytes!("../../../../quality/fixtures/member_data_sample.xlsx");
        let ParsedFile {
            mut users,
            failed_users: failed,
            total_rows,
            flats,
        } = parse_member_xlsx(bytes).expect("fixture must parse");
        users.sort_by_key(|a| a.flat.to_string());

        assert_eq!(
            total_rows, 4,
            "the title row and the 'XXXX' trailer must not count as data rows, but the row with a broken UnitNo and a real EmailID must"
        );
        assert_eq!(
            failed.len(),
            1,
            "the '????' row has a real EmailID, so a broken UnitNo must be a reported failure, not a silent skip"
        );
        assert_eq!(failed[0].email, "broken.unit@example.test");
        assert!(failed[0].reason.contains("Invalid UnitNo"));
        assert_eq!(users.len(), 3);
        assert_eq!(
            flats.iter().map(|f| f.to_string()).collect::<Vec<_>>(),
            ["Z-0001", "Z-0002", "Z-0003"],
            "every well-formed UnitNo registers a flat, but the broken '????' one does not"
        );

        assert_eq!(users[0].flat.to_string(), "Z-0001");
        assert_eq!(users[0].email, "resident.one@example.test");
        assert_eq!(users[1].flat.to_string(), "Z-0002");
        assert_eq!(
            users[1].email, "resident.two@example.test",
            "CCMail must never be onboarded as the account email"
        );
        assert_eq!(users[2].flat.to_string(), "Z-0003");
        assert_eq!(users[2].email, "resident.three@example.test");
    }
}
