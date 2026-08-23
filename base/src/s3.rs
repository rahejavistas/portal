use crate::config::S3Config;
use aws_sdk_s3::{presigning::PresigningConfig, primitives::ByteStream, Client};
use std::time::Duration;

/// Records one S3 call outcome, for Grafana visibility into this external dependency.
fn record_operation_metric(operation: &str, outcome: &str) {
    let meter = opentelemetry::global::meter("s3_accessor");
    let counter = meter.u64_counter("s3_operations_total").init();
    counter.add(
        1,
        &[
            opentelemetry::KeyValue::new("operation", operation.to_string()),
            opentelemetry::KeyValue::new("outcome", outcome.to_string()),
        ],
    );
}

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
    ) -> Result<(), Box<aws_sdk_s3::Error>> {
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

        match &res {
            Ok(_) => record_operation_metric("upload", "success"),
            Err(_) => record_operation_metric("upload", "error"),
        }
        res.map_err(|e| Box::new(aws_sdk_s3::Error::from(e)))?;
        Ok(())
    }

    pub async fn download_file(&self, key: &str) -> Result<ByteStream, Box<aws_sdk_s3::Error>> {
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

        match &resp {
            Ok(_) => record_operation_metric("download", "success"),
            Err(_) => record_operation_metric("download", "error"),
        }
        let resp = resp.map_err(|e| Box::new(aws_sdk_s3::Error::from(e)))?;
        Ok(resp.body)
    }

    pub async fn create_presigned_url(
        &self,
        key: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let presigning_config = PresigningConfig::expires_in(self.presigned_expiry)?;

        let req = match self
            .presigned_client
            .get_object()
            .bucket(&self.bucket_name)
            .key(key)
            .presigned(presigning_config)
            .await
        {
            Ok(req) => {
                record_operation_metric("presign_get", "success");
                req
            }
            Err(e) => {
                record_operation_metric("presign_get", "error");
                return Err(Box::new(e));
            }
        };

        Ok(req.uri().to_string())
    }

    /// Generates a presigned PUT URL for direct client-to-S3 uploads.
    /// The URL is locked to the exact key and content type specified.
    pub async fn create_presigned_put_url(
        &self,
        key: &str,
        content_type: &str,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let presigning_config = PresigningConfig::expires_in(self.presigned_expiry)?;

        let req = match self
            .presigned_client
            .put_object()
            .bucket(&self.bucket_name)
            .key(key)
            .content_type(content_type)
            .presigned(presigning_config)
            .await
        {
            Ok(req) => {
                record_operation_metric("presign_put", "success");
                req
            }
            Err(e) => {
                record_operation_metric("presign_put", "error");
                return Err(Box::new(e));
            }
        };

        Ok(req.uri().to_string())
    }

    /// Checks if an object exists at the given key by performing a HEAD request.
    /// Returns the content length in bytes if found, or None if the key doesn't exist.
    pub async fn head_object(&self, key: &str) -> Result<Option<i64>, Box<dyn std::error::Error>> {
        use aws_sdk_s3::operation::head_object::HeadObjectError;

        match self
            .client
            .head_object()
            .bucket(&self.bucket_name)
            .key(key)
            .send()
            .await
        {
            Ok(output) => {
                record_operation_metric("head", "success");
                Ok(Some(output.content_length().unwrap_or(0)))
            }
            Err(sdk_err) => {
                let service_err = sdk_err.into_service_error();
                match service_err {
                    HeadObjectError::NotFound(_) => {
                        record_operation_metric("head", "not_found");
                        Ok(None)
                    }
                    other => {
                        record_operation_metric("head", "error");
                        Err(Box::new(other) as Box<dyn std::error::Error>)
                    }
                }
            }
        }
    }
}
