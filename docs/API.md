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
| `process.templates.manage` | Create/edit process templates (the versioned builder) and activate versions |

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
  "can_view_all_processes": false,
  "can_manage_process_templates": false
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
    "processes": [ { "process_id": "uuid", "template_version_id": 1001, "template_name": "Tenant Onboarding", "ownership_id": 5, "flat_label": "A-301", "user_email": "...", "created_at": "...", "steps_total": 9, "steps_completed": 3, "status": "USER_INPUT_REQUIRED" } ],
    "flats": [ { "flat_id": 5, "wing": "A", "flat_number": "301" } ]
  }
  ```
  `status` is one of `COMPLETED`, `ADMIN_REVIEW_REQUIRED`, `USER_INPUT_REQUIRED`. `template_version_id` is the exact pinned version this process started on (see [Process Templates](#process-templates-versioned-admin-authored) below); `template_name` comes from a join and is just for display.

### POST `/api/v1/process/state`
Retrieves or auto-initializes a process. Pass `process_id` to fetch an existing one, or `template_id` (the `process_templates.template_id` of the process *type* to start — always resolves to whichever version is currently active — plus optional `flat_id` if the caller owns multiple flats) to fetch-or-create.
* **Request:** `{ "process_id": "uuid" }` or `{ "template_id": 1, "flat_id": 5, "personal_only": false }`
* **Response (200):**
  ```json
  {
    "process_id": "uuid",
    "template_version_id": 1001,
    "process_name": "Tenant Onboarding",
    "ownership_id": 5,
    "flat_label": "A-301",
    "is_completed": false,
    "next_actions": [ { "template_step_id": 3, "name": "Upload ID Proof", "assigned_to": "user" } ],
    "reviews": [ { "review_id": "uuid", "template_step_id": 2, "admin_email": "admin@...", "is_approved": true, "notes": "Looks good", "created_at": "...", "flagged_step_ids": [] } ],
    "admin_comments": [ { "comment_id": "uuid", "process_id": "uuid", "template_step_id": 2, "admin_email": "admin@...", "comment": "Internal note", "created_at": "..." } ],
    "steps": [ { "template_step_id": 2, "name": "...", "description": "Optional stage description, or null", "status": "OPEN", "is_review_required": true, "fields": [ { "template_field_id": 101, "name": "ID Proof", "type": "document", "filled_by": "user", "is_optional": false, "value": { "text_value": null, "document_id": "uuid", "document_title": "Aadhaar.pdf" }, "history": [] } ], "substeps": [] } ]
  }
  ```
  `status` is one of `LOCKED`, `OPEN`, `PENDING_REVIEW`, `COMPLETED`. `admin_comments` is only populated for callers with `process.reviewer`/`process.view`. `template_id` in the request identifies the process *type*, not a specific version — the resolved, permanently-pinned version is what comes back as `template_version_id`.
* **400** if neither `process_id` nor `template_id` given, the given `template_id` has no active version yet, or the caller owns multiple flats and didn't disambiguate with `flat_id`; **403** if the caller doesn't own the process; **404** if not found.

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

## Process Templates (versioned, admin-authored)

A process template is authored in the admin builder UI and stored in Postgres, not a git-committed file. `process_templates` is the logical template ("process type", e.g. "Tenant Onboarding") — a stable `template_id` plus which version is currently `active`. Every Save appends a new, immutable `process_template_versions` row (own `version_id`) rather than mutating one in place; a version never auto-activates itself — activation is always a separate, explicit action. A process instance is permanently pinned to the one `version_id` it started on for its entire lifetime, so editing/activating a newer version never affects processes already in progress.

`step_id`/`field_id` are assigned by the server on Save (the client posts a tree with no IDs at all) and only need to be unique **within that one version's `definition` JSON** — nothing else relies on them being unique anywhere else, so different versions (even of the same template) may reuse the same numbers freely.

### POST `/api/v1/admin/process-templates/list`
Requires `process.templates.manage`.
* **Response (200):** `Vec<TemplateSummary>`:
  ```json
  [
    {
      "template_id": 1,
      "name": "Tenant Onboarding",
      "active_version_id": 3,
      "versions": [ { "version_id": 3, "version_number": 2, "created_at": "...", "created_by": 12, "created_by_email": "admin@..." }, { "version_id": 1, "version_number": 1, "created_at": "...", "created_by": 12, "created_by_email": "admin@..." } ]
    }
  ]
  ```
  `active_version_id` is `null` until a version has been explicitly activated. `versions` is every version ever saved for that template, newest first.

### POST `/api/v1/admin/process-templates/get-version`
Requires `process.templates.manage`. Loads one version's full tree, for opening it in the builder or previewing it.
* **Request:** `{ "version_id": 3 }`
* **Response (200):** the full `ProcessTemplate` tree (same shape as a saved definition — `name`, `description`, `step_id`, `is_optional`, `is_review_required`, `substeps`, `fields`, recursively).
* **404** if the version doesn't exist.

### POST `/api/v1/admin/process-templates/save`
Requires `process.templates.manage`. The upsert: omitting `template_id` (or `null`) creates a brand-new template; an existing `template_id` appends a new draft version to it. Never auto-activates — a fresh template's first version, and every subsequent edit, sits inactive until a separate Activate call.
* **Request:** a `TemplateDraft` — no IDs anywhere, the server assigns every `step_id`/`field_id`:
  ```json
  {
    "template_id": null,
    "name": "Tenant Onboarding",
    "description": null,
    "is_optional": false,
    "is_review_required": false,
    "substeps": [
      {
        "name": "Stage 1",
        "description": "optional",
        "is_review_required": true,
        "substeps": [
          {
            "name": "Substep 1",
            "fields": [
              { "name": "ID Proof", "type": "document", "is_optional": false, "filled_by": "user" }
            ]
          }
        ]
      }
    ]
  }
  ```
* **Response (200):** `{ "version_id": 4 }` — the newly created version's id.
* **400** on an unknown `template_id`.

### POST `/api/v1/admin/process-templates/activate`
Requires `process.templates.manage`. Makes the given version the one residents start new processes against.
* **Request:** `{ "template_id": 1, "version_id": 4 }`
* **Response (200):** `{ "status": "ok" }`
* **400** if `version_id` doesn't belong to `template_id`.

### POST `/api/v1/process-templates/available`
No special permission — any authenticated user. Resident-facing: populates the "Start Process" picker.
* **Response (200):** `Vec<{ "template_id": 1, "name": "Tenant Onboarding" }>` — only templates that currently have an active version.

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
