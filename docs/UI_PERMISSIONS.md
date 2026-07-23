# UI ↔ Permission Map

What UI a resident sees is driven entirely by boolean capability flags returned on the
`User` object (`can_manage_users`, `can_view_roles`, etc.) — never by raw permission codes.
See `derived_capabilities` in `server/authentication/src/lib.rs` for how each flag is
computed from the 10 seeded permission codes. This doc maps each permission to the concrete
UI it unlocks, for screenshotting/reference.

Two things are **not** permissions but still gate UI:
- **Flat ownership** (`wing`/`flat_number` not `TBD`) — gates "My Flat Dashboard", the
  personal `/process` page, and the `/dashboard` profile view.
- **`super_admin` designation** — a seed-only designation that holds all 10 permissions.
  Nothing in the UI hardcodes a `super_admin` check for capability gating; it just happens
  to unlock everything because it holds every permission.

---

## `users.manage`
*Edit/view residents and staff of the society.*

- Admin → **Residents** tab: left column ("Add Resident" form, "Bulk Upload CSV") is visible.
  Without this permission the column is hidden and the Registry table takes the full width.
- Per-resident **"Revoke Flat"** button in the Registry table.

## `users.view`
*View all residents.*

- Admin → **Residents** tab becomes visible at all (nav item + default landing suite).
- Registry table (email, flat mapping) is populated. Without either `users.view` or
  `users.manage`, the whole Residents suite is hidden.

## `users.roles.manage`
*Manage the designations of users.*

- Admin → **Roles** tab becomes visible; **User Designations** subtab is the default.
- Every resident (except `super_admin` accounts) is listed with their current designation
  badges. Your own row shows "(You)" instead of an action — you cannot edit your own
  designations even with this permission (self-demotion protection, enforced server-side).
- **"Manage Designations"** button per row opens a modal: a checklist of every non-system
  designation, current ones pre-checked, with a Save button to assign/revoke in one batch.

## `users.roles.view`
*View the designations of users (read-only).*

- Same **User Designations** subtab and table as `users.roles.manage`, but every row's
  Actions column is blank — no "Manage Designations" button, view-only.
- This is the "auditor for role assignments" permission — sees who has what, can't change it.

## `designations.manage`
*Create designations and manage their permissions and document category access.*

- Admin → **Roles** tab becomes visible; **Manage Designations** subtab appears.
- **All Designations** list: inline add-row (label → auto-slugified code), Rename, Delete
  per designation (system designations like `super_admin` are excluded).
- **Designation Permissions** matrix: one row per non-system designation, one checkbox
  column per permission code, sticky-left designation column, single Save button that
  diffs and batches the changes.
- **Document Category Access** card: per-designation "Manage Access" button opens a modal
  listing every grantable category (including `PUBLIC`) with Read/Manage checkboxes.
- Also unlocks the **Categories** subtab under Documents (category CRUD) — see
  `documents.categories.manage` below, which grants the same subtab independently.

## `documents.all.manage`
*View and manage all categories of documents.*

- Admin → **Documents** tab becomes visible; **Explorer** and **Add Document** subtabs
  are shown (and are the default landing subtab).
- Full document lifecycle across every category: browse/filter, upload, sign, delete.
- Note: `can_manage_documents` (the underlying flag) is also true if a user's designation
  holds a per-category `can_manage` grant via Document Category Access, even without this
  permission — e.g. a designation scoped to just `FINANCE` still sees Explorer/Add Document,
  just implicitly filtered to categories it manages.

## `documents.categories.manage`
*Create document categories.*

- Admin → **Documents** tab becomes visible; **Categories** subtab is shown and is the
  default landing subtab (Explorer/Add Document are hidden if this is the *only* documents
  permission held — a categories-only manager has no reason to see document content).
- **All Categories** list: inline add-row (label → auto-slugified code), Rename, Delete
  per category (`PUBLIC`/`RESIDENT_SPECIFIC` are system categories, excluded from CRUD but
  still grantable via Document Category Access above).

## `process.view`
*View all existing processes (read-only oversight/auditor role).*

- **"Process Review"** nav link appears (Admin sidebar, and as a cross-link from the
  personal `/process` page) — routes to `/admin-process`.
- `/admin-process` shows every resident's process queue, tabs (Admin Review Required /
  User Input Required / Completed), and process detail view with admin-role fields and
  reviewer comments visible.
- No write actions: no approve/reject, no "upload as admin", no admin note submission.
  (Combined with `process.reviewer` below for the write side — this permission alone is
  strictly read/oversight.)

## `process.reviewer`
*Review processes.*

- Implies everything `process.view` grants (same `/admin-process` access), plus:
- Approve/reject actions on process steps, ability to upload documents as an admin on a
  resident's behalf, submit admin/reviewer notes, edit admin-only fields.
- This is the only permission gating any *write* action on someone else's process —
  `personal_only`/write-path handlers on the backend never widen for `process.view` alone.

## `process.templates.manage`
*Create/edit process templates (the versioned builder) and activate versions.*

- **"Process Templates"** nav link/tab appears — reachable both from the Admin sidebar and
  from the `/admin-process` (Process Review) sidebar, so it's never hidden behind whichever
  page you happen to be on.
- Template list: each row has a **Versions (N)** button (opens a popup listing every version
  with its creator email, a **View** button to preview it read-only, and an **Activate**
  button on any non-active version) and an **Edit Latest** button (always opens the highest
  version number in the builder, regardless of which one is currently active).
- Builder: `+ New Template` / `+ Add Stage` / `+ Add Substep` / `+ Add Field` — a stage always
  has substeps (no "direct fields" mode). Saving always creates a new draft version; it never
  auto-activates — a separate Activate action is required, even for a template's very first
  version.
- Preview modal: read-only render of a version's stages/substeps/fields, matching the
  resident-facing `/process` page's visual language, so an admin can check how a template
  will actually look before activating it.
- This permission does not itself grant `process.view`/`process.reviewer` — it only manages
  template *definitions*, not any resident's in-progress process data.

---

## Quick reference table

| Permission | Suite tab unlocked | Primary UI |
|---|---|---|
| `users.manage` | Residents | Add Resident / Bulk Upload / Revoke Flat |
| `users.view` | Residents | Registry table |
| `users.roles.manage` | Roles → User Designations | Assign/revoke designations (not self) |
| `users.roles.view` | Roles → User Designations | Read-only designation list |
| `designations.manage` | Roles → Manage Designations | Designation CRUD, permission matrix, category access |
| `documents.all.manage` | Documents → Explorer/Add Document | Full document lifecycle |
| `documents.categories.manage` | Documents → Categories | Category CRUD |
| `process.view` | Process Review (`/admin-process`) | Read-only process oversight |
| `process.reviewer` | Process Review (`/admin-process`) | Approve/reject/upload/notes |
| `process.templates.manage` | Process Templates | Builder: create/edit/version/activate templates |
