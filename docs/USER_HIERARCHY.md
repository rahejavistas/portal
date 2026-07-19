# User Hierarchy & Data Model

Access control combines two models:

- **RBAC (designations + permissions)** — what a user's role lets them do, independent of any specific resource.
- **ABAC (flat ownership)** — which flat-scoped documents a resident can see, based on ownership, independent of role.

## Designations

A user can hold zero or more active designations at once.

| Code | Label | Notes |
|---|---|---|
| `super_admin` | Super Admin | Seed-only — granted at startup to `SUPER_ADMIN_EMAIL`, never assignable through the API. Holds every permission. |
| `staff` | Staff | Manages `OPERATIONAL` documents. |
| `committee` | Committee | The only designation assignable/revocable through `POST /api/v1/admin/users/roles`. Manages `COMMITTEE` documents. |
| `office_bearer` | Office Bearer | Reserved — not currently mapped to any permission or document category. |
| `treasurer` | Treasurer | Manages `FINANCE` documents. |
| `secretary` | Secretary | Manages `GOVERNANCE` documents. |
| `auditor` | Auditor | Reserved — not currently mapped to any permission or document category. |

A user with no designations is a plain resident: no admin-panel access, document visibility limited to `PUBLIC` documents plus `RESIDENT_SPECIFIC` documents mapped to their own flat.

## Permissions

Seeded in the same migration, all currently granted only to `super_admin` (`designation_permissions`). Other designations get their access implicitly through `document_category_designations` (see below), not through this permissions table — no code path currently calls `user_has_permission` for `staff`/`committee`/`treasurer`/`secretary`.

| Code | Meaning |
|---|---|
| `users.manage` | Edit/view residents and staff of the society |
| `users.view` | View all residents |
| `users.roles.manage` | Manage the designations of users |
| `users.roles.view` | View the designations of users |
| `documents.all.manage` | View and manage all categories of documents |
| `documents.public.manage` | Manage `PUBLIC` documents |
| `documents.resident_specific.manage` | Manage `RESIDENT_SPECIFIC` documents |
| `documents.committee.manage` | Manage `COMMITTEE` documents |
| `documents.finance.manage` | Manage `FINANCE` documents |
| `documents.governance.manage` | Manage `GOVERNANCE` documents |
| `documents.operational.manage` | Manage `OPERATIONAL` documents |
| `process.view` | View all existing processes |
| `process.reviewer` | Review processes |

## Document access resolution

Document visibility is driven by `documents.category`, not by a per-document ACL. `document/manager.rs::get_allowed_document_ids` resolves the set of document IDs a user may see:

1. `super_admin` — bypasses everything, sees all non-deleted documents.
2. `PUBLIC` category — visible to every authenticated user.
3. Category mapped to one of the user's active designations, via `document_category_designations` (e.g. a `committee` designation unlocks all `COMMITTEE` documents).
4. `RESIDENT_SPECIFIC` documents explicitly linked (via `document_flats`) to a flat the user owns/occupies (`flat_ownerships`).

## Tables

### Users, flats, RBAC

| Table | Purpose |
|---|---|
| `users` | Core identity: email, active flag, timestamps. |
| `user_credentials` | Password hash and password-reset token, 1:1 with `users`. |
| `flats` | Wing + flat number, the unit of residence. |
| `flat_ownerships` | Which user owns/occupies which flat, with `is_active` (one active owner per flat, enforced by a partial unique index) and start/end dates. |
| `designations` | The role catalog (see table above). `is_system` marks seed-only roles (`super_admin`). |
| `permissions` | The permission catalog (see table above). |
| `designation_permissions` | Join table: which designations hold which permissions. Currently every row points at `super_admin` — all 13 permissions are seeded only for that designation, so `staff`/`committee`/`treasurer`/`secretary` currently hold none. Those designations get their actual access another way: implicitly, through `document_category_designations` (see below), not through this table. |
| `user_designations` | Join table: which users hold which designations, with `is_active` (revocation without deletion) and `assigned_by` (audit trail). |

### Documents

| Table | Purpose |
|---|---|
| `documents` | Core document record: title, description, `category`, upload/deletion/confirmation state. |
| `generic_document_metadata` | Optional 1:1 metadata: subcategory, tags, document date, validity window. |
| `document_flats` | Many-to-many: which `RESIDENT_SPECIFIC` documents are visible to which flats. |
| `document_category_designations` | The RBAC-to-document bridge: maps each non-`PUBLIC`, non-`RESIDENT_SPECIFIC` category to the designation(s) that can view/manage it. Seeded as `COMMITTEE`→`committee`, `FINANCE`→`treasurer`, `GOVERNANCE`→`secretary`, `OPERATIONAL`→`staff`. A user's active designations are joined against this table (see [Document access resolution](#document-access-resolution)) to compute visibility — it is additive, so a designation can be mapped to more than one category and a category could be opened up to more than one designation. `PUBLIC` bypasses this table entirely (visible to all); `RESIDENT_SPECIFIC` bypasses it too (governed by `document_flats`/`flat_ownerships` instead). |

### Process workflows

| Table | Purpose |
|---|---|
| `processes` | One resident-initiated workflow instance (e.g. tenant onboarding), tied to a `template_id` and optionally a `flat_ownerships` row. |
| `process_steps` | Steps within a process, tree-shaped via `parent_step_instance_id`; `is_completed` drives the state machine. |
| `process_fields` | Append-only field submission log per step (documents or text values); latest value per field wins. |
| `process_step_reviews` | Reviewer decision history per step (`reviewer_user_id`, `is_approved`, notes, flagged step IDs). |
| `process_reviewer_comments` | Standalone reviewer notes on a step, never shown to the resident. |

## Frontend

`frontend/src/views/{dashboard,admin,process}.rs` each embed a standalone `<script>` block (no shared JS bundle in this pipeline). All three define an identical `isAdminUser(user)` helper checking `user.designations.includes('super_admin')` — kept in sync manually across the three files.
