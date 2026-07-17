# API Reference

All application endpoints are accessed via **POST** (except `/health` which uses **GET**). Authenticated endpoints expect `Authorization: Bearer <jwt>`.

Routes under `/api/v1/admin/*` are protected by `admin_guard`, which requires the `super_admin` designation. That guard is the sole enforcement point for the group — individual handlers do not re-check.

## Authentication & Password APIs

### POST `/api/v1/login`
Authenticates a user and returns a JWT.

### POST `/api/v1/forgot-password`
Generates a secure password setup / reset link for the given email.

### POST `/api/v1/reset-password`
Completes password setup/reset using a valid database token.

### POST `/api/v1/profile`
Returns the authenticated user's profile (email, wing, flat, designations).

## Resident Document APIs

See [Document Upload Flow](DOCUMENT_UPLOAD.md) for the presigned-URL upload mechanism and access rules.

### POST `/api/v1/documents`
Lists visible active documents for the authenticated user.

### POST `/api/v1/documents/:id`
Retrieves a specific document with a presigned download URL.

### POST `/api/v1/documents/upload-url`
Creates an unconfirmed document record and returns a presigned PUT URL for direct S3 upload.
* **Request Body:**
  ```json
  { "title": "Sale Agreement", "filename": "sale_agreement.pdf" }
  ```
* **Response (200 OK):**
  ```json
  { "document_id": "a1b2c3d4-...", "upload_url": "https://s3.amazonaws.com/bucket/key?X-Amz-Signature=..." }
  ```

### POST `/api/v1/documents/confirm-upload`
Verifies the file exists on S3 (HEAD check) and marks the document as confirmed.
* **Request Body:**
  ```json
  { "document_id": "a1b2c3d4-..." }
  ```
* **Response (200 OK):**
  ```json
  { "status": "confirmed" }
  ```

## Process Workflow APIs

### POST `/api/v1/process/list`
Lists all processes belonging to the authenticated user.

### POST `/api/v1/process/state`
Retrieves or initializes a process. Pass `template_id` to create new; `process_id` to fetch existing.
* **Request Body:** `{ "template_id": "tenant_onboarding_v1" }` or `{ "process_id": "uuid-..." }`

### POST `/api/v1/process/save-step`
Saves field values for a process step. Documents must be confirmed before use.
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

### POST `/api/v1/process/download-url`
Returns a presigned download URL for a document within a process.

## Admin User Management APIs

Requires `super_admin`.

### POST `/api/v1/admin/users/list`
Lists all users along with their active designations.

### POST `/api/v1/admin/users/create`
Adds a new resident to the system.

### POST `/api/v1/admin/users/roles`
Sets or clears a user's `committee` designation.
* **Request Body:** `{ "user_id": 12, "target_role": "committee" }` (or `"target_role": null` to clear)

### POST `/api/v1/admin/users/revoke-flat`
Deactivates a user's flat ownership mapping.

### POST `/api/v1/admin/users/upload`
Bulk-creates residents from an uploaded CSV file (multipart form, field name `file`).

## Admin Document APIs

Requires `super_admin`.

### POST `/api/v1/admin/documents/list`
Lists all documents (including soft-deleted ones) with no filters.

### POST `/api/v1/admin/documents/upload-url`
Creates an unconfirmed document, with an explicit `category`, optional flat mapping, and optional metadata, and returns a presigned PUT URL.

### POST `/api/v1/admin/documents/confirm-upload`
Confirms the admin upload landing in S3.

### POST `/api/v1/admin/documents/:id/get`
Fetches details of any specific document (including deleted ones).

### POST `/api/v1/admin/documents/:id/update`
Updates an existing document's properties.

### POST `/api/v1/admin/documents/:id/delete`
Toggles the soft-deleted state of a document.

## Admin Process Review APIs

Requires `super_admin`.

### POST `/api/v1/admin/process/review`
Approves or rejects a process step pending review.
* **Request Body:**
  ```json
  { "process_id": "uuid-...", "template_step_id": 2, "is_approved": true, "notes": "Looks good" }
  ```

### POST `/api/v1/admin/process/revoke`
Reopens an already-completed step for resident re-submission.

### POST `/api/v1/admin/process/upload-url`
Creates an upload intent for a process document uploaded by a reviewer. The backend derives the flat mapping and forces `RESIDENT_SPECIFIC` category.
* **Request Body:**
  ```json
  { "process_id": "uuid-...", "title": "Reviewer note attachment", "filename": "note.pdf", "content_type": "application/pdf" }
  ```

### POST `/api/v1/admin/process/add-note`
Adds a standalone reviewer comment on a process step, not shown to the resident.
