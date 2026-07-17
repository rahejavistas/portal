# Vistas File Server

A document sharing portal with designation-based (RBAC) and ownership-based (ABAC) access control.

## Running the Application

Start the local stack (database, storage, server, and UI) with:

```bash
docker compose up --build
```

Stop with:
```bash
docker compose down -v
```

## Documentation

- [API Reference](docs/API.md) — all HTTP endpoints
- [Document Upload Flow](docs/DOCUMENT_UPLOAD.md) — presigned S3 upload mechanism
- [User Hierarchy & Data Model](docs/USER_HIERARCHY.md) — designations, permissions, and what tables hold what
- [Deployment Guide](DEPLOYMENT.md) — AWS Lambda + Supabase + S3 production setup

## Authentication

Authentication is stateless and uses JWT (JSON Web Tokens).

1. **Login Flow**: The client calls `/api/v1/login` with email and password. On success, the server returns a signed JWT.
2. **Session Persistence**: The client stores the JWT in `localStorage` under `vfs_token` and includes it in subsequent API requests as a bearer token (`Authorization: Bearer <token>`).
3. **Password Setup / Reset**:
   * A super admin adds a new resident.
   * Resident requests a password setup link via `/api/v1/forgot-password`.
   * A single-use 32-character secure token is saved in the database with an expiration.
   * The user clicks the link and submits a new password to `/api/v1/reset-password`, which verifies the token and updates the password hash.
