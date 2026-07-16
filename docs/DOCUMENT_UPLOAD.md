# Document Upload Flow (Presigned URLs)

All document uploads use a secure **direct-to-S3 presigned URL** mechanism. The server never streams file bytes — the client uploads directly to S3.

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

There are two variants of this flow, sharing the same three-step shape:

- **Resident**: `POST /api/v1/documents/upload-url` → `POST /api/v1/documents/confirm-upload`. Category is always `RESIDENT_SPECIFIC`, scoped to the uploader's own flat.
- **Admin**: `POST /api/v1/admin/documents/upload-url` → `POST /api/v1/admin/documents/confirm-upload`. Caller supplies an explicit `category`, optional flat mapping, and optional metadata.
- **Process reviewer**: `POST /api/v1/admin/process/upload-url` creates a `RESIDENT_SPECIFIC` document attached to a process step; the flat mapping is derived server-side from the process owner, not supplied by the caller.

## Security constraints enforced

- JWT authentication + authorization on every API call
- File extension whitelist (pdf, doc, docx, xls, xlsx, txt)
- Content-type derived from extension (client-provided value ignored)
- Presigned URL locked to exact S3 key and content-type
- `is_upload_confirmed` gate prevents unconfirmed files from being used in workflows (list/download/process-field endpoints filter on it)
- Server-side file size validation via S3 HEAD check during confirmation (10MB max)

## S3 key layout

`document/mod.rs::derive_s3_key` computes the object key from category and document ID:

```rust
pub fn derive_s3_key(category: DocumentCategory, document_id: Uuid) -> String {
    let prefix = match category {
        DocumentCategory::ResidentSpecific => "private",
        _ => "public",
    };
    ...
}
```

- `RESIDENT_SPECIFIC` documents live under the `private/` prefix.
- Every other category (`PUBLIC`, `COMMITTEE`, `FINANCE`, `GOVERNANCE`, `OPERATIONAL`) lives under `public/`.

This is a storage-layout detail only — it does not itself gate access. Read access is enforced at the database query layer (see [User Hierarchy & Data Model](USER_HIERARCHY.md#document-access-resolution)); presigned URLs are always short-lived and issued only after that check passes.
