use std::env;

#[derive(Debug, Clone)]
pub struct EmailConfig {
    pub smtp_server: String,
    pub smtp_port: u16,
    pub smtp_username: String,
    pub smtp_password: String,
    pub sender_email: String,
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
