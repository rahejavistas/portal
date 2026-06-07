# Infra (Migrations)

This crate contains the database migrations and a CLI tool to execute them against any specific PostgreSQL host.

## Running Migrations

You can run the migrations binary using Cargo. It supports environment variables or command-line arguments to specify the database connection details.

### Using CLI Arguments

```bash
cargo run -p infra -- \
    --host localhost \
    --port 5432 \
    --username postgres \
    --password mysecretpassword \
    --database vistas_db
```

### Using Environment Variables

Alternatively, you can set the standard PostgreSQL environment variables:

```bash
export PGHOST=my.remote.db.host.com
export PGPORT=5432
export PGUSER=admin
export PGPASSWORD=securepassword
export PGDATABASE=production_db

cargo run -p infra
```

### Available Options

- `--host` (or `PGHOST`): The database host (default: `localhost`)
- `--port` (or `PGPORT`): The database port (default: `5432`)
- `--username` (or `PGUSER`): The database user (default: `postgres`)
- `--password` (or `PGPASSWORD`): The database password
- `--database` (or `PGDATABASE`): The database name (default: `postgres`)
