use crate::state::AppState;
use axum::extract::Multipart;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use base::flat::FlatIdentifier;

#[derive(serde::Serialize)]
pub struct FailedUser {
    pub email: String,
    pub reason: String,
}

#[derive(serde::Serialize)]
pub struct UploadUsersResult {
    pub total_rows: usize,
    pub successfully_onboarded: usize,
    pub failed_users: Vec<FailedUser>,
}

struct ParsedUser {
    email: String,
    flat: FlatIdentifier,
}

fn parse_csv(csv_data: &[u8]) -> Result<(Vec<ParsedUser>, Vec<FailedUser>, usize), String> {
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

    let mut valid_users = Vec::new();
    let mut failed_users = Vec::new();
    let mut total_rows = 0;
    let mut parsed_users = Vec::new();
    let mut flat_counts = std::collections::HashMap::new();

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
                failed_users.push(FailedUser {
                    email: email.clone(),
                    reason: e,
                });
                continue;
            }
        };
        let flat_key = flat.to_string();

        *flat_counts.entry(flat_key.clone()).or_insert(0) += 1;

        parsed_users.push(ParsedUser { email, flat });
    }

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

    Ok((valid_users, failed_users, total_rows))
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

    let (valid_users, mut failed_users, total_rows) = match parse_csv(&file_bytes) {
        Ok(res) => res,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"status": "error", "message": e})),
            )
                .into_response()
        }
    };

    let mut successfully_onboarded = 0;

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
            failed_users,
        }),
    )
        .into_response()
}
