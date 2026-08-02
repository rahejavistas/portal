//! SignYu (https://signyu.com/docs) — pay-per-use Aadhaar eSign. Every request/response
//! is logged to `signature_provider_requests` and recorded as a metric.
//!
//! The webhook contract wasn't fully available when this was written, so
//! `handlers::signatures::signyu_webhook` treats any webhook hit as "go re-poll
//! get_document" rather than trusting the payload body directly.

use super::{DocumentStatusResult, SendDocumentResult, SignerContact, SignerProgress};
use opentelemetry::{global, KeyValue};

pub struct SignYuClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    pg_pool: sqlx::PgPool,
}

/// Records one SignYu call outcome. `status_code` is None for a transport-level
/// failure (timeout, DNS, connection refused).
fn record_response_metric(operation: &str, status_code: Option<i32>) {
    let meter = global::meter("file_server_signyu");
    let counter = meter.u64_counter("signyu_provider_responses_total").init();
    let status_label = status_code
        .map(|c| c.to_string())
        .unwrap_or_else(|| "network_error".to_string());
    counter.add(
        1,
        &[
            KeyValue::new("operation", operation.to_string()),
            KeyValue::new("status_code", status_label),
        ],
    );
}

impl SignYuClient {
    pub fn new(base_url: String, api_key: String, pg_pool: sqlx::PgPool) -> Self {
        Self {
            // reqwest::Client::new() has NO default timeout — an unresponsive SignYu
            // would otherwise hang the calling request (and burn Lambda time) forever.
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .expect("failed to build SignYu HTTP client"),
            base_url,
            api_key,
            pg_pool,
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn log(
        &self,
        requirement_id: Option<i32>,
        direction: &str,
        endpoint: &str,
        method: &str,
        request_body: Option<&serde_json::Value>,
        response_status: Option<i32>,
        response_body: Option<&serde_json::Value>,
        error: Option<&str>,
    ) {
        let _ = sqlx::query(
            "INSERT INTO signature_provider_requests \
                (provider, requirement_id, direction, endpoint, method, request_body, response_status, response_body, error) \
             VALUES ('signyu', $1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(requirement_id)
        .bind(direction)
        .bind(endpoint)
        .bind(method)
        .bind(request_body)
        .bind(response_status)
        .bind(response_body)
        .bind(error)
        .execute(&self.pg_pool)
        .await;
    }

    /// Sends a request, logs it, and returns (status, body) for any HTTP response —
    /// callers decide what counts as success. Only a transport-level failure is Err.
    #[allow(clippy::too_many_arguments)]
    async fn send_logged(
        &self,
        operation: &str,
        method: &str,
        endpoint: &str,
        request: reqwest::RequestBuilder,
        request_body_for_log: Option<&serde_json::Value>,
        requirement_id_for_log: Option<i32>,
    ) -> Result<(i32, serde_json::Value), String> {
        let result = request.send().await;

        let outcome = match result {
            Ok(resp) => {
                let status = resp.status().as_u16() as i32;
                let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
                Ok((status, body))
            }
            Err(e) => Err(e.to_string()),
        };

        record_response_metric(operation, outcome.as_ref().ok().map(|(status, _)| *status));
        self.log(
            requirement_id_for_log,
            "outbound",
            endpoint,
            method,
            request_body_for_log,
            outcome.as_ref().ok().map(|(status, _)| *status),
            outcome.as_ref().ok().map(|(_, body)| body),
            outcome.as_ref().err().map(String::as_str),
        )
        .await;

        outcome.map_err(|e| format!("SignYu {} failed: {}", operation, e))
    }

    /// POST /documents — uploads the PDF and creates a PENDING document at SignYu.
    pub async fn create_document(
        &self,
        pdf_bytes: Vec<u8>,
        name: &str,
        requirement_id_for_log: Option<i32>,
    ) -> Result<String, String> {
        let endpoint = format!("{}/documents", self.base_url);
        let part = reqwest::multipart::Part::bytes(pdf_bytes)
            .file_name("document.pdf")
            .mime_str("application/pdf")
            .map_err(|e| e.to_string())?;
        let form = reqwest::multipart::Form::new()
            .part("file", part)
            .text("name", name.to_string());
        let request = self
            .http
            .post(&endpoint)
            .bearer_auth(&self.api_key)
            .multipart(form);

        let (status, body) = self
            .send_logged(
                "create_document",
                "POST",
                &endpoint,
                request,
                Some(&serde_json::json!({ "name": name })),
                requirement_id_for_log,
            )
            .await?;

        if status != 201 && status != 200 {
            return Err(format!(
                "SignYu create_document returned {}: {}",
                status, body
            ));
        }
        body["documentId"]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| {
                format!(
                    "SignYu create_document response missing documentId: {}",
                    body
                )
            })
    }

    /// POST /documents/{id}/signers — signers are added (and will sign) in array order.
    pub async fn add_signers(
        &self,
        document_id: &str,
        signers: &[SignerContact],
        requirement_id_for_log: Option<i32>,
    ) -> Result<Vec<String>, String> {
        let endpoint = format!("{}/documents/{}/signers", self.base_url, document_id);
        let request_body = serde_json::json!({
            "signers": signers.iter().map(|s| serde_json::json!({
                "name": s.name, "phone": s.phone, "email": s.email
            })).collect::<Vec<_>>()
        });
        let request = self
            .http
            .post(&endpoint)
            .bearer_auth(&self.api_key)
            .json(&request_body);

        let (status, body) = self
            .send_logged(
                "add_signers",
                "POST",
                &endpoint,
                request,
                Some(&request_body),
                requirement_id_for_log,
            )
            .await?;

        if status != 201 && status != 200 {
            return Err(format!("SignYu add_signers returned {}: {}", status, body));
        }
        let signer_ids: Vec<String> = body["signers"]
            .as_array()
            .ok_or_else(|| format!("SignYu add_signers response missing signers[]: {}", body))?
            .iter()
            .filter_map(|s| s["signerId"].as_str().map(str::to_string))
            .collect();
        if signer_ids.len() != signers.len() {
            return Err(format!(
                "SignYu add_signers returned {} signer ids for {} requested signers",
                signer_ids.len(),
                signers.len()
            ));
        }
        Ok(signer_ids)
    }

    /// POST /documents/{id}/send — deducts credits, emails signers, returns each
    /// signUrl so we can also offer "open signing page" in our own UI.
    pub async fn send(
        &self,
        document_id: &str,
        requirement_id_for_log: Option<i32>,
    ) -> Result<std::collections::HashMap<String, String>, String> {
        let endpoint = format!("{}/documents/{}/send", self.base_url, document_id);
        let request = self.http.post(&endpoint).bearer_auth(&self.api_key);

        let (status, body) = self
            .send_logged(
                "send",
                "POST",
                &endpoint,
                request,
                None,
                requirement_id_for_log,
            )
            .await?;

        if status != 200 {
            return Err(format!("SignYu send returned {}: {}", status, body));
        }

        let sign_urls = body["signers"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|s| {
                let signer_id = s["signerId"].as_str()?.to_string();
                let sign_url = s["signUrl"].as_str()?.to_string();
                Some((signer_id, sign_url))
            })
            .collect();
        Ok(sign_urls)
    }

    /// Runs create_document -> add_signers -> send as one pipeline.
    pub async fn create_and_send(
        &self,
        pdf_bytes: Vec<u8>,
        document_name: &str,
        signers: &[SignerContact],
        requirement_id_for_log: Option<i32>,
    ) -> Result<SendDocumentResult, String> {
        let external_document_id = self
            .create_document(pdf_bytes, document_name, requirement_id_for_log)
            .await?;
        let external_signer_ids = self
            .add_signers(&external_document_id, signers, requirement_id_for_log)
            .await?;
        let sign_urls_by_signer_id = self
            .send(&external_document_id, requirement_id_for_log)
            .await?;
        let sign_urls = external_signer_ids
            .iter()
            .map(|id| sign_urls_by_signer_id.get(id).cloned())
            .collect();
        Ok(SendDocumentResult {
            external_document_id,
            external_signer_ids,
            sign_urls,
        })
    }

    /// GET /documents/{id} — full status, including per-signer progress and (once
    /// COMPLETED) a temporary download link for the fully signed PDF.
    pub async fn get_document(
        &self,
        document_id: &str,
        requirement_id_for_log: Option<i32>,
    ) -> Result<DocumentStatusResult, String> {
        let endpoint = format!("{}/documents/{}", self.base_url, document_id);
        let request = self.http.get(&endpoint).bearer_auth(&self.api_key);

        let (status, body) = self
            .send_logged(
                "get_document",
                "GET",
                &endpoint,
                request,
                None,
                requirement_id_for_log,
            )
            .await?;

        if status != 200 {
            return Err(format!("SignYu get_document returned {}: {}", status, body));
        }

        let is_completed = body["status"].as_str() == Some("COMPLETED");
        let download_url = body["downloadUrl"].as_str().map(str::to_string);
        let signer_progress = body["signers"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|s| {
                let external_signer_id = s["signerId"].as_str()?.to_string();
                let has_signed = s["hasSigned"].as_bool().unwrap_or(false);
                Some(SignerProgress {
                    external_signer_id,
                    has_signed,
                })
            })
            .collect();

        Ok(DocumentStatusResult {
            is_completed,
            download_url,
            signer_progress,
        })
    }

    /// Downloads the signed PDF from a presigned S3 URL — not a SignYu API call, so no
    /// bearer auth and not run through send_logged.
    pub async fn download_signed_pdf(&self, download_url: &str) -> Result<Vec<u8>, String> {
        let resp = self
            .http
            .get(download_url)
            .send()
            .await
            .map_err(|e| format!("Failed to download signed PDF: {}", e))?;
        if !resp.status().is_success() {
            return Err(format!(
                "Failed to download signed PDF: HTTP {}",
                resp.status()
            ));
        }
        resp.bytes()
            .await
            .map(|b| b.to_vec())
            .map_err(|e| format!("Failed to read signed PDF bytes: {}", e))
    }
}
