# Vistas File Server

A document sharing portal with role-based access control.

## Running the Application

Start the local stack (database, storage, server, and UI) with:

```bash
docker compose up --build
```

Stop with:
```bash
docker compose down -v
```

---

## Architecture

### Document Upload Flow (Presigned URLs)

All user-facing document uploads use a secure **direct-to-S3 presigned URL** mechanism. The server never streams file bytes — the client uploads directly to S3.

```
┌─────────┐      1. POST /upload-url        ┌──────────┐
│  Client  │ ──────────────────────────────► │  Server  │
│ (Browser)│ ◄────────────────────────────── │  (API)   │
│          │    { document_id, upload_url }   │          │
│          │                                 │          │
│          │      2. PUT (binary)            │          │      ┌──────┐
│          │ ───────────────────────────────────────────────►  │  S3  │
│          │ ◄─────────────────────────────────────────────── │      │
│          │        200 OK                   │          │      └──────┘
│          │                                 │          │
│          │      3. POST /confirm-upload    │          │
│          │ ──────────────────────────────► │          │
│          │ ◄────────────────────────────── │          │
│          │     { status: "confirmed" }     │          │
└─────────┘                                 └──────────┘
```

**Security constraints enforced:**
- JWT authentication + user authorization on every API call
- File extension whitelist (pdf, doc, docx, xls, xlsx, txt)
- Content-type derived from extension (client-provided value ignored)
- Presigned URL locked to exact S3 key and content-type
- `is_upload_confirmed` gate prevents unconfirmed files from being used in workflows
- Server-side file size validation via S3 HEAD check during confirmation (10MB max)

---

## Authentication

Authentication is stateless and uses JWT (JSON Web Tokens).

1. **Login Flow**: The client calls `/api/v1/login` with email and password. On success, the server returns a signed JWT.
2. **Session Persistence**: The client stores the JWT in `localStorage` under `vfs_token` and includes it in subsequent API requests as a bearer token (`Authorization: Bearer <token>`).
3. **Password Setup / Reset**:
   * Admin adds a new resident.
   * Resident requests a password setup link via `/api/v1/forgot-password`.
   * A single-use 32-character secure token is saved in the database with an expiration.
   * The user clicks the link and submits a new password to `/api/v1/reset-password`, which verifies the token and updates the password hash.

---

## Authorization

Role-based access controls are enforced on both the backend database queries and frontend navigation menus:

* **Resident**: Can view `PUBLIC` documents and `PRIVATE` documents explicitly mapped to their specific unit (Wing/Flat).
* **Committee**: Can view `PUBLIC` and `COMMITTEE_ONLY` documents.
* **Admin**: Has full access. Can list all documents/residents, create/update/delete documents, and add/update user roles.

---

## API Reference

All application endpoints are accessed via **POST** (except `/health` which uses **GET**).

<details>
<summary><b>Authentication & Password APIs</b></summary>

### POST `/api/v1/login`
Authenticates a user and returns a JWT.

### POST `/api/v1/forgot-password`
Generates a secure password setup / reset link for the given email.

### POST `/api/v1/reset-password`
Completes password setup/reset using a valid database token.

### POST `/api/v1/profile`
Returns the authenticated user's profile (email, wing, flat, roles).

</details>

<details>
<summary><b>Resident Document APIs</b></summary>

### POST `/api/v1/documents`
Lists visible active documents for the authenticated user.
* **Used by:** Dashboard → Documents tab

### POST `/api/v1/documents/:id`
Retrieves a specific document with a presigned download URL.
* **Used by:** Dashboard → Download button

### POST `/api/v1/documents/upload-url`
Creates an unconfirmed document record and returns a presigned PUT URL for direct S3 upload.
* **Used by:** Dashboard → Upload Document modal
* **Request Body:**
  ```json
  {
    "title": "Sale Agreement",
    "filename": "sale_agreement.pdf"
  }
  ```
* **Response (200 OK):**
  ```json
  {
    "document_id": "a1b2c3d4-...",
    "upload_url": "https://s3.amazonaws.com/bucket/key?X-Amz-Signature=..."
  }
  ```

### POST `/api/v1/documents/confirm-upload`
Verifies the file exists on S3 (HEAD check) and marks the document as confirmed.
* **Used by:** Dashboard → Upload Document modal (after S3 PUT succeeds)
* **Request Body:**
  ```json
  { "document_id": "a1b2c3d4-..." }
  ```
* **Response (200 OK):**
  ```json
  { "status": "confirmed" }
  ```

</details>

<details>
<summary><b>Process Workflow APIs</b></summary>

### POST `/api/v1/process/list`
Lists all processes belonging to the authenticated user.
* **Used by:** Process page → Process list view

### POST `/api/v1/process/state`
Retrieves or initializes a process. Pass `template_id` to create new; `process_id` to fetch existing.
* **Used by:** Process page → Process detail view
* **Request Body:**
  ```json
  { "template_id": "tenant_onboarding_v1" }
  ```
  or
  ```json
  { "process_id": "uuid-..." }
  ```

### POST `/api/v1/process/save-step`
Saves field values for a process step. Documents must be confirmed before use.
* **Used by:** Process page → Save Step Inputs button
* **Request Body:**
  ```json
  {
    "process_id": "uuid-...",
    "template_step_id": 3,
    "fields": [
      { "template_field_id": 101, "document_id": "uuid-..." },
      { "template_field_id": 102, "text_value": "some text" }
    ]
  }
  ```

### POST `/api/v1/process/upload-url`
Creates an unconfirmed document record scoped to a process and returns a presigned PUT URL.
* **Used by:** Process page → Document upload (inline & picker modal)
* **Request Body:**
  ```json
  {
    "process_id": "uuid-...",
    "title": "sale_agreement.pdf",
    "filename": "sale_agreement.pdf"
  }
  ```

### POST `/api/v1/process/confirm-upload`
Verifies S3 upload and confirms the document for use in process steps.
* **Used by:** Process page → After S3 PUT completes
* **Request Body:**
  ```json
  {
    "process_id": "uuid-...",
    "document_id": "uuid-..."
  }
  ```

### POST `/api/v1/process/download-url`
Returns a presigned download URL for a document within a process.
* **Used by:** Process page → Document download links

</details>

<details>
<summary><b>Admin User Management APIs</b></summary>

### POST `/api/v1/admin/users/list`
Lists all residents in the system.

### POST `/api/v1/admin/users/create`
Adds a new resident to the system.

### POST `/api/v1/admin/users/roles`
Updates a resident's access role (Resident, Committee, Admin).

</details>

<details>
<summary><b>Admin Document APIs</b></summary>

### POST `/api/v1/admin/documents/list`
Lists all documents (including soft-deleted ones) with no filters.

### POST `/api/v1/admin/documents/create`
Creates a new document via multipart upload (up to 1GB). Admin-only, server-proxied.
* **Note:** This is the only endpoint that still uses server-side streaming, since admin uploads may be up to 1GB.

### POST `/api/v1/admin/documents/:id/get`
Fetches details of any specific document (including deleted ones).

### POST `/api/v1/admin/documents/:id/update`
Updates an existing document's properties.

### POST `/api/v1/admin/documents/:id/delete`
Toggles the soft-deleted state of a document.

</details>

<details>
<summary><b>Admin Process Review APIs</b></summary>

### POST `/api/v1/admin/process/review`
Approves or rejects a process step that is pending admin review.
* **Request Body:**
  ```json
  {
    "process_id": "uuid-...",
    "template_step_id": 2,
    "is_approved": true,
    "notes": "Looks good"
  }
  ```

### POST `/api/v1/admin/process/revoke`
Reopens an already-completed step for resident re-submission.

</details>

---
