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
| `accounting.view` | View accounting/billing data (read-only) |
| `accounting.manage` | Manage accounting/billing — invoices, payments, ledger, settings |

There is no signatures-specific permission at all. Signing eligibility (self-registering
as a signer on the Profile page, viewing/acting on your own signing queue) is gated on
**holding any real designation** — `authentication::User::has_any_designation()` — not
a grantable permission; a plain resident with no designation can't register.
`super_admin` doesn't count on its own: it's an abstract "holds every permission" role,
not a real person realistically picked as a document signer, so a bare super_admin
account (no other designation) is not sign-eligible either. Signer directory oversight
(viewing everyone/deactivating — there is no delete, only deactivation) and full
signature visibility are covered by `process.reviewer`, since that's already who
attaches signers to documents in the first place.

---

## Authentication & Password APIs

### POST `/api/v1/login`
No auth required.
* **Request:** `{ "email": "user@example.com", "password": "..." }`
* **Response (200), MFA off:** `{ "status": "success", "user": { ...User }, "token": "<jwt>", "message": null }`
* **Response (200), MFA on for this account:** `{ "status": "mfa_required", "user": null, "token": null, "mfa_required": true, "pending_token": "<opaque>" }` — a 6-digit code has been emailed; no session token is issued yet. Finish via `/api/v1/mfa/verify` below.
* **403** if the account is locked (too many failed attempts); **401** on invalid credentials.

### Email OTP MFA
MFA is fully **opt-in per account** (see [Account Settings](#account-settings-mfa--notification-preferences) below) — there is currently no forced-MFA rule for any designation, including `super_admin`. All three endpoints below are unauthenticated (no `Authorization` header) since the caller doesn't have a session token yet — they're keyed entirely off the opaque `pending_token` issued by `/login`.

### POST `/api/v1/mfa/verify`
No auth required.
* **Request:** `{ "pending_token": "<opaque>", "code": "123456" }`
* **Response (200):** `{ "status": "success", "user": { ...User }, "token": "<jwt>", "message": null }` — same shape as a normal successful login.
* **400** `{ "mfa_required": false, "message": "..." }` if the code is wrong, expired, or the challenge was already used/replaced by a newer login — the client should fall back to the login form. Wrong-code attempts are capped at 5 before the whole challenge is dead (`TooManyAttempts`); a fresh login is then required to get a new code.

### POST `/api/v1/mfa/resend`
No auth required.
* **Request:** `{ "pending_token": "<opaque>" }`
* **Response (200):** `{ "status": "success", "message": "A new code has been sent." }` — issues a fresh 6-digit code against the same `pending_token`, resets the wrong-attempt counter.
* **403** once a single login's challenge has been resent 3 times — this cap exists specifically so a leaked `pending_token` can't be used to indefinitely reset the 5-wrong-attempt counter and grind through unlimited guesses. A fresh login is required past that point.

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

## Account Settings: MFA & notification preferences

Both live in a single `user_settings` row per user (auto-created on first write, defaults apply if the row doesn't exist yet): `mfa_enabled` defaults **off**, `notify_process_updates`/`notify_signature_events` default **on**. Every notification email that targets a specific known recipient (process approve/reject/revoke, "your turn to sign"/"document fully signed") checks the recipient's own preference before sending — an unreadable/missing row is treated as "send" (fail open), never as silent suppression of a real error. The one exception is the admin-pending-review email, which goes to a single fixed `super_admin_email` config value with no per-user row to check against, so it's unconditional.

### POST `/api/v1/profile/mfa`
Any authenticated user. Returns the caller's own MFA setting.
* **Response (200):** `{ "mfa_enabled": false, "forced": false }` — `forced` is currently always `false` (no designation is ever forced into MFA; see the login section above).

### POST `/api/v1/profile/mfa/toggle`
Any authenticated user. Self-service only — always the session user's own setting.
* **Request:** `{ "enabled": true }`
* **Response (200):** `{ "mfa_enabled": true, "forced": false }`

### POST `/api/v1/profile/notifications`
Any authenticated user. Returns the caller's own notification preferences.
* **Response (200):** `{ "notify_process_updates": true, "notify_signature_events": true }`

### POST `/api/v1/profile/notifications/toggle`
Any authenticated user. Self-service only.
* **Request:** `{ "notify_process_updates": true, "notify_signature_events": false }` — both fields required, always sets both together.
* **Response (200):** `{ "notify_process_updates": true, "notify_signature_events": false }`

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
    "uploaded_by": 12,
    "content_type": "application/pdf"
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
`metadata` and `flat` are `null` when not applicable. `subcategory_*` fields inside `metadata` are `null` when the document has no subcategory tag (subcategories are always optional). `content_type` is captured once at upload time from the file extension and never changes afterward — editing a document's title/description/category never touches it, and a document is never overwritten in place (every "replacement", e.g. a new signature-chain copy, is a brand-new row with its own `content_type`). Documents uploaded before this field existed have `content_type: null`.

---

## Process Workflow APIs

### POST `/api/v1/process/list`
* **Request:** `{ "personal_only": false }` (defaults `false`)
* Callers holding `process.reviewer` or `process.view` see every resident's processes unless `personal_only: true` forces just their own.
* **Response (200):**
  ```json
  {
    "processes": [ { "process_id": "uuid", "template_version_id": 1001, "template_name": "Tenant Onboarding", "ownership_id": 5, "flat_label": "A-301", "user_email": "...", "created_at": "...", "updated_at": "...", "steps_total": 9, "steps_completed": 3, "status": "USER_INPUT_REQUIRED", "tat_working_days": 3, "sla_due_date": "2026-08-28", "days_running": 2, "effective_due_date": "2026-08-28", "is_sla_paused": false, "is_overdue": false } ],
    "flats": [ { "flat_id": 5, "wing": "A", "flat_number": "301" } ]
  }
  ```
  `status` is one of `COMPLETED`, `ADMIN_REVIEW_REQUIRED`, `USER_INPUT_REQUIRED`. `template_version_id` is the exact pinned version this process started on (see [Process Templates](#process-templates-versioned-admin-authored) below); `template_name` comes from a join and is just for display.
  `days_running`/`effective_due_date`/`is_sla_paused`/`is_overdue` only mean anything when the template has `tat_working_days` set (see [TAT/SLA tracking](#tatsla-tracking) below); otherwise `sla_due_date`/`effective_due_date` are `null` and `days_running` is just calendar days since `created_at`, frozen once the process completes rather than growing forever.

### POST `/api/v1/process/state`
Retrieves or auto-initializes a process. Pass `process_id` to fetch an existing one, or `template_id` (the `process_templates.template_id` of the process *type* to start — always resolves to whichever version is currently active — plus optional `flat_id` if the caller owns multiple flats) to fetch-or-create.

Fetching by `process_id` is allowed if the caller: owns the process; holds `process.reviewer`/`process.view` (unless `personal_only` is set); holds a designation that owns at least one step in that process's template; OR currently/previously has a signature requirement (any status) attached to some document within that exact process — this is the narrow read-only grant that lets a signer view the one process their signature concerns without giving them `process.reviewer`'s blanket visibility into every resident's processes (it does not affect `admin_comments` visibility, which still requires `process.reviewer`/`process.view` — a designation holder never sees these, even for a process they can otherwise act on). Otherwise **403**.
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
    "audit_log": [ { "audit_id": 1, "template_step_id": 2, "event_type": "STEP_APPROVED", "detail": null, "actor_email": "admin@...", "created_at": "..." } ],
    "effective_due_date": "2026-08-28",
    "is_sla_paused": false,
    "tat_actual_working_days": null,
    "steps": [ { "template_step_id": 2, "name": "...", "description": "Optional stage description, or null", "status": "OPEN", "filled_by": { "kind": "designation", "code": "secretary" }, "filled_by_designation_label": "Secretary", "fields": [ { "template_field_id": 101, "name": "ID Proof", "type": "document", "is_optional": false, "requires_signature": false, "required_signs_from_designations": [], "value": { "text_value": null, "document_id": "uuid", "document_title": "Aadhaar.pdf", "document_content_type": "application/pdf" }, "history": [], "signature_status": [] } ], "substeps": [] } ]
  }
  ```
  `status` is one of `LOCKED`, `OPEN`, `PENDING_REVIEW`, `COMPLETED`. `admin_comments` and `audit_log` are only populated for callers with `process.reviewer`/`process.view` — `audit_log` is the append-only status/verification/return trail (`event_type` is one of `STEP_COMPLETED`, `STEP_APPROVED`, `STEP_RETURNED`, `STEP_FLAGGED`, `PROCESS_CLOSED`), and each entry carries `acted_as_designation` (non-null only when a `Designation`-owned stage produced it); a field's own edit history is on each field's `history` array instead, and now carries `actor_email` (who saved that value) and `acted_as_designation` alongside it. Each step's `filled_by` mirrors the template's (`{"kind":"user"|"admin"}` or `{"kind":"designation","code":...}`), with `filled_by_designation_label` resolved server-side for the `designation` case so the frontend never needs its own lookup. `next_actions[].assigned_to` is `"user"`, `"admin"`, or a designation code. `tat_actual_working_days` is `null` until the process has actually closed. `template_id` in the request identifies the process *type*, not a specific version — the resolved, permanently-pinned version is what comes back as `template_version_id`.
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
* Attached documents must already be confirmed and be `RESIDENT_SPECIFIC`. **403** unless the caller owns the process, holds `process.reviewer`, or holds a designation that owns `template_step_id` — note `process.view` does **not** grant write access here. Loading the process this way only gets the caller in the door; the targeted step's own `filled_by` is still separately checked (a resident can only save their own `User`-owned steps, a designation holder only their own `Designation`-owned steps). **400** on validation/state-machine errors.

### POST `/api/v1/process/download-url`
* **Request:** `{ "process_id": "uuid", "document_id": "uuid" }`
* **Response (200):** `{ "download_url": "https://..." }`
* Holders of `process.reviewer`/`process.view`, and a designation holder for a process their designation owns a step in, can download a process's attached documents — but only documents actually attached to *that* process, verified via its field history, not a blanket category grant. No document-category permission is required for this. **403/404** otherwise.

### POST `/api/v1/process/audit-log-pdf`
Requires `process.reviewer` or `process.view`.
* **Request:** `{ "process_id": "uuid" }`
* **Response (200):** a rendered PDF (audit trail status timeline + field-change history), returned the same way as other generated PDFs in this app — see the accounting PDF endpoints for the response shape convention.
* **403/404** under the same ownership rules as `/api/v1/process/state`.

---

## TAT/SLA tracking

Not its own API surface — it's a set of fields threaded through `/api/v1/process/list` and `/api/v1/process/state` above, plus the admin-only endpoints under [Process Templates](#process-templates-versioned-admin-authored) for configuring it. Summary:

- Opt-in per template version, via `tat_working_days` on the definition. `null`/absent means no TAT tracking for that template at all.
- The clock (`sla_started_at`/`sla_due_date`) starts once, at the process's first full submission — not at process creation.
- An admin rejecting a step pauses the clock (`sla_paused_at`); the resident resubmitting resumes it, folding the paused time into `sla_paused_working_days`, which shifts `effective_due_date` forward by that many working days (weekends and rows in `public_holidays` are skipped).
- None of this gates anything — a process isn't blocked, locked, or auto-escalated for being overdue. It is purely for display (`is_overdue`) and reporting (`tat_actual_working_days`, `/api/v1/admin/process-templates/tat-report`).

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

### POST `/api/v1/admin/users/create-staff`
Requires `users.manage`. Creates an account with no flat ownership, auto-assigned the `staff` designation.
* **Request:** `{ "email": "staff@example.com" }`
* **Response (201):** `User`.
* **400** if `email` is empty.

### POST `/api/v1/admin/users/revoke-flat`
Requires `users.manage`.
* **Request:** `{ "user_id": 12, "wing": "A", "flat_number": "301" }`
* **Response (200):** `{ "status": "success", "message": "Flat mapping revoked successfully." }`

### POST `/api/v1/admin/users/upload`
Requires `users.manage`. `multipart/form-data`, file field name `file` — CSV with `Email`, `Wing`, `Flat` columns (case-insensitive). Flat number must be in range and wing one of `A-K`; duplicate flat claims within the file are rejected.
* **Response (200):** `{ "total_rows": 50, "successfully_onboarded": 48, "failed_users": [ { "email": "bad@x.com", "reason": "Invalid flat number" } ] }`

---

## Admin Flats APIs

Managing the wing/floor/flat catalog itself (as opposed to who owns a flat, which is the User Management APIs above). Reachable in the UI only from Society Profile → Flat Setup, `super_admin`-only.

### POST `/api/v1/admin/flats/list`
Requires `users.view`, `users.manage`, `users.roles.manage`, or `users.roles.view`.
* **Response (200):** `[ { "flat_id": 5, "wing": "A", "flat_number": "0301", "area_sqft": "850.00", "is_fire_flat": false, "owner_email": "resident@..." } ]` — `owner_email` is `null` for a vacant flat.

### POST `/api/v1/admin/flats/create`
* **Request:** `{ "wing": "A", "flat_number": "0301", "area_sqft": "850.00" }` (`area_sqft` optional, must be positive if given)
* **Response (200):** the created `FlatSummary`. Safe to call again for the same `(wing, flat_number)` — it returns the existing flat rather than erroring, but won't change its `area_sqft` on a repeat call.

### POST `/api/v1/admin/flats/bulk-create`
* **Request:** `{ "wing": "A", "flat_numbers": ["0201", "0202", "0203"] }`
* **Response (200):** `{ "created": 3 }` — the count actually inserted; flat numbers that already exist for that wing are silently skipped, not double-created.

### POST `/api/v1/admin/flats/set-fire-flag`
* **Request:** `{ "flat_id": 5, "is_fire_flat": true }`
* **Response (200):** `{ "status": "ok" }`. A fire flat is excluded from the Add Resident flat picker and from bulk floor/wing removal, but otherwise behaves like any other flat.

### POST `/api/v1/admin/flats/delete`
* **Request:** `{ "flat_id": 5 }`
* **Response (200):** `{ "status": "ok" }`.
* **400** `"This flat is occupied."` if it has an active owner, or a foreign-key-violation message if it has billing/document/process records attached — delete the dependent records (or revoke the owner) first.

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
* **403** if the caller can't manage the document's current category, or (when changing category) the target category; **400** if title is empty/too long, or the category change moves the document into or out of `RESIDENT_SPECIFIC` — that category's visibility also depends on its flat mapping, which this endpoint doesn't touch, so such changes are rejected outright. Use [`/admin/documents/bulk-update-category`](#post-apiv1admindocumentsbulk-update-category) instead (it works on a single ID too) if the category needs to move into or out of `RESIDENT_SPECIFIC`.

### POST `/api/v1/admin/documents/:id/delete`
Soft-deletes (toggles `is_deleted = true`).
* **Response:** **204 No Content**.

### POST `/api/v1/admin/documents/bulk-delete`
Soft-deletes every requested document the caller can manage; the rest are reported as skipped instead of failing the whole request.
* **Request:** `{ "document_ids": ["uuid", ...] }`
* **Response (200):** `{ "succeeded": ["uuid", ...], "skipped": [{ "document_id": "uuid", "reason": "..." }, ...] }`
* **400** if `document_ids` is empty. A document that's missing or in a category the caller can't manage lands in `skipped`, not a 403 — permission scoping is per document, not per request.

### POST `/api/v1/admin/documents/bulk-update-subcategory`
Sets (or, if `subcategory_code` is `null`, clears) the subcategory on every requested document the caller can manage. Each document's code is resolved against its own category, so a selection spanning categories works — a document whose category doesn't offer that code, or that has no metadata row at all, is skipped rather than rejecting the batch.
* **Request:** `{ "document_ids": ["uuid", ...], "subcategory_code": "RENEWAL" }`
* **Response (200):** same `{ succeeded, skipped }` shape as bulk-delete.
* **400** if `document_ids` is empty.

### POST `/api/v1/admin/documents/bulk-update-category`
Moves every requested document the caller can manage into `category`, clearing its subcategory and any existing flat mapping first. If `category` is `RESIDENT_SPECIFIC`, `flat` is required and applied to every document in the batch — this is the only way to move a document into or out of `RESIDENT_SPECIFIC` (the single-document update endpoint above rejects that).
* **Request:** `{ "document_ids": ["uuid", ...], "category": "RESIDENT_SPECIFIC", "flat": { "wing": "A", "flat_number": "301" } }` (`flat` omitted/`null` for a non-`RESIDENT_SPECIFIC` target)
* **Response (200):** same `{ succeeded, skipped }` shape as bulk-delete.
* **403** if the caller can't manage `category`; **400** if `document_ids` is empty, `category` is `RESIDENT_SPECIFIC` with no `flat`, or `flat` doesn't match a real flat — an invalid `flat` fails the whole batch (unlike the per-document permission skips above), since it's one value applied to every document in the request.

---

## Admin Process Review APIs

Not gated by a blanket permission at the router level. Each route requires either
`process.reviewer` globally, or — for `/review`, `/revoke`, `/add-note`, `/upload-url`,
`/confirm-upload` — holding the designation that owns the specific step being acted on
(checked against that process's own template on every request; see the `filled_by`
scoping rules under [`/admin/process-templates/save`](#post-apiv1adminprocess-templatessave)).
A caller with neither gets **403**.

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

### POST `/api/v1/admin/process/confirm-upload`
Confirms an upload started via the endpoint above. **Deliberately separate from** [`/admin/documents/confirm-upload`](#post-apiv1admindocumentsconfirm-upload) — that one requires `documents.all.manage` or a category-level `can_manage` grant, which a plain `process.reviewer` won't have. This endpoint only re-checks that the caller can review the given `process_id` (same as starting the upload) — **no document-category grant is required or checked at all.** This is intentional: a reviewer's ability to upload/confirm/download documents through the process-review flow is scoped to the specific process they're authorized to review, never to a whole document category. Granting a role broad `RESIDENT_SPECIFIC` category access (via Roles → Category Access) is a *separate*, wider capability — it only matters for the general [Admin Document APIs](#admin-document-apis) library, not for anything in this section.
* **Request:** `{ "process_id": "uuid", "document_id": "uuid" }`
* **Response (200):** `{ "status": "confirmed" }`

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
* **Response (200):** the full `ProcessTemplate` tree (same shape as a saved definition — `name`, `description`, `step_id`, `is_optional`, `filled_by`, `substeps`, `fields`, recursively).
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
    "substeps": [
      {
        "name": "Stage 1",
        "description": "optional",
        "filled_by": { "kind": "user" },
        "substeps": [
          {
            "name": "Substep 1",
            "fields": [
              { "name": "ID Proof", "type": "document", "is_optional": false },
              { "name": "NOC Document", "type": "document", "is_optional": false, "requires_signature": true, "required_signs_from_designations": ["staff", "secretary"] }
            ]
          }
        ]
      }
    ]
  }
  ```
  `filled_by` is set once per top-level Stage (`{"kind":"user"}`, `{"kind":"admin"}`,
  `{"kind":"designation","code":"secretary"}`, or `{"kind":"wing_representative"}`) and
  denormalized onto every substep beneath it at save time — a field no longer carries its
  own `filled_by`; every field in a stage shares that stage's owner. `User` always
  requires a reviewer's approval to complete; `Admin`/`Designation`/`WingRepresentative`
  never do — filling the stage completes it immediately. A `Designation`-owned stage is
  exclusive to that designation's active holders and requires no permission grant at all:
  holding the designation is itself enough to load, fill, review, and reopen that stage
  on any resident's process, scoped to only the step(s) that designation owns — nothing
  broader. Holding `process.reviewer` globally, even as a super admin, does not grant
  access to a `Designation`-owned stage the caller doesn't hold. The check is
  re-evaluated live on every request, so assigning or revoking the designation takes
  effect on the holder's very next call, no caching involved. Every write this scoping
  governs (`process_fields`, `process_step_reviews`, `process_audit_log`) snapshots which
  designation authorized it, surfaced back as `acted_as_designation` on the corresponding
  response entries. When a `Designation`-owned stage transitions to `Open`, every active
  holder of that designation is emailed; `Admin`-owned stages get no such email since
  there's no single owner to notify.

  `WingRepresentative` is the same as `Designation` ownership, except the designation
  itself is resolved per-process from the resident-owner's own wing (`flats.wing` →
  `wings.representative_designation_id` → `designations.code`), not fixed in the
  template. Every wing gets its own representative designation auto-created the moment
  its first flat is created (labeled "Wing Representative `<wing>`", no manual setup
  needed) — this is why `flats.wing` is a foreign key into a `wings` table rather than a
  free-text column.

  A document field's `requires_signature` (default `false`) does not itself declare who
  must sign, or provision anything — signature requirements are still always attached
  ad hoc by a process reviewer via [`/admin/process/documents/add-signers`](#post-apiv1adminprocessdocumentsadd-signers).
  It's purely a completion gate: once a document is submitted to a `requires_signature`
  field, that field is NOT considered filled — and the step cannot complete — until a
  reviewer has attached at least one signer AND every attached signer has signed. This
  stops a reviewer from silently completing the step without ever routing the document
  for signature.

  `required_signs_from_designations` (default `[]`, order-significant) scopes and orders who
  can be attached as a signer for this field — it does not provision anything (a
  reviewer still explicitly attaches signers), but unlike `requires_signature` it IS
  enforced server-side, not just in the picker UI:
  - **On attach** (`add-signers`, below): every listed `signer_id` must currently hold
    at least one of the field's designations, AND the array order must be consistent
    with the designations' order in the field — i.e. non-decreasing designation rank
    across `signer_ids` (ties, e.g. two signers who both cover the same designation,
    may appear in either relative order). Violating either rule is a **400**.
  - **On signing** (`replace-upload-url`/`replace-confirm-upload`, below): re-checked
    at the moment of signing, not just at attach time — if the signer's designation was
    revoked in between, the requirement is effectively invalidated: signing is rejected
    with **403** and the requirement stays `PENDING`. Since there's no cancel endpoint,
    this requirement is now permanently stuck until the signer's designation is
    restored — this is a known consequence of "no cancellation" and worth designing
    templates/designation changes around.

  Its array order is also what the "Add Signers" picker UI uses to derive `sign_order`
  client-side before submitting: it groups eligible signers by which listed designation
  they hold and assigns `sign_order` by that position (not by click order) — e.g.
  `["staff", "secretary"]` means whichever signer covers `staff` always signs before
  whichever covers `secretary`. With an empty list, `sign_order` is plain click order
  and no server-side designation check applies, as before. `SignerDirectoryEntry`
  responses from [`/admin/signers/list`](#post-apiv1adminsignerslist) include
  `designation_codes` (that signer's own currently-held, active designation codes) so
  the frontend can do this filtering/grouping, and so the server can re-check it at
  attach/sign time.
* **Response (200):** `{ "version_id": 4 }` — the newly created version's id.
* **400** on an unknown `template_id`.

### POST `/api/v1/admin/process-templates/activate`
Requires `process.templates.manage`. Makes the given version the one residents start new processes against.
* **Request:** `{ "template_id": 1, "version_id": 4 }`
* **Response (200):** `{ "status": "ok" }`
* **400** if `version_id` doesn't belong to `template_id`.

### POST `/api/v1/admin/process-templates/deactivate`
Requires `process.templates.manage`. Clears the active version, so `/api/v1/process-templates/available` no longer lists it and no resident can start a new process against it — any process already in progress on it keeps working, unaffected.
* **Request:** `{ "template_id": 1 }`
* **Response (200):** `{ "status": "ok" }`
* **400** for an unknown `template_id`.

### POST `/api/v1/admin/process-templates/holidays/list`
Requires `process.templates.manage`.
* **Response (200):** `[ { "holiday_date": "2026-01-26", "description": "Republic Day" } ]` — the shared calendar excluded from every template's TAT working-day math, alongside weekends.

### POST `/api/v1/admin/process-templates/holidays/add`
* **Request:** `{ "holiday_date": "2026-01-26", "description": "Republic Day" }`
* **Response (200):** `{ "status": "ok" }`

### POST `/api/v1/admin/process-templates/holidays/remove`
* **Request:** `{ "holiday_date": "2026-01-26" }`
* **Response (200):** `{ "status": "ok" }`

### POST `/api/v1/admin/process-templates/tat-report`
Requires `process.templates.manage`.
* **Response (200):** `[ { "template_name": "Tenant Onboarding", "closed_count": 12, "average_tat_actual_working_days": 4.3 } ]` — one row per template, only over processes that have actually closed with TAT tracking on. Backend-only right now; nothing in the admin UI surfaces this yet.

### POST `/api/v1/process-templates/available`
No special permission — any authenticated user. Resident-facing: populates the "Start Process" picker.
* **Response (200):** `Vec<{ "template_id": 1, "name": "Tenant Onboarding" }>` — only templates that currently have an active version.

### Signature status on process fields

Signature requirements are never declared in the template — a process reviewer attaches signers to a document from the Process Review screen (see `process/documents/add-signers` below). `process/state` and `process/save-step` field responses expose the result as `signature_status: [{ requirement_id, signer_name, sign_order, status }]` on each `document`-type field, sorted by `sign_order`. While any entry is `PENDING`, the field does not count as filled and the step cannot complete — residents see a generic "Submitted — awaiting signature" placeholder instead of the document until every signer has signed.

---

## Signer Directory

There's no signatures-specific permission — eligibility to register as a signer is
**holding any real designation** (`authentication::User::has_any_designation()`); a
plain resident holding zero designations can't, and neither can a bare `super_admin`
account with no other designation (it's an abstract admin role, not counted on its
own). An eligible user self-registers their
own contact info (name/phone/digits-only phone/email) from their **Profile page**;
that's the only way a `document_signers` row is created (never admin-created on
someone else's behalf, and never admin-*edited* either — a reviewer or even
super_admin cannot change another person's registered contact info). Nobody but the
signer themselves can even **view** their own phone/email either — the admin-facing
`SignerDirectoryEntry` shape (below) deliberately omits both; a reviewer only ever
sees name + login email, enough to pick the right person without browsing their
private contact details. Signer directory oversight (viewing everyone incl. inactive,
deactivating — there is no delete, only deactivation) and full signature visibility
are covered by `process.reviewer`, since that's already who attaches signers to
documents in the first place.

### POST `/api/v1/profile/signer-info`
Any authenticated user. Returns the caller's own `Signer` row (full shape, incl. phone/email), or `null` if they've never registered.

### POST `/api/v1/profile/signer-info/save`
Any authenticated user, but rejected with **403** unless the caller holds at least one designation. Self-service upsert of the caller's own signer contact info — always the SESSION user's own row, never another user's.
* **Request:** `{ "name": "...", "phone": "9876543210", "email": "..." }` — `phone` must be digits only.
* **Response (200):** the upserted `Signer`.

### POST `/api/v1/admin/signers/list`
Requires `process.reviewer`.
* **Request:** `{}` (or no body) — active signers only, the "Add Signers" picker source. `{ "include_inactive": true }` — every signer regardless of `is_active`, for the directory oversight view.
* **Response (200):** `Vec<SignerDirectoryEntry>` — `{ signer_id, user_id, name, is_active, created_at, updated_at, updated_by, user_email, designation_codes }`. No `phone`/`email` — see the note above. `designation_codes` is that signer's own currently-held, active designation codes — used client-side to filter/order the "Add Signers" picker against a field's `required_signs_from_designations` (see [Process Templates](#post-apiv1adminprocess-templatessave)).

### POST `/api/v1/admin/signers/set-active`
Requires `process.reviewer`. There is no delete endpoint — a signer is only ever deactivated, never removed. Deactivating never touches anything already in progress: a `PENDING` requirement already waiting on a deactivated signer is completely unaffected, and they can still complete it — deactivation only stops them from being offered for *new* attachments going forward. Reversible any time by setting `is_active: true` again.
* **Request:** `{ "signer_id": 3, "is_active": false }`

---

## Document Signatures

A `SignatureRequirement` row tracks ONE (document, signer) pair: `requirement_id`, `document_id`, `signed_document_id`, `process_id`, `template_step_id`, `template_field_id`, `signer_id`, `sign_order`, `provider` (`"manual"` or an external provider name), `status` (`PENDING` or `COMPLETED` — there is no `CANCELLED`, see below), `signed_by`, `signed_at`, plus joined `document_title`/`document_category`/`signer_user_id`/`signer_name`/`signer_email`/`signer_phone`. For a `signyu`-provider row, it also carries `external_document_id`/`external_signer_id` (SignYu's own ids, joined in from `signyu_requirement_links`) and `sign_url` — SignYu's own signing-page link for that signer, so the UI can offer "Open Signing Page" directly instead of the signer having to find SignYu's email. Signing never mutates the original document — it uploads (or, for an external provider, downloads back) a brand-new one and points `signed_document_id` at it.

**Signing order**: `sign_order` is assigned in the order signers were attached (1-based). A requirement is only actionable once every lower-`sign_order` requirement for the same document has been signed (`COMPLETED`) — earlier signers must finish before later ones can act.

**No cancellation**: once a signer is attached, that requirement exists until it's signed — there is no endpoint to cancel or remove it. A required signature is required, full stop. This means a reviewer must get `required_signs_from_designations` and the signer selection right at attach time; there's no undo for a mis-attached signer short of the signer actually signing it.

### POST `/api/v1/admin/process/documents/add-signers`
Requires `process.reviewer` globally, or the designation that owns `template_step_id`. Attaches one or more signers (by `signer_id`, from the active signer directory) to a document already submitted into a process field, **in the given array order**. Idempotent per (document, signer).
* **Request:**
  ```json
  {
    "process_id": "<uuid>", "template_step_id": 8, "template_field_id": 112,
    "document_id": "<uuid>", "signer_ids": [3, 7],
    "provider": "manual"
  }
  ```
  `provider` defaults to `"manual"`. `"signyu"` sends the document out via SignYu instead (see below) — rejected with **400** if no `SIGNYU_API_KEY` is configured, or if the document's `content_type` isn't `application/pdf` (SignYu only accepts PDFs; the UI hides the SignYu option for non-PDF documents, but this is enforced server-side regardless of what the client sends).

  If the target field declares `required_signs_from_designations`, `signer_ids` is validated
  against it (**400** on failure): every signer must currently hold one of the listed
  designations, and their designations' order must be non-decreasing across the array
  (see [Process Templates](#post-apiv1adminprocess-templatessave)).
* **Response (200):** `{ "status": "success", "requirements": [SignatureRequirement, ...] }` — only the newly-created rows (already-attached signers are skipped).
* For `provider: "manual"`, each newly attached signer gets our own email notification. For `"signyu"`, SignYu emails its own signing links directly (no double notification).

### POST `/api/v1/admin/documents/signatures/queue`
Any authenticated user — no permission gate; the response is self-scoped, so nothing else is needed. `PENDING` requirements where the caller IS the signer AND it's currently their turn (see sign_order above).
* **Response (200):** `Vec<SignatureRequirement>`.

### POST `/api/v1/admin/documents/signatures/all`
Requires `process.reviewer`, `process.view`, or `documents.all.manage`. The request body is optional:
* **Request:** `{}` (or no body) — every requirement regardless of status, used to build the document-list signature lineage (original ↔ signed copy). `{ "document_id": "<uuid>" }` — scoped to just that document's requirements, in `sign_order`.
* **Response (200):** `Vec<SignatureRequirement>`.

### POST `/api/v1/admin/documents/signatures/provider-status`
Any authenticated user — no permission gate.
* **Response (200):** `{ "signyu_available": true }` — whether `SIGNYU_API_KEY` is configured. Lets the "Add Signers" UI hide the SignYu option entirely rather than offering a choice that will 400; the UI additionally only shows it when the target document's `content_type` is `application/pdf` (see above).

### POST `/api/v1/admin/documents/signatures/document-download-url`
No permission gate — the caller must BE the requirement's own signer, and only while it's still `PENDING` (**403** otherwise). Lets the signer view what they're about to sign before doing so: the original document for the first signer in the chain, otherwise the previous signer's already-signed copy.
* **Request:** `{ "requirement_id": 4 }`
* **Response (200):** `DocumentWithMetadata` (includes a presigned `download_url`).

### POST `/api/v1/admin/documents/signatures/replace-upload-url`
No permission gate — the caller must BE the requirement's own signer (**403** otherwise, checked in the handler). Nobody else, not even `process.reviewer` or super_admin, can act on someone else's requirement — signing is a legal act tied to a specific person. Generates a presigned S3 PUT URL for the signed replacement file. **400** if it isn't this signer's turn yet. **403** if the field declares `required_signs_from_designations` and the signer no longer currently holds one of them (re-checked live, not just at attach time — a designation revoked after attach invalidates the pending requirement).
* **Request:** `{ "requirement_id": 4, "content_type": "application/pdf" }`
* **Response (200):** `{ "new_document_id": "<uuid>", "upload_url": "..." }`

### POST `/api/v1/admin/documents/signatures/replace-confirm-upload`
Same rules as above (re-checked again here too). Marks the requirement `COMPLETED`. Only once EVERY attached signer for the underlying document has signed does the process field actually get repointed at a signed copy (the last signer's) — signing while others are still outstanding just satisfies that one signer's row and keeps the step blocked.
* **Request:** `{ "requirement_id": 4, "new_document_id": "<uuid>" }`
* **Response (200):** the updated `SignatureRequirement`.

### POST `/api/v1/admin/documents/signatures/refresh-status`
Same permission set as `/all`. Manual fallback for the "Refresh Status" button — polls the external provider instead of waiting for its webhook. No-op (`reconciled: false`) for `manual`-provider documents.
* **Request:** `{ "document_id": "<uuid>" }`
* **Response (200):** `{ "status": "ok", "reconciled": true|false }`

### POST `/webhooks/signyu`
Public (no session auth) — SignYu fires this on two events, `signer.signed` and `document.completed`. **Requires `SIGNYU_WEBHOOK_SECRET` to be configured**; a webhook is rejected outright (not just ignored) if:
* no secret is configured at all (**500** — unverifiable input is untrusted input),
* the `X-SignSetu-Signature` header is missing (**403**),
* or the signature doesn't verify (**403**).

The signature is `sha256=<hex>`, an HMAC-SHA256 of the **exact raw request body bytes** (not a re-serialized object) keyed with the webhook's signing secret from the SignYu dashboard — verified via `hmac::Mac::verify_slice` (constant-time). Once verified, we still don't trust the payload's own `status`/`signer`/`signers` fields as authoritative — we only read `documentId` and use it as a signal to re-poll SignYu's `GET /documents/{id}` (the exact same path `/documents/signatures/refresh-status` above uses), so there is only ever one code path that actually writes signature completion state. This also makes the handler naturally idempotent against SignYu's documented retry behavior (a non-2xx response or 10s timeout gets retried with backoff) — a redelivered event just re-polls a document that's already fully reconciled, which is a no-op.
* **Request headers:** `X-SignSetu-Event: signer.signed|document.completed`, `X-SignSetu-Signature: sha256=<hex>`.
* **Request body:** `{ "event": "...", "documentId": "...", "status": "...", "occurredAt": "...", "signer": {...}, "signers": [...] }` — see SignYu's Webhooks reference for the full per-signer shape.
* **Response (200):** `{ "status": "ok" }`, or `{ "status": "ignored" }` if the body had no `documentId`.
* **400** if the body isn't valid JSON (after the signature already verified).

---

## Pluggable e-signature providers

`provider` on a signature requirement is either `"manual"` (the built-in upload-a-signed-copy flow above, always available) or the name of a registered external provider. Adding a new provider tomorrow means: a new module under `server/file_server/src/signature_providers/`, one new `AppState` field for its client, one new provider-specific correlation table (mirroring `signyu_requirement_links`), and one new arm in the `add-signers` provider dispatch — the shared tables (`document_signature_requirements`, `signature_provider_requests`) never gain provider-specific columns; each provider's own ids/links live in its own table instead.

**SignYu** (https://signyu.com/docs) is the only external provider currently wired up: Aadhaar-based e-signature. Configure via env vars — `SIGNYU_API_KEY` (required to enable it), `SIGNYU_WEBHOOK_SECRET` (optional), `SIGNYU_BASE_URL` (optional, defaults to the real API — override for pointing at a mock server in tests). Every request and response to/from SignYu is logged to `signature_provider_requests`, success or failure.

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
