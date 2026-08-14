use std::env;

#[derive(Clone)]
pub struct EmailConfig {
    pub smtp_server: String,
    pub smtp_port: u16,
    pub smtp_username: String,
    pub smtp_password: String,
    pub sender_email: String,
}

impl std::fmt::Debug for EmailConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmailConfig")
            .field("smtp_server", &self.smtp_server)
            .field("smtp_port", &self.smtp_port)
            .field("smtp_username", &self.smtp_username)
            .field("smtp_password", &"[redacted]")
            .field("sender_email", &self.sender_email)
            .finish()
    }
}

impl EmailConfig {
    /// Loads the EmailConfig from environment variables.
    /// Returns None if any required configuration is missing, ensuring no weak fallback defaults.
    pub fn from_env() -> Option<Self> {
        // A set-but-empty var (e.g. unresolved `${SMTP_SERVER}` in compose) must not count as configured.
        let non_empty = |key: &str| env::var(key).ok().filter(|v| !v.is_empty());

        let smtp_server = non_empty("SMTP_SERVER")?;
        let smtp_port = non_empty("SMTP_PORT")?.parse::<u16>().ok()?;
        let smtp_username = non_empty("SMTP_USERNAME")?;
        let smtp_password = non_empty("SMTP_PASSWORD")?;
        let sender_email = non_empty("SMTP_SENDER_EMAIL")?;

        Some(Self {
            smtp_server,
            smtp_port,
            smtp_username,
            smtp_password,
            sender_email,
        })
    }
}

/// Base URL emailed links point at (e.g. `https://portal.example.com`). Separate from
/// `EmailConfig` since it's not part of the SMTP on/off gate — falls back to the local
/// dev server rather than disabling email entirely when unset.
pub fn portal_base_url() -> String {
    env::var("PORTAL_BASE_URL")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "http://localhost:4000".to_string())
}
