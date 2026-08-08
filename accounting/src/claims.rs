use crate::db;
use crate::payments::{self, RecordPaymentRequest};
use crate::{ClaimId, InvoiceId, PaymentId};
use chrono::Utc;

#[derive(Debug)]
pub enum ClaimError {
    NotFound,
    NotPending,
    NotOwnClaim,
    Payment(payments::PaymentError),
    Db(sqlx::Error),
}

impl std::fmt::Display for ClaimError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClaimError::NotFound => write!(f, "Payment claim not found"),
            ClaimError::NotPending => write!(f, "This claim has already been reviewed"),
            ClaimError::NotOwnClaim => write!(f, "This claim does not belong to you"),
            ClaimError::Payment(e) => write!(f, "{}", e),
            ClaimError::Db(e) => write!(f, "{}", e),
        }
    }
}

impl From<sqlx::Error> for ClaimError {
    fn from(e: sqlx::Error) -> Self {
        ClaimError::Db(e)
    }
}

impl From<payments::PaymentError> for ClaimError {
    fn from(e: payments::PaymentError) -> Self {
        ClaimError::Payment(e)
    }
}

pub struct SubmitClaimRequest<'a> {
    pub invoice_id: InvoiceId,
    pub claimed_by: i32,
    pub amount: rust_decimal::Decimal,
    pub payment_method: &'a str,
    pub reference_number: &'a str,
    pub note: Option<&'a str>,
}

/// A resident reporting "I paid this, here's my transaction reference" for an offline
/// payment. Only ever creates a PENDING claim row — never touches the ledger. An admin
/// must separately confirm (`confirm_claim`) before it becomes a real, posted payment.
pub async fn submit_claim(
    conn: &mut sqlx::PgConnection,
    req: SubmitClaimRequest<'_>,
) -> Result<ClaimId, ClaimError> {
    let claim_id = db::insert_payment_claim(
        conn,
        db::NewPaymentClaim {
            invoice_id: req.invoice_id,
            claimed_by: req.claimed_by,
            amount: req.amount,
            payment_method: req.payment_method,
            reference_number: req.reference_number,
            note: req.note,
        },
    )
    .await?;
    Ok(claim_id)
}

/// Confirms a pending claim: posts the actual payment (Dr. Bank / Cr. Receivable) via
/// the same path an admin-entered payment takes, then links the claim to it. The claim
/// itself is never the thing that moves money — this call is.
pub async fn confirm_claim(
    conn: &mut sqlx::PgConnection,
    claim_id: ClaimId,
    reviewed_by: i32,
) -> Result<PaymentId, ClaimError> {
    let claim = db::get_payment_claim(conn, claim_id)
        .await?
        .ok_or(ClaimError::NotFound)?;
    if claim.status != "PENDING" {
        return Err(ClaimError::NotPending);
    }

    let result = payments::record_payment(
        conn,
        RecordPaymentRequest {
            invoice_id: claim.invoice_id,
            amount: claim.amount,
            payment_method: &claim.payment_method,
            reference_number: Some(&claim.reference_number),
            payment_date: Utc::now().date_naive(),
            recorded_by: reviewed_by,
        },
    )
    .await?;

    db::mark_claim_confirmed(conn, claim_id, reviewed_by, result.payment_id).await?;
    Ok(result.payment_id)
}

pub async fn reject_claim(
    conn: &mut sqlx::PgConnection,
    claim_id: ClaimId,
    reviewed_by: i32,
    review_note: &str,
) -> Result<(), ClaimError> {
    let claim = db::get_payment_claim(conn, claim_id)
        .await?
        .ok_or(ClaimError::NotFound)?;
    if claim.status != "PENDING" {
        return Err(ClaimError::NotPending);
    }
    db::mark_claim_rejected(conn, claim_id, reviewed_by, review_note).await?;
    Ok(())
}
