use crate::accounts;
use crate::db;
use crate::ledger::{self, EntryLine, PostTransactionRequest};
use crate::{InvoiceId, PaymentId, ReceiptId, TransactionId};
use chrono::NaiveDate;
use rust_decimal::Decimal;

#[derive(Debug)]
pub enum PaymentError {
    InvoiceNotFound,
    InvoiceVoid,
    Post(ledger::PostError),
    Db(sqlx::Error),
}

impl std::fmt::Display for PaymentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PaymentError::InvoiceNotFound => write!(f, "Invoice not found"),
            PaymentError::InvoiceVoid => {
                write!(f, "Cannot record a payment against a void invoice")
            }
            PaymentError::Post(e) => write!(f, "{}", e),
            PaymentError::Db(e) => write!(f, "{}", e),
        }
    }
}

impl From<sqlx::Error> for PaymentError {
    fn from(e: sqlx::Error) -> Self {
        PaymentError::Db(e)
    }
}

impl From<ledger::PostError> for PaymentError {
    fn from(e: ledger::PostError) -> Self {
        PaymentError::Post(e)
    }
}

/// How close together two reference-less (cash) payments with identical
/// invoice/amount/method/recorder have to be to be treated as a duplicate submission
/// rather than a genuine second payment. Deliberately short — long enough to absorb a
/// double-click or a retried request, short enough not to block a real same-day repeat
/// cash payment made minutes apart.
const DUPLICATE_CASH_PAYMENT_WINDOW_SECONDS: i64 = 30;

pub struct RecordPaymentResult {
    pub payment_id: PaymentId,
    pub receipt_id: ReceiptId,
    pub receipt_number: String,
    /// `false` when `reference_number` was already recorded for this `payment_method`
    /// — nothing new was posted, the original receipt is returned as-is.
    pub newly_recorded: bool,
}

pub struct RecordPaymentRequest<'a> {
    pub invoice_id: InvoiceId,
    pub amount: Decimal,
    pub payment_method: &'a str,
    pub reference_number: Option<&'a str>,
    pub payment_date: NaiveDate,
    pub recorded_by: i32,
}

/// How much of a payment applies to the invoice it's recorded against vs. how much is
/// an overpayment that becomes a credit balance instead of over-crediting the invoice.
fn split_payment(amount: Decimal, outstanding: Decimal) -> (Decimal, Decimal) {
    if amount <= outstanding {
        (amount, Decimal::ZERO)
    } else {
        (outstanding, amount - outstanding)
    }
}

/// Records a manually-confirmed payment (from a bank statement, cheque, UPI reference,
/// or cash — there is no payment gateway integration): Dr. Bank / Cr. Member Dues
/// Receivable (up to the invoice's outstanding balance), applies it to the invoice's
/// running balance, and issues a receipt. Any amount paid beyond the invoice's
/// outstanding balance is credited to the flat's advance/credit balance (Cr. Member
/// Advance) instead of silently over-crediting the invoice past 100% paid — see
/// `apply_credit_to_invoice` for spending that credit against a future bill.
///
/// Idempotent on (payment_method, reference_number) when a reference is given, checked
/// BEFORE any ledger write, so re-submitting the same bank line never double-posts.
pub async fn record_payment(
    conn: &mut sqlx::PgConnection,
    req: RecordPaymentRequest<'_>,
) -> Result<RecordPaymentResult, PaymentError> {
    if let Some(reference) = req.reference_number {
        if let Some(existing) =
            db::find_payment_by_reference(conn, req.payment_method, reference).await?
        {
            let receipt = db::get_receipt_by_payment(conn, existing.payment_id)
                .await?
                .expect("payment always has a receipt — inserted in the same transaction");
            return Ok(RecordPaymentResult {
                payment_id: existing.payment_id,
                receipt_id: receipt.receipt_id,
                receipt_number: receipt.receipt_number,
                newly_recorded: false,
            });
        }
    } else if let Some(existing) = db::find_recent_similar_reference_less_payment(
        conn,
        req.invoice_id,
        req.amount,
        req.payment_method,
        req.recorded_by,
        DUPLICATE_CASH_PAYMENT_WINDOW_SECONDS,
    )
    .await?
    {
        let receipt = db::get_receipt_by_payment(conn, existing.payment_id)
            .await?
            .expect("payment always has a receipt — inserted in the same transaction");
        return Ok(RecordPaymentResult {
            payment_id: existing.payment_id,
            receipt_id: receipt.receipt_id,
            receipt_number: receipt.receipt_number,
            newly_recorded: false,
        });
    }

    let invoice = db::get_invoice_for_update(conn, req.invoice_id)
        .await?
        .ok_or(PaymentError::InvoiceNotFound)?;
    if invoice.status == "VOID" {
        return Err(PaymentError::InvoiceVoid);
    }

    let outstanding = invoice.total_amount - invoice.amount_paid;
    let (applied, excess) = split_payment(req.amount, outstanding.max(Decimal::ZERO));

    let bank_account = db::get_account_id_by_code(conn, accounts::BANK_OPERATING).await?;
    let receivable_account =
        db::get_account_id_by_code(conn, accounts::MEMBER_DUES_RECEIVABLE).await?;

    let mut entries = vec![EntryLine {
        account_id: bank_account,
        flat_id: Some(invoice.flat_id),
        debit: req.amount,
        credit: Decimal::ZERO,
    }];
    if applied > Decimal::ZERO {
        entries.push(EntryLine {
            account_id: receivable_account,
            flat_id: Some(invoice.flat_id),
            debit: Decimal::ZERO,
            credit: applied,
        });
    }
    if excess > Decimal::ZERO {
        let advance_account = db::get_account_id_by_code(conn, accounts::MEMBER_ADVANCE).await?;
        entries.push(EntryLine {
            account_id: advance_account,
            flat_id: Some(invoice.flat_id),
            debit: Decimal::ZERO,
            credit: excess,
        });
    }

    let transaction_id = ledger::post_transaction(
        conn,
        PostTransactionRequest {
            transaction_date: req.payment_date,
            description: &format!("Payment for invoice {}", invoice.invoice_number),
            source_type: "PAYMENT",
            source_id: Some(req.invoice_id.0),
            reverses_transaction_id: None,
            entries,
            created_by: req.recorded_by,
        },
    )
    .await?;

    let payment_id = db::insert_payment(
        conn,
        db::NewPayment {
            invoice_id: req.invoice_id,
            amount: req.amount,
            payment_method: req.payment_method,
            reference_number: req.reference_number,
            recorded_by: req.recorded_by,
            ledger_transaction_id: transaction_id,
        },
    )
    .await?;

    if applied > Decimal::ZERO {
        db::apply_payment_to_invoice(conn, req.invoice_id, applied).await?;
    }

    let receipt_number = db::next_receipt_number(conn).await?;
    let receipt_id = db::insert_receipt(conn, &receipt_number, payment_id).await?;

    Ok(RecordPaymentResult {
        payment_id,
        receipt_id,
        receipt_number,
        newly_recorded: true,
    })
}

#[derive(Debug)]
pub enum CreditApplicationError {
    InvoiceNotFound,
    InvoiceVoid,
    NoOutstandingBalance,
    InsufficientCredit,
    Post(ledger::PostError),
    Db(sqlx::Error),
}

impl std::fmt::Display for CreditApplicationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CreditApplicationError::InvoiceNotFound => write!(f, "Invoice not found"),
            CreditApplicationError::InvoiceVoid => {
                write!(f, "Cannot apply credit to a void invoice")
            }
            CreditApplicationError::NoOutstandingBalance => {
                write!(
                    f,
                    "This invoice has no outstanding balance to apply credit against"
                )
            }
            CreditApplicationError::InsufficientCredit => {
                write!(
                    f,
                    "This flat's credit balance is lower than the amount requested"
                )
            }
            CreditApplicationError::Post(e) => write!(f, "{}", e),
            CreditApplicationError::Db(e) => write!(f, "{}", e),
        }
    }
}

impl From<sqlx::Error> for CreditApplicationError {
    fn from(e: sqlx::Error) -> Self {
        CreditApplicationError::Db(e)
    }
}

impl From<ledger::PostError> for CreditApplicationError {
    fn from(e: ledger::PostError) -> Self {
        CreditApplicationError::Post(e)
    }
}

/// Spends (part of) a flat's advance/credit balance against a specific invoice: Dr.
/// Member Advance / Cr. Member Dues Receivable. `amount` is capped at both the flat's
/// available credit and the invoice's outstanding balance by the caller checking first
/// — this function itself still re-validates both rather than trusting the caller.
pub async fn apply_credit_to_invoice(
    conn: &mut sqlx::PgConnection,
    invoice_id: InvoiceId,
    amount: Decimal,
    applied_by: i32,
    apply_date: NaiveDate,
) -> Result<TransactionId, CreditApplicationError> {
    let invoice = db::get_invoice_for_update(conn, invoice_id)
        .await?
        .ok_or(CreditApplicationError::InvoiceNotFound)?;
    if invoice.status == "VOID" {
        return Err(CreditApplicationError::InvoiceVoid);
    }
    let outstanding = invoice.total_amount - invoice.amount_paid;
    if outstanding <= Decimal::ZERO {
        return Err(CreditApplicationError::NoOutstandingBalance);
    }
    if amount > outstanding {
        return Err(CreditApplicationError::NoOutstandingBalance);
    }
    db::lock_flat_credit(conn, invoice.flat_id).await?;
    let available_credit = db::get_flat_credit_balance(conn, invoice.flat_id).await?;
    if amount > available_credit {
        return Err(CreditApplicationError::InsufficientCredit);
    }

    let advance_account = db::get_account_id_by_code(conn, accounts::MEMBER_ADVANCE).await?;
    let receivable_account =
        db::get_account_id_by_code(conn, accounts::MEMBER_DUES_RECEIVABLE).await?;

    let transaction_id = ledger::post_transaction(
        conn,
        PostTransactionRequest {
            transaction_date: apply_date,
            description: &format!("Credit applied to invoice {}", invoice.invoice_number),
            source_type: "CREDIT_APPLICATION",
            source_id: Some(invoice_id.0),
            reverses_transaction_id: None,
            entries: vec![
                EntryLine {
                    account_id: advance_account,
                    flat_id: Some(invoice.flat_id),
                    debit: amount,
                    credit: Decimal::ZERO,
                },
                EntryLine {
                    account_id: receivable_account,
                    flat_id: Some(invoice.flat_id),
                    debit: Decimal::ZERO,
                    credit: amount,
                },
            ],
            created_by: applied_by,
        },
    )
    .await?;

    db::apply_payment_to_invoice(conn, invoice_id, amount).await?;
    Ok(transaction_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn exact_payment_has_no_excess() {
        assert_eq!(split_payment(dec!(5000), dec!(5000)), (dec!(5000), dec!(0)));
    }

    #[test]
    fn underpayment_applies_in_full_no_excess() {
        assert_eq!(split_payment(dec!(3000), dec!(5000)), (dec!(3000), dec!(0)));
    }

    #[test]
    fn overpayment_caps_applied_at_outstanding_and_credits_the_rest() {
        assert_eq!(
            split_payment(dec!(7000), dec!(5000)),
            (dec!(5000), dec!(2000))
        );
    }

    #[test]
    fn payment_against_zero_outstanding_is_entirely_credit() {
        assert_eq!(split_payment(dec!(1000), dec!(0)), (dec!(0), dec!(1000)));
    }
}
