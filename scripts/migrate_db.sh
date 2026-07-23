#!/usr/bin/env bash
# Runs the infra crate's pending SQL migrations against an arbitrary Postgres
# database — use this for one-off targets like the Supabase instance, since the
# server no longer auto-migrates on startup.
#
# Usage:
#   ./scripts/migrate_db.sh "postgresql://user:password@host:port/dbname"
#   DATABASE_URL="postgresql://user:password@host:port/dbname" ./scripts/migrate_db.sh
set -euo pipefail

DB_URL="${1:-${DATABASE_URL:-}}"

if [ -z "$DB_URL" ]; then
    echo "Usage: $0 <postgres-connection-string>" >&2
    echo "   or: DATABASE_URL=<postgres-connection-string> $0" >&2
    exit 1
fi

cd "$(dirname "$0")/.."

# Parse postgresql://user:password@host:port/dbname into its parts with Python
# (stdlib only, no extra dependency) so we can hand them to infra's --flags.
mapfile -t PARTS < <(python3 - "$DB_URL" <<'PYEOF'
import sys
from urllib.parse import urlparse, unquote

parsed = urlparse(sys.argv[1])
print(parsed.username or "")
print(unquote(parsed.password) if parsed.password else "")
print(parsed.hostname or "")
print(parsed.port or 5432)
print((parsed.path or "/").lstrip("/") or "postgres")
PYEOF
)
DB_USER="${PARTS[0]}"
DB_PASS="${PARTS[1]}"
DB_HOST="${PARTS[2]}"
DB_PORT="${PARTS[3]}"
DB_NAME="${PARTS[4]}"

echo "=== Running migrations against $DB_HOST:$DB_PORT/$DB_NAME ==="

# infra's main.rs reads config_${APP_ENV}.toml first (any values not overridden
# by the flags below come from there), then overrides host/port/user/pass/db
# from the CLI flags — config_local.toml just needs to exist and parse.
APP_ENV="${APP_ENV:-local}" cargo run -p infra -- \
    --host "$DB_HOST" \
    --port "$DB_PORT" \
    --username "$DB_USER" \
    --password "$DB_PASS" \
    --database "$DB_NAME"

echo "=== Migrations complete ==="
