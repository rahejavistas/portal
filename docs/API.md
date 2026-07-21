# API Reference

All application endpoints are accessed via **POST** (except `/health`, which uses **GET**). Authenticated endpoints expect `Authorization: Bearer <jwt>`.

There is no blanket admin guard. Every `/api/v1/admin/*` route group is gated by one or more specific permission codes (see [Permissions](#permissions) below) checked independently — a caller only needs to hold at least one permission in a route group's list. The `documents_manage_routes` group (`/admin/documents/*`) has no router-level gate at all; each handler checks per-category access itself (a `documents.all.manage` holder can manage every category, others only the categories a designation grants them `can_manage` on).

`super_admin` is a seeded **designation**, not a permission — it happens to be granted every permission in the seed data, but nothing in code special-cases it.

## Response shape inconsistency

`roles.rs`, `admin.rs`, and `process.rs` mutation handlers return `{"status":"success"|"error","message":"..."}` on both success and failure. `documents.rs` handlers instead return the raw error message as a bare JSON string (`Json<String>`), not wrapped in an object. Parse error bodies accordingly depending on which handler you're calling.

## Permissions

| Code | Used to gate |
|---|---|
| `users.view` | View the resident list |
| `users.manage` | Create/manage residents, revoke flats, bulk upload |
| `users.roles.view` | View which designations users hold |
| `users.roles.manage` | Assign/revoke designations on users |
| `designations.manage` | Create/rename/delete designations, toggle their permissions and category-designation grants |
| `documents.all.manage` | Manage documents/categories in every category, bypassing per-category grants |
| `documents.categories.manage` | Create/rename/delete document categories and subcategories |
| `process.view` | Read-only: view every resident's processes and attached files, no write/review access |
| `process.reviewer` | Approve/reject process steps, upload documents as a reviewer, add internal notes |
| `documents.signatures.sign` | View the signing queue and sign (complete) any item in it |
| `documents.signatures.manage` | Cancel a signature requirement; read requirement status outside the queue |

---

## Authentication & Password APIs

### POST `/api/v1/login`
No auth required.
* **Request:** `{ "email": "user@example.com", "password": "..." }`
* **Response (200):** `{ "status": "success", "user": { ...User }, "token": "<jwt>", "message": null }`
* **403** if the account is locked (too many failed attempts); **401** on invalid credentials.

### POST `/api/v1/forgot-password`
No auth required.
* **Request:** `{ "email": "user@example.com" }`
* **Response (200):** `{ "status": "success", "message": "Password setup/reset link generated successfully" }`
* **404** if the email isn't registered.

### POST `/api/v1/reset-password`
No auth required.
* **Request:** `{ "token": "...", "password": "newpassword" }`
* **Response (200):** `{ "status": "success", "message": "Password updated successfully" }`
* **400** if the token is invalid or expired.

### POST `/api/v1/profile`
Returns the caller's own profile.
* **Response (200):** `User` object (see [User shape](#user-shape) below).

### User shape

Returned by `/api/v1/login`, `/api/v1/profile`, and `/api/v1/admin/users/list`:
```json
{
  "user_id": 12,
  "email": "user@example.com",
  "is_active": true,
  "wing": "A",
  "flat_number": "301",
  "designations": ["committee"],
  "can_manage_roles": false,
  "can_manage_designations": false,
  "can_manage_categories": false,
  "can_manage_users": false,
  "can_view_users": false,
  "can_view_roles": false,
  "can_review_process": false,
  "can_manage_documents": false,
  "can_view_all_processes": false
}
```

---

## Resident Document APIs

See [Document Upload Flow](DOCUMENT_UPLOAD.md) for the presigned-URL upload mechanism and access rules.

### POST `/api/v1/documents`
Lists all non-deleted documents visible to the caller.
* **Response (200):** `Vec<DocumentWithMetadata>` (see [Document shape](#document-shape)).

### POST `/api/v1/documents/:id`
Fetches one document with a presigned download URL.
* **Response (200):** `DocumentWithMetadata`.
* **403** if not visible to the caller; **404** if not found.

### POST `/api/v1/documents/upload-url`
Creates an unconfirmed document record (always category `RESIDENT_SPECIFIC`, linked to every flat the caller actively owns) and returns a presigned S3 PUT URL.
* **Request:** `{ "title": "Sale Agreement", "filename": "sale_agreement.pdf", "content_type": null }`
* **Response (200):** `{ "document_id": "a1b2c3d4-...", "upload_url": "https://..." }`
* **400** if title is empty/too long or the file extension isn't allowed (`pdf doc docx xls xlsx txt jpg jpeg png`); **403** if the caller has no active flat ownership.

### POST `/api/v1/documents/confirm-upload`
Verifies the file exists on S3 (HEAD check, ≤10MB) and marks the document confirmed.
* **Request:** `{ "document_id": "a1b2c3d4-..." }`
* **Response (200):** `{ "status": "confirmed" }`
* **400** if the S3 object is missing or oversized; **403/404** if the document isn't the caller's.

### Document shape

```json
{
  "document": {
    "document_id": "uuid",
    "title": "string",
    "description": "string | null",
    "category": "RESIDENT_SPECIFIC",
    "uploaded_date": "2026-07-19T00:00:00Z",
    "updated_at": "2026-07-19T00:00:00Z",
    "is_deleted": false,
    "is_user_uploaded": true,
    "is_upload_confirmed": true,
    "uploaded_by": 12
  },
  "metadata": {
    "document_id": "uuid",
    "subcategory_id": 3,
    "subcategory_code": "URGENT",
    "subcategory_label": "Urgent",
    "tags": ["maintenance"],
    "document_date": "2026-07-19",
    "valid_from": null,
    "valid_until": null
  },
  "flat": { "wing": "A", "flat_number": "301" },
  "download_url": "https://..."
}
```
`metadata` and `flat` are `null` when not applicable. `subcategory_*` fields inside `metadata` are `null` when the document has no subcategory tag (subcategories are always optional).

---

## Process Workflow APIs

### POST `/api/v1/process/list`
* **Request:** `{ "personal_only": false }` (defaults `false`)
* Callers holding `process.reviewer` or `process.view` see every resident's processes unless `personal_only: true` forces just their own.
* **Response (200):**
  ```json
  {
    "processes": [ { "process_id": "uuid", "template_id": "tenant_onboarding_v1", "ownership_id": 5, "flat_label": "A-301", "user_email": "...", "created_at": "...", "steps_total": 9, "steps_completed": 3, "status": "USER_INPUT_REQUIRED" } ],
    "flats": [ { "flat_id": 5, "wing": "A", "flat_number": "301" } ]
  }
  ```
  `status` is one of `COMPLETED`, `ADMIN_REVIEW_REQUIRED`, `USER_INPUT_REQUIRED`.

### POST `/api/v1/process/state`
Retrieves or auto-initializes a process. Pass `process_id` to fetch an existing one, or `template_id` (+ optional `flat_id` if the caller owns multiple flats) to fetch-or-create.
* **Request:** `{ "process_id": "uuid" }` or `{ "template_id": "tenant_onboarding_v1", "flat_id": 5, "personal_only": false }`
* **Response (200):**
  ```json
  {
    "process_id": "uuid",
    "template_id": "tenant_onboarding_v1",
    "process_name": "Tenant Onboarding",
    "ownership_id": 5,
    "flat_label": "A-301",
    "is_completed": false,
    "next_actions": [ { "template_step_id": 3, "name": "Upload ID Proof", "assigned_to": "user" } ],
    "reviews": [ { "review_id": "uuid", "template_step_id": 2, "admin_email": "admin@...", "is_approved": true, "notes": "Looks good", "created_at": "...", "flagged_step_ids": [] } ],
    "admin_comments": [ { "comment_id": "uuid", "process_id": "uuid", "template_step_id": 2, "admin_email": "admin@...", "comment": "Internal note", "created_at": "..." } ],
    "steps": [ { "template_step_id": 2, "name": "...", "status": "OPEN", "is_review_required": true, "fields": [ { "template_field_id": 101, "name": "ID Proof", "type": "document", "filled_by": "user", "is_optional": false, "value": { "text_value": null, "document_id": "uuid", "document_title": "Aadhaar.pdf" }, "history": [] } ], "substeps": [] } ]
  }
  ```
  `status` is one of `LOCKED`, `OPEN`, `PENDING_REVIEW`, `COMPLETED`. `admin_comments` is only populated for callers with `process.reviewer`/`process.view`.
* **400** if neither `process_id` nor `template_id` given, or the caller owns multiple flats and didn't disambiguate with `flat_id`; **403** if the caller doesn't own the process; **404** if not found.

### POST `/api/v1/process/save-step`
* **Request:**
  ```json
  {
    "process_id": "uuid",
    "template_step_id": 3,
    "fields": [
      { "template_field_id": 101, "document_id": "uuid" },
      { "template_field_id": 102, "text_value": "some text" }
    ]
  }
  ```
* **Response (200):** `{ "status": "success", "message": "Step saved successfully", "step": { "template_step_id": 3, "is_completed": true } }`
* Attached documents must already be confirmed and be `RESIDENT_SPECIFIC`. **403** if the caller doesn't own the process — note `process.view` does **not** grant write access here, only `process.reviewer` does. **400** on validation/state-machine errors.

### POST `/api/v1/process/download-url`
* **Request:** `{ "process_id": "uuid", "document_id": "uuid" }`
* **Response (200):** `{ "download_url": "https://..." }`
* Holders of `process.reviewer`/`process.view` can download any process's attached documents; access is evaluated as the process owner would see it. **403/404** otherwise.

---

## Admin User Management APIs

### POST `/api/v1/admin/users/list`
Requires `users.view`, `users.manage`, `users.roles.manage`, or `users.roles.view`.
* **Response (200):** `Vec<User>`.

### POST `/api/v1/admin/users/create`
Requires `users.manage`.
* **Request:** `{ "email": "new@example.com", "wing": "A", "flat_number": "301" }`
* **Response (201):** `User`.
* **400** on invalid email/flat.

### POST `/api/v1/admin/users/revoke-flat`
Requires `users.manage`.
* **Request:** `{ "user_id": 12, "wing": "A", "flat_number": "301" }`
* **Response (200):** `{ "status": "success", "message": "Flat mapping revoked successfully." }`

### POST `/api/v1/admin/users/upload`
Requires `users.manage`. `multipart/form-data`, file field name `file` — CSV with `Email`, `Wing`, `Flat` columns (case-insensitive). Flat number must be in range and wing one of `A-K`; duplicate flat claims within the file are rejected.
* **Response (200):** `{ "total_rows": 50, "successfully_onboarded": 48, "failed_users": [ { "email": "bad@x.com", "reason": "Invalid flat number" } ] }`

---

## Admin Document APIs

No router-level permission gate — each handler checks whether the caller can manage the document's category (`documents.all.manage` bypasses this; otherwise the caller needs a `can_manage` grant on that specific category via a designation).

### POST `/api/v1/admin/documents/list`
* **Response (200):** `Vec<DocumentWithMetadata>` — every document (including soft-deleted) in categories the caller can manage.

### POST `/api/v1/admin/documents/categories`
* **Response (200):** `Vec<CategoryWithSubcategories>` — categories the caller can manage, each with its subcategories nested in (see [Roles: document categories](#roles-document-categories--subcategories)).

### POST `/api/v1/admin/documents/flat-owners`
Previews who currently owns a flat, so an admin mapping a document to it can see who'll get access before saving.
* **Request:** `{ "wing": "A", "flat_number": "301" }`
* **Response (200):** `{ "emails": ["resident@example.com"] }` — empty array if the flat doesn't exist, has no active owner, or the input is incomplete/invalid. Never errors; this is a soft preview, not a validated write.

### POST `/api/v1/admin/documents/upload-url`
* **Request:**
  ```json
  {
    "title": "Sale Agreement",
    "description": "optional",
    "category": "LEASES",
    "metadata": { "subcategory_code": "RENEWAL", "tags": ["2026"], "document_date": "2026-07-19", "valid_from": null, "valid_until": null },
    "flat": { "wing": "A", "flat_number": "301" },
    "filename": "sale_agreement.pdf",
    "content_type": null
  }
  ```
  `metadata` and `flat` are optional; `flat` is required if `category` is `RESIDENT_SPECIFIC`. `metadata.subcategory_code` must belong to `category` if provided.
* **Response (201):** `{ "document_id": "uuid", "upload_url": "https://..." }`
* **403** if the caller can't manage `category`; **400** on invalid extension, missing flat, or unknown/mismatched subcategory code.

### POST `/api/v1/admin/documents/confirm-upload`
* **Request:** `{ "document_id": "uuid" }`
* **Response (200):** `{ "status": "confirmed" }`

### POST `/api/v1/admin/documents/:id/get`
Fetches any document, including soft-deleted ones.
* **Response (200):** `DocumentWithMetadata`. **403/404** per the manage-category check.

### POST `/api/v1/admin/documents/:id/update`
Updates title/description/category and replaces metadata (upserted if `metadata` is an object, cleared entirely if `metadata` is `null`). Bumps `updated_at`.
* **Request:**
  ```json
  {
    "title": "Sale Agreement (Amended)",
    "description": "optional",
    "category": "LEASES",
    "metadata": { "subcategory_code": "RENEWAL", "tags": ["2026"], "document_date": "2026-07-19", "valid_from": null, "valid_until": null }
  }
  ```
* **Response (200):** `DocumentWithMetadata`.
* **403** if the caller can't manage the document's current category, or (when changing category) the target category; **400** if title is empty/too long, or the category change moves the document into or out of `RESIDENT_SPECIFIC` — that category's visibility also depends on its flat mapping, which this endpoint doesn't touch, so such changes are rejected outright. There is no separate flat-remapping endpoint; a `RESIDENT_SPECIFIC` document's flat mapping is fixed at upload time.

### POST `/api/v1/admin/documents/:id/delete`
Soft-deletes (toggles `is_deleted = true`).
* **Response:** **204 No Content**.

---

## Admin Process Review APIs

Requires `process.reviewer` for all routes in this group.

### POST `/api/v1/admin/process/review`
* **Request:** `{ "process_id": "uuid", "template_step_id": 2, "is_approved": true, "notes": "Looks good", "flagged_step_ids": [4, 5] }`
* **Response (200):** `{ "status": "success", "message": "Review recorded successfully. Step is marked as completed.", "is_completed": true }`
* Emails the resident on completion.

### POST `/api/v1/admin/process/revoke`
Reopens an already-completed/pending step for resident re-submission.
* **Request:** `{ "process_id": "uuid", "template_step_id": 2, "notes": "Please re-upload, blurry scan" }`
* **Response (200):** `{ "status": "success", "message": "Step revoked successfully" }`
* Emails the resident.

### POST `/api/v1/admin/process/upload-url`
Uploads a document as a reviewer, attached to a process. The backend derives the flat mapping from the process's ownership and forces category `RESIDENT_SPECIFIC`.
* **Request:** `{ "process_id": "uuid", "title": "Reviewer note attachment", "filename": "note.pdf", "content_type": "application/pdf" }`
* **Response (201):** `{ "document_id": "uuid", "upload_url": "https://..." }`

### POST `/api/v1/admin/process/add-note`
Adds a standalone internal-only comment on a process step — never shown to the resident.
* **Request:** `{ "process_id": "uuid", "template_step_id": 2, "comment": "Called resident, awaiting callback" }`
* **Response (200):** `{ "status": "success", "message": "Internal note added" }`
* **400** if `comment` is empty.

---

## Document Signatures (e-signature tracking)

Which documents need a signature is declared **in the process template itself** — a document-type field can be flagged `"requires_signature": true`. The moment such a field is saved (via `/api/v1/process/save-step`, resident or admin), a `PENDING` signature requirement is auto-provisioned for that document — there's no separate "flag this document" admin action, and everyone holding `documents.signatures.sign` is emailed that it's ready to sign. The step containing that field cannot complete (or pass review) until the requirement is `COMPLETED`.

Signing is a **shared queue**: anyone holding `documents.signatures.sign` sees every pending item and can sign any of them — first to sign completes it, no per-document assigned recipient list. Signing means uploading the signed file; it always becomes a **brand-new document** (never an in-place overwrite) — the original is untouched and stays visible, and `signed_document_id` on the requirement is the link from original to signed copy. The process field is repointed at the new document automatically.

Uploading the signed file uses the same **presigned upload-url / confirm** pattern as every other document upload in this app — the file goes straight from the client to S3, never through this server. This server runs on Lambda (see `main.rs`'s `is_lambda` branch), which caps request/response payload well below what a signed file can be, so a proxied single-call upload isn't an option here.

### POST `/api/v1/admin/documents/signatures/queue`
Requires `documents.signatures.sign`.
* **Response (200):** `Vec<SignatureRequirement>` — every `PENDING` requirement, oldest first.

### POST `/api/v1/admin/documents/signatures/for-document`
Requires `documents.signatures.sign`, `documents.signatures.manage`, `process.reviewer`, `process.view`, or `documents.all.manage` (any one) — this is the read used by the admin process/document views to show "is this signed yet."
* **Request:** `{ "document_id": "uuid" }`
* **Response (200):** `SignatureRequirement | null` — the active (non-cancelled) requirement for that document, or `null` if it was never flagged.

### POST `/api/v1/admin/documents/signatures/all`
Same permission set as `for-document`. Every requirement regardless of status, so the admin document list can build a document_id ↔ signed_document_id lineage map in one call instead of a lookup per row.
* **Response (200):** `Vec<SignatureRequirement>`.

### POST `/api/v1/admin/documents/signatures/replace-upload-url`
Requires `documents.signatures.sign`. Step (a) of signing — creates a new document to hold the signed file and returns a presigned S3 PUT URL for it.
* **Request:** `{ "document_id": "uuid", "content_type": "application/pdf" }` — `document_id` is the original/unsigned document.
* **Response (200):** `{ "new_document_id": "uuid", "upload_url": "https://..." }`
* **400** if there's no `PENDING` requirement for `document_id`.
* Client then `PUT`s the file bytes directly to `upload_url` (max 1GB, enforced via S3 HEAD-check on confirm).

### POST `/api/v1/admin/documents/signatures/replace-confirm-upload`
Requires `documents.signatures.sign`. Step (b) — confirms the signed file landed on S3 and completes the requirement. Status-guarded so a concurrent second confirm for the same requirement cleanly fails ("first to sign wins").
* **Request:** `{ "document_id": "uuid", "new_document_id": "uuid" }`
* **Response (200):** `SignatureRequirement`, now `COMPLETED` with `signed_document_id`/`signed_by`/`signed_at` set.
* **400** if the requirement is no longer `PENDING`, or the file was never actually uploaded to S3.

### POST `/api/v1/admin/documents/signatures/cancel`
Requires `documents.signatures.manage`. Cancels a `PENDING` requirement without ever signing it — the field keeps pointing at the original document, and the step unblocks immediately (an admin override for a wrongly-flagged upload).
* **Request:** `{ "document_id": "uuid", "notes": "optional" }`
* **Response (200):** `{ "status": "ok" }`
* **400** if there's no `PENDING` requirement for `document_id`.

### `SignatureRequirement` shape
```json
{
  "requirement_id": 1,
  "document_id": "uuid",
  "signed_document_id": "uuid | null",
  "provider": "manual",
  "status": "PENDING | COMPLETED | CANCELLED",
  "signed_by": "user_id | null",
  "signed_at": "timestamp | null",
  "created_at": "timestamp",
  "updated_at": "timestamp",
  "document_title": "Issued NOC Document",
  "document_category": "RESIDENT_SPECIFIC"
}
```
`requirement_id` is a plain serial int (like every other internal id in this schema — `document_id`/`signed_document_id` stay UUID since they reference `documents`). `provider` is always `"manual"` today (no external e-signature vendor is wired up yet — this field exists so one can be added later without a schema change).

---

## Roles: designations

### POST `/api/v1/admin/roles/designations`
Requires `users.roles.manage`, `designations.manage`, or `documents.categories.manage`.
* **Response (200):** `Vec<Designation>` — `{ "designation_id": 3, "code": "committee", "label": "Committee", "is_system": false, "permission_codes": ["documents.categories.manage"] }`.

### POST `/api/v1/admin/roles/permissions`
Same permission gate as above.
* **Response (200):** `Vec<{ "permission_id": 1, "code": "users.view", "label": "..." }>` — the full static catalog.

### POST `/api/v1/admin/roles/designations/assign`
Requires `users.roles.manage`.
* **Request:** `{ "user_id": 12, "designation_code": "committee" }`
* **Response (200):** `{ "status": "success", "message": "Designation assigned." }`
* **403** if targeting yourself or an `is_system` designation (e.g. `super_admin`); **400** if the code is unknown. Best-effort emails the affected user.

### POST `/api/v1/admin/roles/designations/revoke`
Requires `users.roles.manage`. Same request/response/guard shape as `assign`; revoking a designation the user never held is a no-op success.

### POST `/api/v1/admin/roles/designations/create`
Requires `designations.manage`.
* **Request:** `{ "code": "committee", "label": "Committee" }` (code normalized: trim, lowercase, spaces → `_`)
* **Response (201):** `Designation`.
* **400** on duplicate/empty code.

### POST `/api/v1/admin/roles/designations/rename`
Requires `designations.manage`.
* **Request:** `{ "code": "committee", "label": "New Label" }`
* **Response (200):** `{ "status": "success", "message": "..." }`

### POST `/api/v1/admin/roles/designations/delete`
Requires `designations.manage`.
* **Request:** `{ "code": "committee" }`
* **400** if the designation is `is_system` or still assigned to users.

### POST `/api/v1/admin/roles/designations/permissions/toggle`
Requires `designations.manage`.
* **Request:** `{ "designation_code": "committee", "permission_code": "documents.categories.manage", "enabled": true }`
* **403** if the designation is `is_system`, or if the caller currently holds that designation themselves (self-escalation guard — you can never grant/revoke permissions on a designation you hold).

---

## Roles: document categories & subcategories

### POST `/api/v1/admin/roles/document-categories`
Requires `users.roles.manage`, `designations.manage`, or `documents.categories.manage`.
* **Response (200):** `Vec<CategoryWithSubcategories>` — every category, including the two protected system ones (`PUBLIC`, `RESIDENT_SPECIFIC`), each with its subcategories nested in:
  ```json
  [
    {
      "code": "LEASES",
      "label": "Leases",
      "is_public_read": false,
      "is_flat_scoped": false,
      "is_system": false,
      "subcategories": [ { "subcategory_id": 3, "category": "LEASES", "code": "RENEWAL", "label": "Renewal" } ]
    }
  ]
  ```

### POST `/api/v1/admin/roles/document-category-designations`
Same permission gate as above.
* **Response (200):** `Vec<{ "category": "LEASES", "designation_code": "committee", "can_read": true, "can_manage": false }>` — the full category↔designation access matrix.

### POST `/api/v1/admin/roles/document-category-designations/toggle`
Requires `designations.manage`.
* **Request:** `{ "category": "LEASES", "designation_code": "committee", "can_read": true, "can_manage": false }`

### POST `/api/v1/admin/roles/document-categories/create`
Requires `documents.categories.manage`.
* **Request:** `{ "code": "LEASES", "label": "Leases" }` (code normalized: trim, uppercase, spaces → `_`)
* **Response (201):** `{ "code": "LEASES", "label": "Leases", "is_public_read": false, "is_flat_scoped": false, "is_system": false }`

### POST `/api/v1/admin/roles/document-categories/rename`
Requires `documents.categories.manage`. `{ "code": "LEASES", "label": "New Label" }`

### POST `/api/v1/admin/roles/document-categories/delete`
Requires `documents.categories.manage`. `{ "code": "LEASES" }`
* **400** if the category is a protected system category or still has documents in it.

### POST `/api/v1/admin/roles/document-subcategories/create`
Requires `documents.categories.manage`. Subcategories are optional, UI-only tags on a document, scoped to a parent category — they never affect access control.
* **Request:** `{ "category": "LEASES", "code": "RENEWAL", "label": "Renewal" }` (code normalized: trim, uppercase, spaces → `_`; unique per `(category, code)`, the same code is allowed under a different category)
* **Response (201):** `{ "subcategory_id": 3, "category": "LEASES", "code": "RENEWAL", "label": "Renewal" }`
* **400** if `category` doesn't exist or `(category, code)` already exists.

### POST `/api/v1/admin/roles/document-subcategories/rename`
Requires `documents.categories.manage`.
* **Request:** `{ "subcategory_id": 3, "label": "New Label" }`
* There is no delete endpoint — an unused subcategory simply never appears as a filter option; deleting the parent category cascades and removes it, and a document's `subcategory_id` is set to `null` if its subcategory is ever removed some other way.
