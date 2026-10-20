# User Hierarchy & Data Model

Access control combines two models:

- **RBAC (designations + permissions)** — what a user's role lets them do, independent of any specific resource.
- **ABAC (flat ownership)** — which flat-scoped documents a resident can see, based on ownership, independent of role.

## Designations

A user can hold zero or more active designations at once. Only two designations are seeded — everything else is created dynamically by an admin at runtime (`POST /api/v1/admin/roles/designations/create`), so there is no fixed catalog beyond these two:

| Code | Label | Notes |
|---|---|---|
| `super_admin` | Super Admin | Seed-only (`is_system = TRUE`), granted at startup to `SUPER_ADMIN_EMAIL`, never assignable/revocable/deletable through the API. Seeded with every current permission. |
| `staff` | Staff | Seed-only (`is_system = TRUE`). Seeded with `users.manage`, `process.view`, `process.reviewer`. |

Any other designation (e.g. "committee", "treasurer") is entirely admin-defined: its code/label, which permissions it holds, and which document categories it can read/manage are all configured through the admin Roles UI — nothing else is hardcoded to a specific designation name in code.

A user with no designations is a plain resident: no admin-panel access, document visibility limited to documents in an `is_public_read` category plus `RESIDENT_SPECIFIC` documents mapped to their own flat.

## Tenants

A flat's owner can map a tenant to it (`POST /api/v1/tenants/add`) — a separate `users` row with no `flat_ownerships` row of its own, holding no designations and inheriting none of the owner's permissions or designations. `tenant_ownerships` tracks the mapping, with at most one active tenant per flat at a time (`is_active`; adding a new tenant deactivates the previous one, which loses access immediately).

| What | Owner | Tenant |
|---|---|---|
| Flat-scoped (`RESIDENT_SPECIFIC`) documents | Visible | Never visible, except the documents the tenant uploaded themselves for their current flat — they can attach those to their own stage; the flat's owner can see them |
| Starting a process | Any template | Only templates that have a `Tenant`-owned stage. The process belongs to the flat's owner, not the tenant |
| Process list | Every process on their own flat(s), including ones a tenant started | Only processes whose template has a `Tenant`-owned stage, and only for the flat they're the current active tenant of |
| What a process shows them | Everything on their flat's processes, including the tenant's stages | The whole process on their own flat, including the owner's answers and declaration, but not the documents on the owner's stages and not the owner's email. Nothing from any other flat |
| Reviewer-only upload actions on a process | No | No — reserved for reviewers |
| Filling a `Tenant`-owned stage | Cannot — even the owner has no standing on it | Only the current active tenant can; reviewing/reopening it is admin-only |
| Filling the owner's declaration stage | Only the flat's current owner — nobody else, not even an admin | Cannot |
| Designations/permissions held by the owner | N/A | Never inherited — a tenant's session carries only their own designations (normally none) |
| OTP/notification settings | Own profile | Own profile — same self-service endpoints as any user |

## Permissions

The full permission catalog is a compile-time enum (`authentication::Permission`), seeded into the `permissions` table in the same migration as the designations above:

| Code | Meaning |
|---|---|
| `users.manage` | Edit/view residents and staff of the society |
| `users.view` | View all residents |
| `users.roles.manage` | Manage the designations of users |
| `users.roles.view` | View the designations of users |
| `designations.manage` | Create designations and manage their permissions and document category access |
| `documents.all.manage` | View and manage all categories of documents |
| `documents.categories.manage` | Create document categories |
| `process.view` | View all existing processes |
| `process.reviewer` | Review processes |
| `process.templates.manage` | Create/edit process templates (the versioned builder) and activate versions |
| `accounting.view` | View accounting/billing data (read-only) |
| `accounting.manage` | Manage accounting/billing — invoices, payments, ledger, settings |

`super_admin` is seeded with all twelve; `staff` with `users.manage`/`process.view`/`process.reviewer` only. Any admin-created designation starts with none and gets permissions toggled on individually via `POST /api/v1/admin/roles/designations/permissions/toggle`.

## Document access resolution

Document visibility is driven by `documents.category`, not by a per-document ACL. `document/manager.rs::get_allowed_document_ids` resolves the set of document IDs a user may see:

1. Holding `documents.all.manage` — bypasses everything, sees every non-deleted document. (`super_admin` gets this because it's seeded with every permission, not because of the designation itself — the check is purely permission-based.)
2. Any category with `is_public_read = TRUE` — visible to every authenticated user.
3. Category read-mapped to one of the user's active designations, via `document_category_designations` (`can_read = TRUE`).
4. `RESIDENT_SPECIFIC` (or any other `is_flat_scoped` category) documents explicitly linked (via `document_flats`) to a flat the user actively owns/occupies (`flat_ownerships`).

## Tables

### Users, flats, RBAC

| Table | Purpose |
|---|---|
| `users` | Core identity: email, active flag, timestamps. |
| `user_credentials` | Password hash and password-reset token, 1:1 with `users`. |
| `flats` | Wing + flat number, the unit of residence, admin-managed (create/bulk-create/delete) via Society Profile → Flat Setup. `is_fire_flat` marks a flat excluded from the Add Resident picker and from bulk floor/wing removal. |
| `flat_ownerships` | Which user owns/occupies which flat, with `is_active` (one active owner per flat, enforced by a partial unique index) and start/end dates. |
| `designations` | The role catalog — only `super_admin`/`staff` are seeded (`is_system = TRUE`); everything else is admin-created at runtime. |
| `permissions` | The permission catalog (see table above), seeded once, never admin-created. |
| `designation_permissions` | Join table: which designations hold which permissions. |
| `user_designations` | Join table: which users hold which designations, with `is_active` (revocation without deletion) and `assigned_by` (audit trail). |
| `tenant_ownerships` | A flat's current (and historical) tenant, mapped by the owner — `ownership_id` → `tenant_user_id`, with `is_active` (at most one active tenant per flat, enforced by a partial unique index), `expires_at`, and an optional `rental_agreement_document_id`. |

### Documents

| Table | Purpose |
|---|---|
| `documents` | Core document record: title, description, `category`, upload/deletion/confirmation state. |
| `document_categories` | Fully admin-creatable category catalog. Only `PUBLIC` and `RESIDENT_SPECIFIC` are seeded (`is_system = TRUE`, the two protected categories); `PUBLIC` has `is_public_read = TRUE`, `RESIDENT_SPECIFIC` has `is_flat_scoped = TRUE`. Any other category an admin creates has both flags `FALSE` by default and relies entirely on `document_category_designations` for visibility. |
| `document_subcategories` | Optional, UI-only tag nested under a category (unique per `(category, code)`) — never affects access control. |
| `generic_document_metadata` | Optional 1:1 metadata: subcategory, tags, document date, validity window. |
| `document_flats` | Many-to-many: which flat-scoped documents are visible to which flats. |
| `document_category_designations` | The RBAC-to-document bridge: `(category, designation_id)` → `can_read`/`can_manage`. Fully admin-configured via the Roles UI — nothing is hardcoded here; it is additive, so a designation can be mapped to more than one category and a category can be opened up to more than one designation. `PUBLIC`-like (`is_public_read`) categories bypass this table entirely for reads (visible to all); flat-scoped categories bypass it too (governed by `document_flats`/`flat_ownerships` instead). |

### Process workflows & templates

| Table | Purpose |
|---|---|
| `process_templates` | One row per logical process type (e.g. "Tenant Onboarding"): its `name` and which `active_version_id` residents currently start against (`NULL` until an admin explicitly activates a version). |
| `process_template_versions` | Append-only: every Save from the admin builder adds a new row here (own `version_id`, a `version_number` for display, and the full tree as `definition` JSONB). Never mutated in place. |
| `processes` | One resident-initiated workflow instance, permanently pinned to one `template_version_id` for its whole lifetime, optionally tied to a `flat_ownerships` row. Also carries the TAT/SLA clock: `tat_working_days` (copied from the template at start), `sla_started_at`/`sla_due_date` (set once, at first full submission — not at process creation), `sla_paused_at`/`sla_paused_working_days` (paused by an admin return, resumed on resubmission). |
| `process_steps` | Steps within a process, tree-shaped via `parent_step_instance_id`; `is_completed` drives the state machine. |
| `process_fields` | Append-only field submission log per step (documents or text values); latest value per field wins. `updated_by` records which user saved that value, for the audit trail. |
| `process_step_reviews` | Reviewer decision history per step (`reviewer_user_id`, `is_approved`, notes, flagged step IDs). |
| `process_reviewer_comments` | Standalone reviewer notes on a step, never shown to the resident. |
| `process_audit_log` | Append-only status/verification/return trail, one row per event (`STEP_COMPLETED`, `STEP_APPROVED`, `STEP_RETURNED`, `STEP_FLAGGED`, `PROCESS_CLOSED`), surfaced in the admin process detail's "Audit Trail" popup and its PDF export — a different, step-level mechanism from `process_fields`' field-level history above. |
| `public_holidays` | Shared calendar excluded from every template's TAT working-day math, alongside weekends. Admin-managed via Society Profile → Public Holidays. |

## Frontend

`frontend/src/views/{dashboard,admin,process}.rs` each embed a standalone `<script>` block (no shared JS bundle in this pipeline). `process.rs` renders both `/process` (resident) and `/admin-process` (reviewer queue) from one shared template, swapping a mode flag and title at render time. All three files define an identical `isAdminUser(user)` helper checking whether the user holds any admin-capability permission flag (`can_view_users`, `can_manage_documents`, `can_review_process`, etc.) — kept in sync manually across the three files.
