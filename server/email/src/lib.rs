use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{MultiPart, SinglePart, header::ContentType},
    transport::smtp::authentication::Credentials,
};

pub mod config;

use config::EmailConfig;

pub struct Cta {
    pub label: String,
    pub url: String,
}

pub struct Attachment {
    pub filename: String,
    pub content_type: String,
    pub bytes: Vec<u8>,
}

/// A structured email: a heading, plain paragraphs, an optional prominent code (for OTP),
/// and an optional call-to-action button. `send_email` renders this into both a plain-text
/// and an HTML part so every email shares one simple, mobile-friendly look instead of each
/// call site hand-rolling its own body string.
pub struct EmailMessage {
    pub to: String,
    pub subject: String,
    pub heading: String,
    pub paragraphs: Vec<String>,
    pub code: Option<String>,
    pub cta: Option<Cta>,
    pub attachment: Option<Attachment>,
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn render_plain_text(msg: &EmailMessage) -> String {
    let mut out = format!("{}\n\n", msg.heading);
    for p in &msg.paragraphs {
        out.push_str(p);
        out.push_str("\n\n");
    }
    if let Some(code) = &msg.code {
        out.push_str(code);
        out.push_str("\n\n");
    }
    if let Some(cta) = &msg.cta {
        out.push_str(&format!("{}: {}\n", cta.label, cta.url));
    }
    out
}

fn render_html(msg: &EmailMessage) -> String {
    let paragraphs_html: String = msg
        .paragraphs
        .iter()
        .map(|p| format!("<p style=\"margin:0 0 16px;\">{}</p>", html_escape(p)))
        .collect();

    let code_html = msg
        .code
        .as_ref()
        .map(|c| {
            format!(
                "<div style=\"margin:0 0 24px; padding:16px; background:#EFF6FF; border-radius:8px; \
                 text-align:center; font-size:32px; font-weight:700; letter-spacing:0.2em; color:#3B82F6;\">{}</div>",
                html_escape(c)
            )
        })
        .unwrap_or_default();

    let cta_html = msg
        .cta
        .as_ref()
        .map(|cta| {
            format!(
                "<table role=\"presentation\" cellpadding=\"0\" cellspacing=\"0\" style=\"margin:8px 0 0;\"><tr><td style=\"border-radius:6px; background:#3B82F6;\">\
                 <a href=\"{}\" style=\"display:inline-block; padding:12px 28px; font-size:15px; font-weight:700; \
                 color:#FFFFFF; text-decoration:none; border-radius:6px;\">{}</a></td></tr></table>",
                html_escape(&cta.url),
                html_escape(&cta.label)
            )
        })
        .unwrap_or_default();

    format!(
        "<!DOCTYPE html><html><head><meta charset=\"UTF-8\">\
         <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\"></head>\
         <body style=\"margin:0; padding:24px 16px; background:#F3F4F6; font-family:-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif;\">\
         <table role=\"presentation\" cellpadding=\"0\" cellspacing=\"0\" width=\"100%\" style=\"max-width:480px; margin:0 auto;\"><tr><td>\
         <div style=\"text-align:center; margin:0 0 20px;\">\
         <div style=\"font-size:28px; font-weight:800; letter-spacing:-0.03em; text-transform:uppercase; color:#3B82F6;\">Vistas</div>\
         <div style=\"font-size:11px; font-weight:600; letter-spacing:0.15em; text-transform:uppercase; color:#9CA3AF; margin-top:2px;\">Resident Portal</div>\
         </div>\
         <div style=\"background:#FFFFFF; border-radius:12px; padding:32px 28px; border-top:3px solid #3B82F6;\">\
         <h1 style=\"margin:0 0 16px; font-size:20px; font-weight:800; color:#111827;\">{}</h1>\
         <div style=\"font-size:15px; line-height:1.5; color:#374151;\">{}{}{}</div>\
         </div>\
         <p style=\"margin:20px 8px 0; font-size:12px; color:#9CA3AF; text-align:center;\">Raheja Vistas Documents Portal</p>\
         </td></tr></table></body></html>",
        html_escape(&msg.heading),
        paragraphs_html,
        code_html,
        cta_html,
    )
}

pub async fn send_email(msg: &EmailMessage) -> Result<(), String> {
    let config = match EmailConfig::from_env() {
        Some(c) => c,
        None => return Ok(()),
    };

    let plain = render_plain_text(msg);
    let html = render_html(msg);

    let alternative = MultiPart::alternative()
        .singlepart(
            SinglePart::builder()
                .header(ContentType::TEXT_PLAIN)
                .body(plain),
        )
        .singlepart(
            SinglePart::builder()
                .header(ContentType::TEXT_HTML)
                .body(html),
        );

    let body = match &msg.attachment {
        None => alternative,
        Some(attachment) => {
            let content_type = ContentType::parse(&attachment.content_type)
                .map_err(|e| format!("Invalid attachment content type: {}", e))?;
            MultiPart::mixed().multipart(alternative).singlepart(
                lettre::message::Attachment::new(attachment.filename.clone())
                    .body(attachment.bytes.clone(), content_type),
            )
        }
    };

    let email = match Message::builder()
        .from(
            config
                .sender_email
                .parse()
                .map_err(|e| format!("Invalid sender email: {}", e))?,
        )
        .to(msg
            .to
            .parse()
            .map_err(|e| format!("Invalid recipient email: {}", e))?)
        .subject(&msg.subject)
        .multipart(body)
    {
        Ok(email) => email,
        Err(e) => {
            eprintln!("Failed to build email message: {}", e);
            return Ok(());
        }
    };

    let creds = Credentials::new(config.smtp_username, config.smtp_password);

    // starttls_relay, not relay — relay() defaults to implicit TLS (port 465), but we run on 587.
    let mailer: AsyncSmtpTransport<Tokio1Executor> =
        AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&config.smtp_server)
            .map_err(|e| format!("Invalid SMTP server: {}", e))?
            .port(config.smtp_port)
            .credentials(creds)
            .build();

    let meter = opentelemetry::global::meter("email_service");
    let counter = meter.u64_counter("emails_sent_total").init();

    // Bounded so a stuck SMTP connection can never hang the caller's request (and burn
    // Lambda time) indefinitely — applies to every email sent through this one function.
    const SEND_TIMEOUT_SECONDS: u64 = 10;
    let send_result = tokio::time::timeout(
        std::time::Duration::from_secs(SEND_TIMEOUT_SECONDS),
        mailer.send(email),
    )
    .await;

    match send_result {
        Ok(Ok(_)) => {
            counter.add(1, &[opentelemetry::KeyValue::new("status", "success")]);
        }
        Ok(Err(e)) => {
            eprintln!("Could not send email to {}: {:?}", msg.to, e);
            counter.add(1, &[opentelemetry::KeyValue::new("status", "error")]);
        }
        Err(_) => {
            eprintln!(
                "Email send to {} timed out after {SEND_TIMEOUT_SECONDS}s",
                msg.to
            );
            counter.add(1, &[opentelemetry::KeyValue::new("status", "timeout")]);
        }
    }

    Ok(())
}
