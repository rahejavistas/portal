---
name: accounting-money-correctness
description: Domain rules for correctness in the accounting/billing/ledger module — double-entry integrity, idempotency, GST/TDS/interest rules, void semantics. Use when writing or reviewing code in accounting/ or server/file_server/src/handlers/accounting/, or anything touching invoices, payments, ledger entries, GST, TDS, interest, or fiscal years.
---

# Accounting money correctness

This module is the one place in the app where a subtle bug costs real money. Bugs already found and fixed here (interest compounding on every scheduled run, void never reversing the ledger) are the shape of mistake to watch for — not hypothetical, already happened once.

## Money type

`rust_decimal::Decimal` everywhere. Never `f64`/`f32` on anything that represents currency.

## The ledger is append-only double-entry

- `ledger::post_transaction` is the *only* function anywhere that writes to `ledger_transactions`/`ledger_entries`. Every debit needs a matching credit in the same transaction — don't add a second write path.
- Rows on those two tables are never `UPDATE`d or `DELETE`d. A correction is a new reversing transaction (`reverses_transaction_id`), not an edit to the original.
- Voiding/cancelling/reversing anything that already posted must always create an equal-and-opposite entry. Flipping a status flag alone leaves revenue/expense permanently misstated — this is exactly the `void_invoice` bug that shipped once already.

## Anything on a schedule must be delta-based, not full-recompute

If code can be re-triggered for the same period (interest accrual, a recurring job), it must charge only what's newly accrued since the last run — never recompute the whole period-to-date and re-post it. Recomputing the full total on every run silently compounds every time it fires. Track "already posted" and subtract it before posting again.

## GST / TDS

- GST is computed on the base amount only (never on an amount that already includes GST or fund contributions), and frozen into the invoice/line-item row at generation time. A GST rule changing later must never retroactively alter an already-posted invoice.
- TDS must check both the single-payment threshold and the cumulative-annual threshold for that vendor. Known unresolved gap: crossing the annual threshold currently taxes only the triggering payment's increment, not the shortfall on already-paid amounts below threshold — if you're touching TDS, this needs a product decision on how to recover the shortfall, not a silent code fix.

## IDs that travel together must be verified together

A signature requirement, an expense, or any record referencing `(process_id, template_step_id, template_field_id, document_id)` or `(po_id, vendor_id)` must have that combination checked as belonging together — not just that each ID independently exists. Two independently-valid IDs pulled from different requests is exactly how a PO got billed to the wrong vendor once.

## Idempotency

Anything that can be double-submitted (a double-click, a retried request) needs a real de-dup path:
- If there's a natural unique key (a bank reference number), enforce it with a DB unique index, checked before any write.
- If there isn't (cash payments), use a narrow same-invoice/amount/method/recorder time-window check — long enough to catch a double-click, short enough not to block a genuine second same-day payment.

## Fiscal year guard

`post_transaction` checks the fiscal year isn't closed before writing. Don't add a code path that posts to the ledger without going through it — that's the only enforcement point.
