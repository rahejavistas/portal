# Grafana dashboards

Import each file in `dashboards/` into Grafana Cloud: **Dashboards → New → Import → Upload JSON file**, then pick your Prometheus data source when asked.

| File | What it shows |
|---|---|
| `portal-api-health.json` | Request rate, 5xx %, latency percentiles, slowest and busiest routes |
| `portal-dependencies.json` | S3, email and SignYu: call rates, failures, latency |
| `portal-security.json` | 401 / 403 / 404 / 429 rates, sign-in failures, routes being probed |

They read the metrics the server pushes over OTLP (`[telemetry]` in the config file).
