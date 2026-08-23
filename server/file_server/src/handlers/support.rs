use crate::extractors::SessionUser;
use crate::handlers::error::{bad_request, internal_err, ApiError};
use crate::state::AppState;
use axum::{extract::Multipart, extract::State, http::StatusCode};

const MAX_ATTACHMENT_BYTES: usize = 5 * 1024 * 1024;
const ALLOWED_IMAGE_TYPES: [&str; 3] = ["image/png", "image/jpeg", "image/webp"];

pub async fn raise_ticket(
    SessionUser(user): SessionUser,
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> Result<StatusCode, ApiError> {
    let mut message = String::new();
    let mut attachment: Option<email::Attachment> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| bad_request(format!("Invalid form data: {e}")))?
    {
        match field.name() {
            Some("message") => {
                message = field
                    .text()
                    .await
                    .map_err(|e| bad_request(format!("Invalid message field: {e}")))?;
            }
            Some("image") => {
                let filename = field.file_name().unwrap_or("attachment").to_string();
                let content_type = field
                    .content_type()
                    .unwrap_or("application/octet-stream")
                    .to_string();
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| bad_request(format!("Invalid image field: {e}")))?;

                if bytes.is_empty() {
                    continue;
                }
                if bytes.len() > MAX_ATTACHMENT_BYTES {
                    return Err(bad_request("Attachment must be 5MB or smaller"));
                }
                if !ALLOWED_IMAGE_TYPES.contains(&content_type.as_str()) {
                    return Err(bad_request("Attachment must be a PNG, JPEG, or WebP image"));
                }

                attachment = Some(email::Attachment {
                    filename,
                    content_type,
                    bytes: bytes.to_vec(),
                });
            }
            _ => {}
        }
    }

    let message = message.trim();
    if message.is_empty() {
        return Err(bad_request("Message cannot be empty"));
    }

    let payload = email::EmailMessage {
        to: state.super_admin_email.clone(),
        subject: format!("Support ticket from {}", user.email),
        heading: "New Support Ticket".to_string(),
        paragraphs: vec![format!("From: {}", user.email), message.to_string()],
        code: None,
        cta: None,
        attachment,
    };

    email::send_email(&payload).await.map_err(internal_err)?;

    Ok(StatusCode::NO_CONTENT)
}
