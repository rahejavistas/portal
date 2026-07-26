use crate::document::DocumentManager;

use crate::middleware::RateLimiters;
use crate::signature_providers::signyu::SignYuClient;
use authentication::PermissionsManager;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub permissions_manager: Arc<PermissionsManager>,
    pub document_manager: Arc<DocumentManager>,
    pub pg_pool: sqlx::PgPool,
    pub jwt_secret: String,
    pub jwt_expiry: std::time::Duration,
    pub forgot_password_expiry: std::time::Duration,
    pub rate_limiters: Arc<RateLimiters>,
    pub super_admin_email: String,
    /// `None` when no `SIGNYU_API_KEY` is configured — the "signyu" provider option is
    /// then simply unavailable (add-signers rejects it with a clear error).
    pub signyu_client: Option<Arc<SignYuClient>>,
    pub signyu_webhook_secret: Option<String>,
}
