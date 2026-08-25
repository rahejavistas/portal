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
- **Admin**: `POST /api/v1/admin/documents/upload-url` → `POST /api/v1/admin/documents/confirm-upload`. Caller supplies an explicit `category`, optional flat mapping, and optional metadata. When mapping to a single flat, the admin UI can select a whole folder instead of one file — each file in it runs through this same three-step flow independently (its own document, its own title derived from the filename), all mapped to that one flat.
- **Process reviewer**: `POST /api/v1/admin/process/upload-url` creates a `RESIDENT_SPECIFIC` document attached to a process step; the flat mapping is derived server-side from the process owner, not supplied by the caller.

## Security constraints enforced

- JWT authentication + authorization on every API call
- File extension whitelist (pdf, doc, docx, xls, xlsx, txt)
- Content-type derived from extension (client-provided value ignored)
- Presigned URL locked to exact S3 key and content-type
- `is_upload_confirmed` gate prevents unconfirmed files from being used in workflows (list/download/process-field endpoints filter on it)
- Server-side file size validation via S3 HEAD check during confirmation (1GB max)

## S3 key layout

`document/mod.rs::derive_s3_key` computes the object key from the document ID alone — category plays no part in it:

```rust
pub fn derive_s3_key(document_id: Uuid) -> String {
    let uuid_str = document_id.to_string();
    let first_char = uuid_str.chars().next().unwrap_or('a').to_ascii_lowercase();
    format!("documents/{}/{}", first_char, uuid_str)
}
```

Every document, regardless of category, lives under `documents/{first hex char of the document_id}/{document_id}` — a flat shard-by-first-character layout purely to avoid dumping every object into one S3 "directory," not a category-based `private`/`public` split.

This is a storage-layout detail only — it does not itself gate access. Read access is enforced at the database query layer (see [User Hierarchy & Data Model](USER_HIERARCHY.md#document-access-resolution)); presigned URLs are always short-lived and issued only after that check passes.
