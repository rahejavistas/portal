# Configuration Reference

How this app is configured, and every environment variable it reads, across every
crate. There are two layers: a `config_{APP_ENV}.toml` file for non-secret, mostly
static settings, and environment variables for anything secret or deployment-specific
(which always win — they override whatever's in the TOML file).

## How config loading works (`server/file_server`)

```
APP_ENV=<name>  →  reads ./config_<name>.toml  →  env vars override individual fields
```

- `APP_ENV` defaults to `"local"` if unset, so a plain `cargo run` reads `config_local.toml`.
- The file must exist and parse, or the server panics at startup with the parse error —
  there's no silent fallback to defaults for the file itself.
- Every override below is optional: if the env var isn't set, whatever's in the TOML
  file is used as-is.

This repo currently ships `config_local.toml` (local/docker dev) and `config_prod.toml`
(the Lambda deployment). The `quality` test harness generates its own
`config_db_api_<random>.toml` per test run and points `APP_ENV` at it, so tests never
touch either of the checked-in files.

## `config_<APP_ENV>.toml` fields

```toml
port = 3000
super_admin_email = "superadmin@vistas.com"
jwt_secret = "..."
jwt_expiry = "1h"
forgot_password_expiry = "1h"

[postgres]
host = "localhost"
port = 5432
username = "postgres"
password = "..."
database = "postgres"
max_connections = 5
# acquire_timeout_secs = 10   # optional, defaults to 10

[s3]
region = "ap-south-1"
bucket_name = "..."
endpoint_url = "http://minio:9000"          # optional — only needed for a non-AWS S3 (e.g. MinIO)
public_endpoint_url = "http://localhost:9000" # optional — browser-facing endpoint for presigned URLs, falls back to endpoint_url
presigned_expiry = "15m"                     # optional

[telemetry]
# otlp_endpoint = ""
# otlp_username = ""
# otlp_password = ""

[rate_limit]
enabled = true
ip_per_min = 100
user_per_min = 60
admin_per_min = 300

# Optional — omit this whole section if SignYu isn't configured for this environment.
[signyu]
# api_key, webhook_secret intentionally left out of the file — set via env vars only.
# base_url = "https://signyu.com/api/v1"     # optional, this is the default
```

| Field | Required | Notes |
|---|---|---|
| `port` | yes | Ignored in Lambda (the Function URL owns the port); used for local/docker `axum::Server::bind`. |
| `super_admin_email` | yes | The account auto-seeded (as `super_admin`) on every startup if it doesn't already exist. |
| `jwt_secret` | yes | Signs session JWTs. Treat as a secret even though it can live in the TOML — prefer the `JWT_SECRET` env var in any real deployment. |
| `jwt_expiry` / `forgot_password_expiry` | yes | Human-readable durations (`"1h"`, `"15m"`, etc — parsed by `humantime`). |
| `postgres.*` | yes | See `base::config::PostgresConfig`. `database_url`, if set, is a full connection string that takes priority in the pool builder over the individual host/port/user fields — see `DATABASE_URL` below. |
| `s3.*` | yes | See `base::config::S3Config`. |
| `telemetry.*` | no | All three sub-fields optional; omit them (or the OTLP endpoint) to disable telemetry export entirely. |
| `rate_limit.*` | yes | `enabled = false` disables rate limiting outright (used in `config_prod.toml` and test configs). |
| `signyu.*` | no | The whole `[signyu]` section can be omitted. `api_key` absent (from both the file and env) means the SignYu provider is simply unavailable — `add-signers` with `provider: "signyu"` returns a 400. |

## Environment variable overrides (`server/file_server`)

All of these are read once, at startup, in `AppConfig::load_config()`. Any not set are
left as whatever the TOML file has.

| Variable | Overrides | Notes |
|---|---|---|
| `APP_ENV` | which `config_<name>.toml` is loaded | Defaults to `local`. |
| `JWT_SECRET` | `jwt_secret` | |
| `DATABASE_URL` | `postgres.database_url` | A full `postgres://...` connection string, if you'd rather not set the individual DB fields below. |
| `DATABASE_HOST` | `postgres.host` | |
| `DATABASE_PORT` | `postgres.port` | Ignored (silently) if it doesn't parse as a `u16`. |
| `DATABASE_USERNAME` | `postgres.username` | |
| `DATABASE_PASSWORD` | `postgres.password` | |
| `DATABASE_NAME` | `postgres.database` | |
| `S3_REGION` | `s3.region` | |
| `S3_BUCKET_NAME` | `s3.bucket_name` | |
| `SUPER_ADMIN_EMAIL` | `super_admin_email` | |
| `SIGNYU_API_KEY` | `signyu.api_key` | **Secret.** Setting this is what actually enables the SignYu provider — see `docs/SIGNATURES.md`. |
| `SIGNYU_WEBHOOK_SECRET` | `signyu.webhook_secret` | **Secret.** The HMAC-SHA256 signing secret from the SignYu dashboard (Developers → your webhook endpoint) — required for `POST /webhooks/signyu` to accept anything at all; every inbound webhook's `X-SignSetu-Signature` header is verified against it, and the endpoint rejects everything outright if this isn't set. |
| `SIGNYU_BASE_URL` | `signyu.base_url` | Defaults to the real SignYu API. Override to point the client at a mock server (this is how `quality`/`file_server`'s own SignYu tests run against a local mock instead of the real API). |

## Other environment variables (not part of `AppConfig`)

These aren't read through the config-loading path above — either a different crate
reads them directly, or they come from the AWS Lambda runtime itself.

| Variable | Read by | Purpose |
|---|---|---|
| `AWS_LAMBDA_RUNTIME_API` | `server/file_server/src/main.rs` | Set automatically by AWS inside a real Lambda execution environment — its mere presence is how the app decides to run as `lambda_http::run(...)` instead of binding a local TCP port. Never set this yourself outside Lambda. |
| `AWS_ACCESS_KEY_ID` / `AWS_SECRET_ACCESS_KEY` | AWS SDK (S3 client), not our own code | Standard AWS credential env vars, picked up by the SDK's default credential chain. In Lambda, credentials instead come from the function's IAM role and these aren't needed. Locally/in tests, they point at MinIO's dummy credentials. |
| `SMTP_SERVER`, `SMTP_PORT`, `SMTP_USERNAME`, `SMTP_PASSWORD`, `SMTP_SENDER_EMAIL` | `server/email` (`EmailConfig::from_env`) | All five must be set together or email sending is silently disabled (`from_env()` returns `None` and the app just skips sending, logging "SMTP configuration is missing"). Used for every outbound notification (review results, signature requests, etc). |
| `PORTAL_BASE_URL` | `server/email` (`portal_base_url()`) | Base URL emailed links (password reset, etc.) point at. Falls back to `http://localhost:4000` if unset — fine for local dev, but must be set to the real deployed URL in production or reset links will point at localhost. |
| `APP_ENV` | `infra` (the migration-runner binary) | Same meaning as above — picks which `config_<name>.toml` to read `[postgres]` from before running migrations. `infra` also accepts `--host`/`--port`/`--username`/`--password`/`--database` CLI flags that override the file, which is what `scripts/migrate_db.sh` uses to target an arbitrary database without needing a config file for it. |
| `PORT` | `frontend` (the local static-file dev server, `frontend/src/main.rs`) | Which port to serve the frontend on locally. Unrelated to the API server's own `port` config field. |
| `API_BASE` | `frontend`'s `build_static` binary | The backend base URL baked into the generated `dist/*.html` at build time (e.g. the Lambda Function URL for a production build, or empty for local dev where the frontend and API share an origin). Not read at runtime — this is a build-time substitution only. |

## Secrets checklist

Never commit real values for these — set them as environment variables in whatever
deploys the server (Lambda environment variables, docker-compose `environment:`, CI
secrets, etc), not in a checked-in `config_*.toml`:

- `JWT_SECRET`
- `DATABASE_PASSWORD` (or embed it in `DATABASE_URL`)
- `SIGNYU_API_KEY`
- `SIGNYU_WEBHOOK_SECRET`
- `SMTP_PASSWORD`
- `AWS_SECRET_ACCESS_KEY` (only relevant outside Lambda, where IAM roles are used instead)
