use opentelemetry::{global, KeyValue};
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::metrics::{Aggregation, InstrumentKind};
use opentelemetry_sdk::Resource;
use opentelemetry_semantic_conventions::resource;

/// Every histogram this app records is in seconds; the SDK's default bucket edges assume
/// milliseconds and would put all latencies in the first bucket.
fn aggregation_for(kind: InstrumentKind) -> Aggregation {
    match kind {
        InstrumentKind::Counter
        | InstrumentKind::UpDownCounter
        | InstrumentKind::ObservableCounter
        | InstrumentKind::ObservableUpDownCounter => Aggregation::Sum,
        InstrumentKind::ObservableGauge => Aggregation::LastValue,
        InstrumentKind::Histogram => Aggregation::ExplicitBucketHistogram {
            boundaries: vec![
                0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
            ],
            record_min_max: true,
        },
    }
}

/// Exports metrics over OTLP/HTTP to `endpoint`: the collector running beside the
/// function, which holds the credentials for the real backend. `None` or empty disables
/// export.
pub fn init_telemetry(endpoint: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = endpoint.unwrap_or_default();
    let endpoint = endpoint.trim_end_matches('/').to_string();

    if endpoint.is_empty() {
        return Ok(());
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
                .with_endpoint(format!("{}/v1/metrics", endpoint)),
        )
        .with_resource(resource.clone())
        .with_aggregation_selector(aggregation_for)
        .build()?;

    global::set_meter_provider(metrics);
    Ok(())
}
