---
name: permission-matrix-tests
description: Pattern for adding permission-matrix test coverage in this repo. Use when adding a new admin API route, a new Permission enum variant, or asked to add matrix test coverage for a route.
---

# Permission matrix tests

Location: `quality/tests/route_permission_matrix_tests.rs`. One test per permission, not one test per route — the file proves each permission grants exactly what it should and nothing else, not that every route works.

## The pattern

```rust
let jwt = seeded_resident(&env, &admin_jwt, "tag", "flat_number", &["permission.code"]);
let a = auth(&jwt);
```

`seeded_resident` creates a designation holding *exactly* the given permission codes, assigns it to a fresh resident, returns that resident's JWT. `flat_number` just needs to be unique within the test file.

For each new permission, write one test that:
1. Hits a representative route or two that permission actually gates → `assert_not_forbidden`.
2. Hits a representative route gated by each *sibling* permission in the same feature area → `assert_forbidden`.
3. Hits one route from a completely unrelated domain → `assert_forbidden`. This catches a permission accidentally granted too broadly.

## The two assertions

- `assert_not_forbidden` — accepts anything that isn't a 403. A 404/400 from fake data still proves the permission gate passed; this file checks gating, not business logic.
- `assert_forbidden` — requires exactly 403. If you need to check "no token at all," that's 401, not 403 — don't use this helper for it, check the status directly.

## Path parameters

Fake IDs are fine (`uuid::Uuid::new_v4()`, or a large int like `999999` for an integer ID) when the test only needs to confirm the permission gate — the resulting error just needs to not be 403.

## Self-service / no-permission routes

Routes reachable by any authenticated user (no router-level permission layer) get seeded with `&[]` and confirmed reachable, plus a separate check that requests with *no* `Authorization` header at all get 401 — self-service still means "must be logged in," not "unauthenticated is fine."

## Beyond the matrix

This file only proves the permission gate. If the new route also takes a resource ID (an invoice, a process, a document), it needs a *separate* test for the actual ownership/IDOR case — can a caller substitute someone else's ID and still succeed — typically alongside the feature's own business-logic tests, not in this file.
