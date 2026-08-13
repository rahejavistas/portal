# Working rules for this repo

Two things live outside this file, in `.claude/skills/`, and load only when the task actually needs them: `accounting-money-correctness` (ledger/GST/TDS/interest domain rules) and `permission-matrix-tests` (the test-authoring pattern for `route_permission_matrix_tests.rs`).

## Top priorities, in order

1. **Security**
2. **Correctness**
3. **Readability**
4. **Tests**

Everything below serves these four. When two rules pull in different directions, resolve toward whichever of these ranks higher.

## Rust idioms

- Prefer enums over hardcoded/stringly-typed state wherever the set of values is fixed at compile time (a status field, a mode, a kind of thing). `process/src/state_machine.rs`'s `StepStatus`/`ProcessTurn` is the pattern to follow — match on the enum, let the compiler catch missing cases, don't compare strings.
- This does **not** apply to genuinely dynamic, user/admin-defined values — designation codes, permission codes, custom template fields, and anything else configured at runtime stay as their dynamic representation (`String`, a code column, etc.). Don't force an open-ended, admin-extensible set into a closed enum.
- Put behavior on the enum itself (`impl` blocks, methods) instead of scattering `match`es over the same enum across multiple call sites. If three functions all match on the same enum to derive the same kind of fact, that's a method on the enum.
- Favor `match` over chains of `if`/`else` on the same value — it's exhaustive, and exhaustiveness is what makes this readable and safe to extend.
- Code should read clearly without being verbose. Prefer the idiom that makes the reader's job easiest, not the one with the most ceremony.

## More Rust idioms

- New entity IDs get a typed newtype (the `entity_id!` macro already used for `InvoiceId`, `VendorId`, etc.), not a raw `i32`/`Uuid` passed around. It's what stops an `invoice_id` and a `vendor_id` from being silently swapped at a call site — the compiler catches it instead of a test having to.
- When matching your own enum, don't add a catch-all `_ =>` arm unless you genuinely mean "everything else, forever." List every variant explicitly, so adding a new variant later forces the compiler to point at every place that needs to handle it — that's the whole value of an enum over a string.
- A small `enum`-per-module error type (`BillingError`, `PaymentError`, `ExpenseError` are the existing pattern) beats a stringly-typed error or a generic catch-all. Implement `Display` and `From<sqlx::Error>` and let `?` do the propagation — don't hand-roll match/return boilerplate for that.
- Avoid `.unwrap()`/`.expect()` on anything that depends on user input or external state. It's fine on a genuine invariant (something the code already guarantees), but say why in the `.expect("...")` message itself rather than leaving it bare.
- Borrow (`&str`, `&[T]`) in function signatures instead of taking ownership unless the function actually needs to keep the value. Don't add a `.clone()` to make a borrow-checker complaint go away without checking whether a borrow would have worked.
- Keep functions to one job. When a function mixes concerns (validate, then compute, then persist, then notify), split it into named steps — that's usually more readable than one long function, not less.

## Comments

- **Frontend/UI code (`frontend/src/views/*.rs`, embedded HTML/CSS/JS): no comments at all.**
- **Server/backend code: comments only when absolutely necessary** — a non-obvious invariant, a subtle correctness reason, a genuine gotcha. Not for anything a reader gets from the code itself.
- When a comment exists, it must be **short**.
- **Never reference past/removed code, prior bugs, or "why this changed" history in a comment.** A comment describes the current code as it stands, not its history. That history belongs in the commit message, not the source.
- Function-level comments (when warranted): state what the function does and flag anything genuinely special about it (a non-obvious side effect, an ordering requirement, a constraint the caller must uphold). Don't restate the function name in prose.

## Code

- No speculative code. No unused abstractions. No code paths for cases that can't happen.
- No code is the best code — prefer deleting/simplifying over adding.
- Use the best-fitting design pattern for the problem, not the first one that works.

## Testing

- Very high bar. Cover real corner cases and real-world scenarios end to end, not just the happy path.
- New API routes get permission-matrix coverage in `quality/tests/route_permission_matrix_tests.rs`.

## Security

- Correctness here is non-negotiable. Every new route needs a real ownership/ID-scoping check, not just a permission check — holding a permission must never imply access to an arbitrary resource unless that's the documented, intentional scope.
- This applies retroactively, not just to new code: any time a handler is opened for any reason — a bug fix, an unrelated refactor, "can you check X" — verify it actually has a real ownership check, not just a permission check, regardless of how old the route is or who wrote it. Old code doesn't get a pass just because it predates this rule.
- Nothing that shouldn't be visible to a given user's role/permissions gets sent to the frontend, ever — not in a response body, not in an error message, not behind a hidden UI element. Client-side hiding is not access control.
- All server API routes are POST, no exceptions.
- New routes need permission-matrix coverage in `quality/tests/route_permission_matrix_tests.rs`, plus tests for the specific IDOR/ownership edge cases relevant to that route (wrong user's ID, wrong flat's ID, cross-resource substitution), not just the 403/not-403 permission check.
- Diff-scoped AI review (CodeRabbit on a PR) only ever sees new/changed lines — it will never catch a bug sitting in code nobody's touched. Periodically trigger a full-repo review (a PR whose base is the repo's root commit, so the diff is the whole codebase) rather than assuming diff reviews add up to full coverage over time. This is on whoever's driving the session to ask for — it doesn't happen on its own.

## Documentation

- `docs/*.md` must stay in sync with the code — whenever a change affects what's documented there (a flow, a permission, a feature's behavior), update the relevant doc in the same piece of work, not later.
- Written for stakeholders, not engineers: plain, simple, human language, no function/file/code references, no implementation detail. One or two lines per point.
- What changed and how the pieces interact — not how it's built. A stakeholder cares about outcomes, not internals.
- Use tables and matrices for anything with more than a couple of variables (roles × permissions, states × transitions) — that reads faster than a paragraph ever will.

## Logging and metrics

- Log sparingly. Don't log something that's about to be logged again one call up the stack, and don't log inside a loop on every iteration.
- Expected/validation failures (bad input, already-approved, insufficient balance, permission denied — anything that's normal control flow and becomes a 4xx) don't need error-level logging. That's not "something went wrong," it's the system working correctly.
- Unexpected errors and DB errors get logged verbosely, with the full error chain and enough context to actually reproduce the issue (the relevant IDs, what operation was being attempted) — not just the bare error string. We don't have a stack trace otherwise, so the log has to carry that weight.
- Add a Grafana counter for system health, not business numbers — external integration calls (SignYu, S3, email) and similar operational signals, the same way `middleware.rs`'s `api_requests_total` and `signyu.rs`'s `signyu_provider_responses_total` already do via `opentelemetry::global::meter(...)`. Business metrics (revenue, invoice counts, collection rates) belong in the actual accounting reports, not a systems dashboard.

## Database and transactions

- Never do external I/O — an HTTP call to SignYu, an S3 upload, sending an email — inside a `pg_pool.begin()`/`commit()` block. That work belongs before the transaction opens or after it commits. A DB transaction should hold nothing but the atomic state change itself, and retried/replayed external calls inside a transaction can run twice.
- Anything that could plausibly be retried or double-submitted (a double-click, a client retry, a re-run of a scheduled job) must be idempotent — the interest and cash-payment fixes already in this codebase are the reference examples. Design for "this might run twice" from the start, not as a patch after the bug report.
- Prefer atomic SQL over read-modify-write in application code: a guarded `UPDATE ... WHERE` that only succeeds when the precondition still holds, or `INSERT ... ON CONFLICT`, instead of fetch-then-check-then-write from Rust. The gap between the read and the write is exactly where double-charges and lost updates come from.
- Batch multi-row writes (a single `INSERT ... SELECT FROM UNNEST(...)`) instead of looping a per-row `INSERT`/`UPDATE` in application code — fewer round trips, and it reads as one operation instead of a loop pretending to be one.
- Select only the columns a query actually needs. Don't `SELECT *` and then use three of the twelve columns.

## Migrations

- **Never run a migration yourself unless explicitly asked to.**

## Formatting and checks

- `./scripts/run_checks.sh` runs `cargo fmt`, `cargo check --all-targets`, and `cargo clippy --all-targets -- -D warnings`, in that order. Run this instead of formatting/checking manually or piecing the commands together — it's already set up to do the whole thing in one go.

## Frontend builds

- **Always run `python frontend/build.py` after touching anything under `frontend/src/views/*.rs`.** The generated `dist/*.html` files are what actually ships — edits to the `.rs` source alone don't take effect until this runs.
- All UI must be mobile-friendly, always. Check this, don't assume it.
- There is no JS tooling in this repo — no linter, no type checker, nothing in `run_checks.sh` or `build.py` looks at the embedded `<script>` blocks at all. `cargo check`/clippy cannot see JavaScript. A scoping bug (referencing a `const`/`let` from outside the block it's declared in) will compile fine and only fail at runtime in a browser. When touching JS inside `frontend/src/views/*.rs`, manually trace variable scope for anything referenced across function boundaries — don't rely on any check catching it, because none currently can.
