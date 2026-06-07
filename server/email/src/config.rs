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
        let smtp_server = env::var("SMTP_SERVER").ok()?;
        let smtp_port = env::var("SMTP_PORT").ok()?.parse::<u16>().ok()?;
        let smtp_username = env::var("SMTP_USERNAME").ok()?;
        let smtp_password = env::var("SMTP_PASSWORD").ok()?;
        let sender_email = env::var("SMTP_SENDER_EMAIL").ok()?;

        Some(Self {
            smtp_server,
            smtp_port,
            smtp_username,
            smtp_password,
            sender_email,
        })
    }
}
