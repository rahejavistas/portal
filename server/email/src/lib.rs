use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor, message::header::ContentType,
    transport::smtp::authentication::Credentials,
};
use tracing::{error, info};

pub mod config;

use config::EmailConfig;

/// Enum representing the types of emails that can be sent, carrying their own content dynamically.
/// The caller is responsible for constructing and setting the subject, description, and recipient.
pub enum Email {
    ForgotPassword {
        to: String,
        subject: String,
        body: String,
    },
    Generic {
        to: String,
        subject: String,
        body: String,
    },
}

impl Email {
    pub fn to(&self) -> &str {
        match self {
            Email::ForgotPassword { to, .. } => to,
            Email::Generic { to, .. } => to,
        }
    }

    pub fn subject(&self) -> &str {
        match self {
            Email::ForgotPassword { subject, .. } => subject,
            Email::Generic { subject, .. } => subject,
        }
    }

    pub fn body(&self) -> &str {
        match self {
            Email::ForgotPassword { body, .. } => body,
            Email::Generic { body, .. } => body,
        }
    }
}

/// Generic carrier ("UBER") function to send any Email payload using SMTP configuration.
pub async fn send_email(email_payload: &Email) -> Result<(), String> {
    let config = match EmailConfig::from_env() {
        Some(c) => c,
        None => {
            error!("SMTP configuration is missing in environment variables. Skipping email send.");
            return Ok(()); // Fail gracefully as per instructions
        }
    };

    let to_email = email_payload.to();

    let email = match Message::builder()
        .from(
            config
                .sender_email
                .parse()
                .map_err(|e| format!("Invalid sender email: {}", e))?,
        )
        .to(to_email
            .parse()
            .map_err(|e| format!("Invalid recipient email: {}", e))?)
        .subject(email_payload.subject())
        .header(ContentType::TEXT_PLAIN)
        .body(email_payload.body().to_string())
    {
        Ok(email) => email,
        Err(e) => {
            error!("Failed to build email message: {}", e);
            return Ok(());
        }
    };

    let creds = Credentials::new(config.smtp_username, config.smtp_password);

    let mailer: AsyncSmtpTransport<Tokio1Executor> =
        AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp_server)
            .map_err(|e| format!("Invalid SMTP server: {}", e))?
            .port(config.smtp_port)
            .credentials(creds)
            .build();

    let meter = opentelemetry::global::meter("email_service");
    let counter = meter.u64_counter("emails_sent_total").init();

    match mailer.send(email).await {
        Ok(_) => {
            info!("Email sent successfully to {}", to_email);
            counter.add(1, &[opentelemetry::KeyValue::new("status", "success")]);
        }
        Err(e) => {
            error!("Could not send email to {}: {:?}", to_email, e);
            counter.add(1, &[opentelemetry::KeyValue::new("status", "error")]);
        }
    }

    Ok(())
}
