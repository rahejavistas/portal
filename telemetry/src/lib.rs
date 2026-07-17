use base64::{engine::general_purpose, Engine as _};
use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::Resource;
use opentelemetry_semantic_conventions::resource;
use std::collections::HashMap;

pub fn init_telemetry(
    endpoint: Option<String>,
    username: Option<String>,
    password: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = endpoint.unwrap_or_default();
    let username = username.unwrap_or_default();
    let password = password.unwrap_or_default();

    if endpoint.is_empty() {
        return Ok(());
    }

    let mut headers = HashMap::new();
    if !username.is_empty() && !password.is_empty() {
        let auth = general_purpose::STANDARD.encode(format!("{}:{}", username, password));
        headers.insert("Authorization".to_string(), format!("Basic {}", auth));
    }

    let resource = Resource::new(vec![
        KeyValue::new(resource::SERVICE_NAME, "vistas_file_server"),
        KeyValue::new(resource::SERVICE_VERSION, "0.1.0"),
    ]);

    // Metrics pipeline
    let metrics = opentelemetry_otlp::new_pipeline()
        .metrics(opentelemetry_sdk::runtime::Tokio)
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .http()
                .with_endpoint(format!("{}/v1/metrics", endpoint))
                .with_headers(headers.clone()),
        )
        .with_resource(resource.clone())
        .build()?;

    global::set_meter_provider(metrics);
    Ok(())
}
