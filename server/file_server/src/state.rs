use crate::document::DocumentManager;

use crate::middleware::RateLimiters;
use authentication::PermissionsManager;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub permissions_manager: Arc<PermissionsManager>,
    pub document_manager: Arc<DocumentManager>,
    pub jwt_secret: String,
    pub jwt_expiry: std::time::Duration,
    pub forgot_password_expiry: std::time::Duration,
    pub rate_limiters: Arc<RateLimiters>,
}
