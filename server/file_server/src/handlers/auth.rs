use crate::extractors::generate_token;
use crate::state::AppState;
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use rand::{thread_rng, Rng};

#[derive(serde::Deserialize)]
pub struct LoginPayload {
    email: String,
    password: String,
}

#[derive(serde::Serialize)]
pub struct LoginResponse {
    status: String,
    user: Option<authentication::User>,
    token: Option<String>,
    message: Option<String>,
    /// True when the password check succeeded but `/mfa/verify` still needs to happen.
    #[serde(default)]
    mfa_required: bool,
    #[serde(default)]
    pending_token: Option<String>,
}

const MFA_OTP_EXPIRY_SECONDS: i64 = 10 * 60;

fn generate_otp_code() -> String {
    format!("{:06}", thread_rng().gen_range(0..1_000_000))
}

fn generate_pending_token() -> String {
    thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect()
}

async fn send_mfa_otp_email(to: &str, code: &str) {
    let payload = email::EmailMessage {
        to: to.to_string(),
        subject: format!("{} is your Raheja Vistas login code", code),
        heading: "Your Login Code".to_string(),
        paragraphs: vec![
            "Enter this code to finish signing in. It expires in 10 minutes.".to_string(),
            "If you didn't try to log in, you can ignore this email.".to_string(),
        ],
        code: Some(code.to_string()),
        cta: None,
    };
    if let Err(e) = email::send_email(&payload).await {
        eprintln!("MFA OTP email send error: {}", e);
    }
}

// super_admin-always-required is disabled for now — every account is opt-in only.
async fn requires_mfa(state: &AppState, user: &authentication::User) -> Result<bool, sqlx::Error> {
    state
        .permissions_manager
        .get_mfa_enabled(user.user_id)
        .await
}

pub async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginPayload>,
) -> impl IntoResponse {
    match state
        .permissions_manager
        .check_authenticated(&payload.email, &payload.password)
        .await
    {
        Ok(authentication::AuthResult::Success(user)) => {
            let needs_mfa = match requires_mfa(&state, &user).await {
                Ok(v) => v,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(LoginResponse {
                            status: "error".to_string(),
                            user: None,
                            token: None,
                            message: Some(format!("Database error: {}", e)),
                            mfa_required: false,
                            pending_token: None,
                        }),
                    )
                        .into_response();
                }
            };

            if needs_mfa {
                let code = generate_otp_code();
                let pending_token = generate_pending_token();
                if let Err(e) = state
                    .permissions_manager
                    .create_mfa_challenge(
                        user.user_id,
                        &code,
                        &pending_token,
                        MFA_OTP_EXPIRY_SECONDS,
                    )
                    .await
                {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(LoginResponse {
                            status: "error".to_string(),
                            user: None,
                            token: None,
                            message: Some(format!("Database error: {}", e)),
                            mfa_required: false,
                            pending_token: None,
                        }),
                    )
                        .into_response();
                }
                send_mfa_otp_email(&user.email, &code).await;
                return (
                    StatusCode::OK,
                    Json(LoginResponse {
                        status: "mfa_required".to_string(),
                        user: None,
                        token: None,
                        message: Some(
                            "Enter the code we emailed you to finish logging in.".to_string(),
                        ),
                        mfa_required: true,
                        pending_token: Some(pending_token),
                    }),
                )
                    .into_response();
            }

            match generate_token(&user, &state.jwt_secret, state.jwt_expiry) {
                Ok(token) => (
                    StatusCode::OK,
                    Json(LoginResponse {
                        status: "success".to_string(),
                        user: Some(user),
                        token: Some(token),
                        message: None,
                        mfa_required: false,
                        pending_token: None,
                    }),
                )
                    .into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(LoginResponse {
                        status: "error".to_string(),
                        user: None,
                        token: None,
                        message: Some(format!("Token generation failed: {}", e)),
                        mfa_required: false,
                        pending_token: None,
                    }),
                )
                    .into_response(),
            }
        }
        Ok(authentication::AuthResult::InvalidCredentials) => (
            StatusCode::UNAUTHORIZED,
            Json(LoginResponse {
                status: "error".to_string(),
                user: None,
                token: None,
                message: Some("Invalid email or password".to_string()),
                mfa_required: false,
                pending_token: None,
            }),
        )
            .into_response(),
        Ok(authentication::AuthResult::AccountLocked) => (
            StatusCode::FORBIDDEN,
            Json(LoginResponse {
                status: "error".to_string(),
                user: None,
                token: None,
                message: Some(
                    "Account locked due to too many failed attempts. Please reset your password."
                        .to_string(),
                ),
                mfa_required: false,
                pending_token: None,
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(LoginResponse {
                status: "error".to_string(),
                user: None,
                token: None,
                message: Some(format!("Database error: {}", e)),
                mfa_required: false,
                pending_token: None,
            }),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct MfaVerifyPayload {
    pending_token: String,
    code: String,
}

pub async fn mfa_verify(
    State(state): State<AppState>,
    Json(payload): Json<MfaVerifyPayload>,
) -> impl IntoResponse {
    match state
        .permissions_manager
        .verify_mfa_otp(&payload.pending_token, &payload.code)
        .await
    {
        Ok(authentication::MfaVerifyResult::Success(user)) => {
            match generate_token(&user, &state.jwt_secret, state.jwt_expiry) {
                Ok(token) => (
                    StatusCode::OK,
                    Json(LoginResponse {
                        status: "success".to_string(),
                        user: Some(user),
                        token: Some(token),
                        message: None,
                        mfa_required: false,
                        pending_token: None,
                    }),
                )
                    .into_response(),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(LoginResponse {
                        status: "error".to_string(),
                        user: None,
                        token: None,
                        message: Some(format!("Token generation failed: {}", e)),
                        mfa_required: false,
                        pending_token: None,
                    }),
                )
                    .into_response(),
            }
        }
        Ok(authentication::MfaVerifyResult::InvalidCode) => (
            StatusCode::UNAUTHORIZED,
            Json(LoginResponse {
                status: "error".to_string(),
                user: None,
                token: None,
                message: Some("Incorrect code. Please try again.".to_string()),
                mfa_required: true,
                pending_token: Some(payload.pending_token),
            }),
        )
            .into_response(),
        Ok(authentication::MfaVerifyResult::TooManyAttempts) => (
            StatusCode::FORBIDDEN,
            Json(LoginResponse {
                status: "error".to_string(),
                user: None,
                token: None,
                message: Some(
                    "Too many incorrect attempts. Please log in again to get a new code."
                        .to_string(),
                ),
                mfa_required: false,
                pending_token: None,
            }),
        )
            .into_response(),
        Ok(authentication::MfaVerifyResult::Expired) => (
            StatusCode::UNAUTHORIZED,
            Json(LoginResponse {
                status: "error".to_string(),
                user: None,
                token: None,
                message: Some(
                    "This code has expired. Please log in again to get a new code.".to_string(),
                ),
                mfa_required: false,
                pending_token: None,
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(LoginResponse {
                status: "error".to_string(),
                user: None,
                token: None,
                message: Some(format!("Database error: {}", e)),
                mfa_required: false,
                pending_token: None,
            }),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct MfaResendPayload {
    pending_token: String,
}

#[derive(serde::Serialize)]
pub struct MfaResendResponse {
    status: String,
    message: String,
}

pub async fn mfa_resend(
    State(state): State<AppState>,
    Json(payload): Json<MfaResendPayload>,
) -> impl IntoResponse {
    let user_id = match state
        .permissions_manager
        .get_user_id_for_pending_mfa(&payload.pending_token)
        .await
    {
        Ok(Some(id)) => id,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(MfaResendResponse {
                    status: "error".to_string(),
                    message: "This login attempt has expired. Please log in again.".to_string(),
                }),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MfaResendResponse {
                    status: "error".to_string(),
                    message: format!("Database error: {}", e),
                }),
            )
                .into_response();
        }
    };

    let Ok(Some(user)) = state.permissions_manager.get_user_by_id(user_id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(MfaResendResponse {
                status: "error".to_string(),
                message: "This login attempt has expired. Please log in again.".to_string(),
            }),
        )
            .into_response();
    };

    let code = generate_otp_code();
    match state
        .permissions_manager
        .resend_mfa_challenge(user_id, &code, MFA_OTP_EXPIRY_SECONDS)
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::FORBIDDEN,
                Json(MfaResendResponse {
                    status: "error".to_string(),
                    message: "Too many resend requests. Please log in again to get a new code."
                        .to_string(),
                }),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(MfaResendResponse {
                    status: "error".to_string(),
                    message: format!("Database error: {}", e),
                }),
            )
                .into_response();
        }
    }
    send_mfa_otp_email(&user.email, &code).await;

    (
        StatusCode::OK,
        Json(MfaResendResponse {
            status: "success".to_string(),
            message: "A new code has been sent.".to_string(),
        }),
    )
        .into_response()
}

#[derive(serde::Deserialize)]
pub struct ForgotPasswordPayload {
    email: String,
}

#[derive(serde::Serialize)]
pub struct ForgotPasswordResponse {
    status: String,
    message: String,
}

pub async fn forgot_password(
    State(state): State<AppState>,
    Json(payload): Json<ForgotPasswordPayload>,
) -> Response {
    let token: String = thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();

    let email = payload.email.trim().to_lowercase();

    match state.permissions_manager.email_exists(&email).await {
        Ok(true) => {
            if let Err(e) = state
                .permissions_manager
                .create_reset_token(
                    &email,
                    &token,
                    state.forgot_password_expiry.as_secs() as i64,
                )
                .await
            {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "status": "error",
                        "message": format!("Failed to set reset token: {}", e)
                    })),
                )
                    .into_response();
            }
        }
        Ok(false) => {
            // Same response as a registered email — an unregistered-email 404 would let
            // anyone enumerate which addresses have accounts.
            return (
                StatusCode::OK,
                Json(ForgotPasswordResponse {
                    status: "success".to_string(),
                    message: "Password setup/reset link generated successfully".to_string(),
                }),
            )
                .into_response();
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "status": "error",
                    "message": format!("Database error: {}", e)
                })),
            )
                .into_response();
        }
    }

    let reset_link = format!(
        "{}/reset-password?token={}",
        email::config::portal_base_url(),
        token
    );

    let email_payload = email::EmailMessage {
        to: email.clone(),
        subject: "Raheja Vistas Documents Portal: Password Setup / Reset".to_string(),
        heading: "Set Up / Reset Your Password".to_string(),
        paragraphs: vec![
            "Click the button below to set up or reset your password.".to_string(),
            "If you didn't request this, you can ignore this email.".to_string(),
        ],
        code: None,
        cta: Some(email::Cta {
            label: "Set Password".to_string(),
            url: reset_link,
        }),
    };

    // Fire-and-forget: an email delivery failure shouldn't fail the request.
    if let Err(e) = email::send_email(&email_payload).await {
        eprintln!("Email send error: {}", e);
    }

    (
        StatusCode::OK,
        Json(ForgotPasswordResponse {
            status: "success".to_string(),
            message: "Password setup/reset link generated successfully".to_string(),
        }),
    )
        .into_response()
}

#[derive(serde::Deserialize)]
pub struct ResetPasswordPayload {
    token: String,
    password: String,
}

pub async fn reset_password(
    State(state): State<AppState>,
    Json(payload): Json<ResetPasswordPayload>,
) -> impl IntoResponse {
    match state
        .permissions_manager
        .reset_password_with_token(&payload.token, &payload.password)
        .await
    {
        Ok(true) => (
            StatusCode::OK,
            Json(serde_json::json!({
                "status": "success",
                "message": "Password updated successfully"
            })),
        )
            .into_response(),
        Ok(false) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "status": "error",
                "message": "Invalid or expired password reset token"
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "status": "error",
                "message": format!("Reset failed: {}", e)
            })),
        )
            .into_response(),
    }
}

pub async fn profile(
    State(state): State<AppState>,
    crate::extractors::SessionUser(session_user): crate::extractors::SessionUser,
) -> impl IntoResponse {
    match state
        .permissions_manager
        .get_user_by_id(session_user.user_id)
        .await
    {
        Ok(Some(user)) => (StatusCode::OK, Json(user)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "User not found").into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
            .into_response(),
    }
}

#[derive(serde::Serialize)]
pub struct MfaSettingsResponse {
    mfa_enabled: bool,
    /// Always `false` for now — reserved for a future forced-MFA rule.
    forced: bool,
}

/// POST /api/v1/profile/mfa
pub async fn get_mfa_settings(
    State(state): State<AppState>,
    crate::extractors::SessionUser(user): crate::extractors::SessionUser,
) -> impl IntoResponse {
    match state
        .permissions_manager
        .get_mfa_enabled(user.user_id)
        .await
    {
        Ok(mfa_enabled) => (
            StatusCode::OK,
            Json(MfaSettingsResponse {
                mfa_enabled,
                forced: false, // super_admin-forced MFA is disabled for now
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct SetMfaEnabledPayload {
    enabled: bool,
}

/// POST /api/v1/profile/mfa/toggle
/// Self-service only — every user controls their own MFA setting.
pub async fn set_mfa_settings(
    State(state): State<AppState>,
    crate::extractors::SessionUser(user): crate::extractors::SessionUser,
    Json(payload): Json<SetMfaEnabledPayload>,
) -> impl IntoResponse {
    match state
        .permissions_manager
        .set_mfa_enabled(user.user_id, payload.enabled)
        .await
    {
        Ok(()) => (
            StatusCode::OK,
            Json(MfaSettingsResponse {
                mfa_enabled: payload.enabled,
                forced: false, // super_admin-forced MFA is disabled for now
            }),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
            .into_response(),
    }
}

/// POST /api/v1/profile/notifications
pub async fn get_notification_settings(
    State(state): State<AppState>,
    crate::extractors::SessionUser(user): crate::extractors::SessionUser,
) -> impl IntoResponse {
    match state
        .permissions_manager
        .get_notification_prefs(user.user_id)
        .await
    {
        Ok(prefs) => (StatusCode::OK, Json(prefs)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
            .into_response(),
    }
}

/// POST /api/v1/profile/notifications/toggle
pub async fn set_notification_settings(
    State(state): State<AppState>,
    crate::extractors::SessionUser(user): crate::extractors::SessionUser,
    Json(prefs): Json<authentication::NotificationPrefs>,
) -> impl IntoResponse {
    match state
        .permissions_manager
        .set_notification_prefs(user.user_id, prefs)
        .await
    {
        Ok(()) => (StatusCode::OK, Json(prefs)).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Database error: {}", e),
        )
            .into_response(),
    }
}

/// POST /api/v1/profile/signer-info
/// Self-service: the caller's own signer directory entry, if any (`null` if they've
/// never filled it in).
pub async fn get_signer_info(
    State(state): State<AppState>,
    crate::extractors::SessionUser(user): crate::extractors::SessionUser,
) -> impl IntoResponse {
    match state
        .document_manager
        .get_signer_by_user_id(user.user_id)
        .await
    {
        Ok(signer) => (StatusCode::OK, Json(signer)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(e)).into_response(),
    }
}

#[derive(serde::Deserialize)]
pub struct SaveSignerInfoPayload {
    pub name: String,
    pub phone: String,
    pub email: String,
}

/// POST /api/v1/profile/signer-info/save
/// Any designation holder can register; a plain resident with none cannot.
pub async fn save_signer_info(
    State(state): State<AppState>,
    crate::extractors::SessionUser(user): crate::extractors::SessionUser,
    Json(payload): Json<SaveSignerInfoPayload>,
) -> impl IntoResponse {
    if !user.has_any_designation() {
        return (
            StatusCode::FORBIDDEN,
            Json("You must hold a designation to register as a signer".to_string()),
        )
            .into_response();
    }

    match state
        .document_manager
        .upsert_own_signer_info(user.user_id, &payload.name, &payload.phone, &payload.email)
        .await
    {
        Ok(signer) => (StatusCode::OK, Json(signer)).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(e)).into_response(),
    }
}
