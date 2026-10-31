# Production Deployment and Setup Guide

This document outlines the step-by-step instructions to configure and deploy the Rust backend server to **AWS Lambda** integrated with **Supabase PostgreSQL** and **Amazon S3**.

---

## Architecture Overview

```mermaid
graph TD
    Client[Browser / Frontend] -->|HTTPS| LambdaURL[AWS Lambda Function URL]
    LambdaURL -->|Execution| Lambda[Rust Backend Binary]
    Lambda -->|Port 5432 / Connection Pool| Supabase[(Supabase DB)]
    Lambda -->|Presigned URLs / Direct API| S3[(Amazon S3 Bucket)]
    Client -->|Direct Upload/Download| S3
```

* **Backend:** Compiled for `arm64` Graviton processors using `cargo-lambda` and run serverlessly.
* **Database:** Connected using port **`5432`** (Session Mode or Direct) to support SQLx prepared statements.
* **Storage:** Amazon S3 bucket with client-side presigned URLs for scalable and cost-effective file transfers.

---

## 1. Build & Package the Application

The application must be cross-compiled for the AWS Lambda target (`arm64` Linux) and bundled with its production configuration file.

### Prerequisites
Make sure you have `cargo-lambda` and `zig` (used for linking) installed on your development machine.

On Fedora:
```bash
sudo dnf install zig
cargo install cargo-lambda
```

On Ubuntu / Debian:
```bash
sudo apt install zig
cargo install cargo-lambda
```

### Build Command
From the root of the repository, execute:
```bash
cargo lambda build --release --arm64 --output-format zip --include config_prod.toml
```

This will compile the Rust project and output a deployment bundle at:
**`target/lambda/file_server/bootstrap.zip`**

---

## 2. AWS Lambda Function Setup

### Step 2.1: Create the Function
1. Open the **AWS Lambda Console** and click **Create function**.
2. Select **Author from scratch**.
3. Configure the basic information:
   * **Function name:** E.g., `vistas-file-server-backend`
   * **Runtime:** Select **Provide your own bootstrap on Amazon Linux 2023**
   * **Architecture:** Select **arm64** (matches the Graviton cross-compilation target)
4. Under **Advanced settings**:
   * Check **Enable function URL**.
   * Set **Auth type** to **NONE** (Authorization is handled in-code via JWT tokens).
   * Check **Configure CORS** to allow cross-origin requests from your frontend.
5. Click **Create function**.

### Step 2.2: Upload Code & Set Environment Variables
1. Under the **Code** tab, click **Upload from** ➔ **.zip file** and upload the generated `target/lambda/file_server/bootstrap.zip`.
2. Select the **Configuration** tab and go to **Environment variables**. Add the following variables:

| Key | Value | Description |
| :--- | :--- | :--- |
| `APP_ENV` | `prod` | Instructs the binary to load `config_prod.toml` |
| `DATABASE_URL` | `postgres://postgres.[REF]:[PASS]@aws-1-ap-south-1.pooler.supabase.com:5432/postgres` | **Must use Port 5432** (see Database warning below) |
| `JWT_SECRET` | `[your-secure-signing-key]` | Used for signing user authorization tokens |
| `S3_BUCKET_NAME` | `266359798742-test` | Name of your storage bucket |
| `S3_REGION` | `ap-south-1` | AWS Region of your bucket |
| `ADMIN_EMAIL` | `admin@vistas.com` | Primary admin email |

### Step 2.3: Metrics to Grafana Cloud via the Collector Layer

The server never talks to Grafana. It sends metrics to a collector running next to it as a Lambda extension (`localhost:4318`), and the collector forwards them to Grafana after the response has gone out. Grafana credentials live only in the collector's environment, not in the zip or the app config.

1. Build with the collector config included:
   ```bash
   cargo lambda build --release --arm64 --output-format zip --include config_prod.toml --include collector.yaml
   ```
2. Add the AWS-managed **ADOT collector layer for arm64** in `ap-south-1` to the function. Take the ARN for the current version from the AWS ADOT Lambda documentation. Use the collector-only layer, not a language layer.
3. Add these environment variables:

| Key | Value | Description |
| :--- | :--- | :--- |
| `OPENTELEMETRY_COLLECTOR_CONFIG_URI` | `/var/task/collector.yaml` | Tells the collector which config to load |
| `GRAFANA_OTLP_AUTH` | `Basic <base64 of instanceID:token>` | Grafana Cloud credentials |

4. `config_prod.toml` has `otlp_endpoint = "http://localhost:4318"` under `[telemetry]` and no credentials. Without the layer attached nothing receives the metrics, so add the layer before deploying this build.

---

## 3. S3 Bucket & CORS Setup

### Step 3.1: CORS Configuration
To allow the frontend browser to upload and view documents via presigned S3 URLs, you must configure CORS on your bucket:

1. Open the **Amazon S3 Console** and click on your bucket **`266359798742-test`**.
2. Go to the **Permissions** tab.
3. Scroll down to **Cross-origin resource sharing (CORS)** and click **Edit**.
4. Paste the following configuration:

```json
[
    {
        "AllowedHeaders": [
            "*"
        ],
        "AllowedMethods": [
            "GET",
            "PUT",
            "POST",
            "DELETE",
            "HEAD"
        ],
        "AllowedOrigins": [
            "https://portal-015.pages.dev"
        ],
        "ExposeHeaders": [
            "ETag"
        ],
        "MaxAgeSeconds": 3000
    }
]
```
5. Click **Save changes**.

---

## 4. Lambda IAM Execution Role Permissions

Your Lambda function needs permissions to read/write/delete objects in S3. 

1. In the Lambda Console, navigate to your function.
2. Select **Configuration** ➔ **Permissions**.
3. Under **Execution role**, click the link under **Role name** to open IAM.
4. Under the **Permissions** tab, click **Add permissions** ➔ **Create inline policy**.
5. Switch to the **JSON** editor and paste this policy:

```json
{
    "Version": "2012-10-17",
    "Statement": [
        {
            "Sid": "S3AccessForBackend",
            "Effect": "Allow",
            "Action": [
                "s3:PutObject",
                "s3:GetObject",
                "s3:DeleteObject",
                "s3:ListBucket"
            ],
            "Resource": [
                "arn:aws:s3:::266359798742-test",
                "arn:aws:s3:::266359798742-test/*"
            ]
        }
    ]
}
```
6. Click **Next**, name the policy `vistas-s3-access`, and click **Create policy**.

---

## ⚠️ Important Production Warnings

### Database Connection Mode & Port `5432`
* **Do not use Port `6543` (Transaction Mode):** SQLx is fundamentally incompatible with Supabase/PgBouncer transaction mode because it ignores `.persistent(false)` and always attempts to send protocol-level named prepared statement commands (`sqlx_s_X`). This raises `prepared statement already exists` (error `42P05`) errors.
* **Connection Limits:** Set `max_connections` inside your database configuration (or pool parameters) to a small number (e.g. `2` or `3`). Because Lambda scales horizontally, keeping the pool limit small per container prevents the function from exhausting Supabase's total connection limits under high concurrent traffic.
