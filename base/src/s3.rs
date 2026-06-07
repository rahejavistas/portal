use crate::config::S3Config;
use aws_sdk_s3::{presigning::PresigningConfig, primitives::ByteStream, Client};
use std::time::Duration;

#[derive(Clone)]
pub struct S3Accessor {
    client: Client,
    presigned_client: Client,
    bucket_name: String,
    presigned_expiry: Duration,
}

impl S3Accessor {
    #[allow(deprecated)]
    pub async fn new(config: &S3Config) -> Self {
        // 1. Build the data client (using internal endpoint_url)
        let mut loader =
            aws_config::from_env().region(aws_config::Region::new(config.region.clone()));

        if let Some(endpoint) = &config.endpoint_url {
            loader = loader.endpoint_url(endpoint.clone());
        }

        let sdk_config = loader.load().await;

        let mut s3_config_builder = aws_sdk_s3::config::Builder::from(&sdk_config);
        if config.endpoint_url.is_some() {
            s3_config_builder = s3_config_builder.force_path_style(true);
        }
        let client = Client::from_conf(s3_config_builder.build());

        // 2. Build the presigning client (using public_endpoint_url)
        let public_endpoint = config
            .public_endpoint_url
            .clone()
            .or_else(|| config.endpoint_url.clone());

        let mut presigned_loader =
            aws_config::from_env().region(aws_config::Region::new(config.region.clone()));

        if let Some(endpoint) = &public_endpoint {
            presigned_loader = presigned_loader.endpoint_url(endpoint.clone());
        }

        let sdk_config_presigned = presigned_loader.load().await;

        let mut presigned_builder = aws_sdk_s3::config::Builder::from(&sdk_config_presigned);
        if public_endpoint.is_some() {
            presigned_builder = presigned_builder.force_path_style(true);
        }
        let presigned_client = Client::from_conf(presigned_builder.build());

        let presigned_expiry = config
            .presigned_expiry
            .unwrap_or_else(|| Duration::from_secs(900));

        Self {
            client,
            presigned_client,
            bucket_name: config.bucket_name.clone(),
            presigned_expiry,
        }
    }

    pub async fn upload_file(
        &self,
        key: &str,
        body: ByteStream,
        content_type: &str,
    ) -> Result<(), aws_sdk_s3::Error> {
        let start = std::time::Instant::now();

        let res = self
            .client
            .put_object()
            .bucket(&self.bucket_name)
            .key(key)
            .body(body)
            .content_type(content_type)
            .send()
            .await;

        let duration = start.elapsed().as_secs_f64();
        let meter = opentelemetry::global::meter("s3_accessor");
        let histogram = meter.f64_histogram("s3_operation_duration_seconds").init();

        histogram.record(
            duration,
            &[opentelemetry::KeyValue::new("operation", "upload")],
        );

        res?;
        Ok(())
    }

    pub async fn download_file(&self, key: &str) -> Result<ByteStream, aws_sdk_s3::Error> {
        let start = std::time::Instant::now();
        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket_name)
            .key(key)
            .send()
            .await;

        let duration = start.elapsed().as_secs_f64();
        let meter = opentelemetry::global::meter("s3_accessor");
        let histogram = meter.f64_histogram("s3_operation_duration_seconds").init();
        histogram.record(
            duration,
            &[opentelemetry::KeyValue::new("operation", "download")],
        );

        Ok(resp?.body)
    }

    pub async fn create_presigned_url(
        &self,
        key: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let presigning_config = PresigningConfig::expires_in(self.presigned_expiry)?;

        let req = self
            .presigned_client
            .get_object()
            .bucket(&self.bucket_name)
            .key(key)
            .presigned(presigning_config)
            .await?;

        let meter = opentelemetry::global::meter("s3_accessor");
        let counter = meter.u64_counter("s3_presigned_url_fetches_total").init();
        counter.add(1, &[]);

        Ok(req.uri().to_string())
    }
}
