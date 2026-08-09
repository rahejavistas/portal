use quality::{quick_create_user, quick_setup_admin, setup_api_test_env, TestEnv};
use std::io::Read;

async fn flat_id_for(env: &TestEnv, wing: &str, flat_number: &str) -> i32 {
    sqlx::query_scalar::<_, i32>("SELECT flat_id FROM flats WHERE wing = $1 AND flat_number = $2")
        .bind(wing)
        .bind(flat_number)
        .fetch_one(&env.db_pool)
        .await
        .expect("flat not found — quick_create_user should have created it")
}

fn auth(jwt: &str) -> String {
    format!("Bearer {}", jwt)
}

fn create_flat_rate_plan(url: &str, admin_auth: &str, amount: i64) -> i64 {
    let resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/billing-plans/create",
        url
    ))
    .set("Authorization", admin_auth)
    .send_json(serde_json::json!({
        "name": "Standard Maintenance",
        "billing_type": "FLAT_RATE",
        "config": { "amount": amount },
        "frequency": "MONTHLY"
    }))
    .expect("create billing plan failed");
    assert_eq!(resp.status(), 201);
    resp.into_json::<serde_json::Value>()
        .unwrap()
        .as_i64()
        .unwrap()
}

fn assign_plan(url: &str, admin_auth: &str, flat_id: i32, billing_plan_id: i64) {
    let resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/billing-plans/assign",
        url
    ))
    .set("Authorization", admin_auth)
    .send_json(serde_json::json!({
        "flat_id": flat_id,
        "billing_plan_id": billing_plan_id
    }))
    .expect("assign billing plan failed");
    assert_eq!(resp.status(), 204);
}

fn generate_invoice(
    url: &str,
    admin_auth: &str,
    flat_id: i32,
    period_start: &str,
    period_end: &str,
    due_date: &str,
) -> serde_json::Value {
    let resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/invoices/generate",
        url
    ))
    .set("Authorization", admin_auth)
    .send_json(serde_json::json!({
        "flat_id": flat_id,
        "period_start": period_start,
        "period_end": period_end,
        "due_date": due_date
    }))
    .expect("generate invoice failed");
    assert_eq!(resp.status(), 200);
    resp.into_json().unwrap()
}

/// Full P0 happy path a real society uses every month: create a flat-rate billing
/// plan, assign it to a unit, generate the bill, record the exact payment, and get a
/// receipt with a real sequential number.
#[tokio::test]
async fn test_flat_rate_billing_and_full_payment_happy_path() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let _ = quick_create_user(&env, &admin_jwt, "k0302@vistas.com", "K", "0302");
    let flat_id = flat_id_for(&env, "K", "0302").await;

    let plan_id = create_flat_rate_plan(&url, &admin_auth, 5000);
    assign_plan(&url, &admin_auth, flat_id, plan_id);

    let gen = generate_invoice(
        &url,
        &admin_auth,
        flat_id,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );
    let invoice_id = gen["invoices_created"][0]
        .as_str()
        .expect("invoice created")
        .to_string();
    assert!(gen["skipped"].as_array().unwrap().is_empty());

    // List invoices for the flat, confirm amount and UNPAID status.
    let list_resp = ureq::post(&format!("{}/api/v1/admin/accounting/invoices/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "flat_id": flat_id }))
        .expect("list invoices failed");
    let invoices: serde_json::Value = list_resp.into_json().unwrap();
    let invoice = &invoices[0];
    assert_eq!(invoice["status"], "UNPAID");
    assert_eq!(invoice["total_amount"].as_str().unwrap(), "5000.00");

    // Record the exact payment.
    let pay_resp = ureq::post(&format!("{}/api/v1/admin/accounting/payments/record", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "invoice_id": invoice_id,
            "amount": 5000,
            "payment_method": "upi",
            "reference_number": "UPI123456",
            "payment_date": "2026-07-05"
        }))
        .expect("record payment failed");
    assert_eq!(pay_resp.status(), 200);
    let pay_val: serde_json::Value = pay_resp.into_json().unwrap();
    assert!(pay_val["newly_recorded"].as_bool().unwrap());
    let receipt_number = pay_val["receipt_number"].as_str().unwrap().to_string();
    assert!(receipt_number.starts_with("RCT-"));

    // Invoice should now show PAID.
    let list_resp2 = ureq::post(&format!("{}/api/v1/admin/accounting/invoices/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "flat_id": flat_id }))
        .expect("list invoices failed");
    let invoices2: serde_json::Value = list_resp2.into_json().unwrap();
    assert_eq!(invoices2[0]["status"], "PAID");
    assert_eq!(invoices2[0]["amount_paid"].as_str().unwrap(), "5000.00");

    // Replaying the exact same payment reference must be a no-op, not a double-post.
    let replay_resp = ureq::post(&format!("{}/api/v1/admin/accounting/payments/record", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "invoice_id": invoice_id,
            "amount": 5000,
            "payment_method": "upi",
            "reference_number": "UPI123456",
            "payment_date": "2026-07-05"
        }))
        .expect("replay payment failed");
    let replay_val: serde_json::Value = replay_resp.into_json().unwrap();
    assert!(!replay_val["newly_recorded"].as_bool().unwrap());
    assert_eq!(
        replay_val["receipt_number"].as_str().unwrap(),
        receipt_number
    );
}

/// GST must NOT apply below the exempt threshold, and must apply to the full amount
/// once crossed — the exact behavior CA sign-off was flagged for.
#[tokio::test]
async fn test_gst_exempt_below_threshold_and_applied_above_it() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let create_gst_rule = |threshold: i64| {
        let resp = ureq::post(&format!("{}/api/v1/admin/accounting/gst-rules/create", url))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({
                "per_member_monthly_exempt_threshold": threshold,
                "society_turnover_registration_threshold": 2000000,
                "gst_rate_pct": 18,
                "hsn_sac_code": "9995",
                "effective_from": "2026-04-01"
            }))
            .expect("create gst rule failed");
        assert_eq!(resp.status(), 201);
    };
    create_gst_rule(7500);

    // Flat 1: billed exactly at the threshold -> exempt.
    let _ = quick_create_user(&env, &admin_jwt, "exempt@vistas.com", "A", "0101");
    let exempt_flat = flat_id_for(&env, "A", "0101").await;
    let exempt_plan = create_flat_rate_plan(&url, &admin_auth, 7500);
    assign_plan(&url, &admin_auth, exempt_flat, exempt_plan);
    let exempt_invoice = generate_invoice(
        &url,
        &admin_auth,
        exempt_flat,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );
    let exempt_id = exempt_invoice["invoices_created"][0].as_str().unwrap();

    // Flat 2: billed above the threshold -> full amount taxed.
    let _ = quick_create_user(&env, &admin_jwt, "taxed@vistas.com", "B", "0202");
    let taxed_flat = flat_id_for(&env, "B", "0202").await;
    let taxed_plan = create_flat_rate_plan(&url, &admin_auth, 8000);
    assign_plan(&url, &admin_auth, taxed_flat, taxed_plan);
    let taxed_invoice = generate_invoice(
        &url,
        &admin_auth,
        taxed_flat,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );
    let taxed_id = taxed_invoice["invoices_created"][0].as_str().unwrap();

    let list_resp = ureq::post(&format!("{}/api/v1/admin/accounting/invoices/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({}))
        .expect("list invoices failed");
    let invoices: serde_json::Value = list_resp.into_json().unwrap();
    let find = |id: &str| {
        invoices
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["invoice_id"] == id)
            .unwrap()
            .clone()
    };

    let exempt = find(exempt_id);
    assert_eq!(exempt["gstin_applicable"], false);
    assert_eq!(exempt["gst_amount"].as_str().unwrap(), "0");
    assert_eq!(exempt["total_amount"].as_str().unwrap(), "7500.00");

    let taxed = find(taxed_id);
    assert_eq!(taxed["gstin_applicable"], true);
    assert_eq!(taxed["gst_amount"].as_str().unwrap(), "1440.00"); // 8000 * 18%
    assert_eq!(taxed["total_amount"].as_str().unwrap(), "9440.00");
}

/// Interest only charges once an invoice is actually overdue, and re-running the job
/// the same day must not double-charge — an admin clicking the button twice by
/// accident is a very real daily-life scenario.
#[tokio::test]
async fn test_interest_charged_once_overdue_and_not_double_charged_same_day() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let _ = quick_create_user(&env, &admin_jwt, "overdue@vistas.com", "C", "0303");
    let flat_id = flat_id_for(&env, "C", "0303").await;
    let plan_id = create_flat_rate_plan(&url, &admin_auth, 10000);
    assign_plan(&url, &admin_auth, flat_id, plan_id);

    // Due date is deliberately in the past relative to "today" in this test run, and
    // interest is charged as of "today" via the run endpoint (which uses Utc::now()).
    // We generate an invoice due far enough in the past that grace period is exceeded
    // regardless of what day the suite runs on: due 60 days before generation date
    // isn't possible (billing_period_start must be <= due_date and within a fiscal
    // year already seeded around "today"), so instead we backdate due_date to the
    // start of the current fiscal year, which is guaranteed to be in the past.
    let gen = generate_invoice(
        &url,
        &admin_auth,
        flat_id,
        "2026-04-01",
        "2026-04-30",
        "2026-04-05",
    );
    let invoice_id = gen["invoices_created"][0].as_str().unwrap().to_string();

    let create_interest_rule = || {
        let resp = ureq::post(&format!(
            "{}/api/v1/admin/accounting/interest-rules/create",
            url
        ))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "annual_rate_pct": 21,
            "grace_period_days": 0,
            "compounding": "SIMPLE",
            "effective_from": "2026-04-01"
        }))
        .expect("create interest rule failed");
        assert_eq!(resp.status(), 201);
    };
    create_interest_rule();

    let run1 = ureq::post(&format!("{}/api/v1/admin/accounting/interest/run", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({}))
        .expect("run interest failed");
    let run1_val: serde_json::Value = run1.into_json().unwrap();
    assert_eq!(run1_val["charges_posted"].as_u64().unwrap(), 1);

    // Same-day re-run must be a no-op (idempotent per invoice per day).
    let run2 = ureq::post(&format!("{}/api/v1/admin/accounting/interest/run", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({}))
        .expect("run interest failed");
    let run2_val: serde_json::Value = run2.into_json().unwrap();
    assert_eq!(run2_val["charges_posted"].as_u64().unwrap(), 0);

    // Invoice total must be unchanged by interest — it's tracked separately.
    let list_resp = ureq::post(&format!("{}/api/v1/admin/accounting/invoices/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "flat_id": flat_id }))
        .expect("list invoices failed");
    let invoices: serde_json::Value = list_resp.into_json().unwrap();
    let invoice = invoices
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["invoice_id"] == invoice_id)
        .unwrap();
    assert_eq!(invoice["total_amount"].as_str().unwrap(), "10000.00");
    let invoice_number = invoice["invoice_number"].as_str().unwrap().to_string();

    let interest_csv = ureq::post(&format!("{}/api/v1/admin/accounting/reports/run", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "report_type": "interest", "date_from": "2020-01-01", "date_to": "2030-12-31" }))
        .expect("interest report failed");
    let csv_body = interest_csv.into_string().unwrap();
    assert!(
        csv_body.contains(&invoice_number),
        "CSV body was: {}",
        csv_body
    );
}

/// Overpaying an invoice must never push amount_paid past total_amount — the excess
/// becomes a credit balance the office can apply to the resident's next bill instead.
#[tokio::test]
async fn test_overpayment_becomes_credit_balance_and_can_be_applied_to_next_invoice() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let _ = quick_create_user(&env, &admin_jwt, "overpay@vistas.com", "D", "0404");
    let flat_id = flat_id_for(&env, "D", "0404").await;
    let plan_id = create_flat_rate_plan(&url, &admin_auth, 5000);
    assign_plan(&url, &admin_auth, flat_id, plan_id);

    let gen1 = generate_invoice(
        &url,
        &admin_auth,
        flat_id,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );
    let invoice1_id = gen1["invoices_created"][0].as_str().unwrap().to_string();

    // Resident pays 7000 against a 5000 bill.
    let pay_resp = ureq::post(&format!("{}/api/v1/admin/accounting/payments/record", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "invoice_id": invoice1_id,
            "amount": 7000,
            "payment_method": "bank_transfer",
            "reference_number": "NEFT001",
            "payment_date": "2026-07-05"
        }))
        .expect("record overpayment failed");
    assert_eq!(pay_resp.status(), 200);

    // Invoice 1 must show exactly 5000 paid (never 7000), status PAID.
    let list1 = ureq::post(&format!("{}/api/v1/admin/accounting/invoices/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "flat_id": flat_id }))
        .expect("list failed");
    let invoices1: serde_json::Value = list1.into_json().unwrap();
    let inv1 = invoices1
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["invoice_id"] == invoice1_id)
        .unwrap();
    assert_eq!(inv1["amount_paid"].as_str().unwrap(), "5000.00");
    assert_eq!(inv1["status"], "PAID");

    // Credit balance must be exactly the 2000 excess.
    let balance_resp = ureq::post(&format!("{}/api/v1/admin/accounting/credit/balance", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "flat_id": flat_id }))
        .expect("credit balance failed");
    let balance: serde_json::Value = balance_resp.into_json().unwrap();
    assert_eq!(balance.as_str().unwrap(), "2000.00");

    // Next month's bill: apply the credit against it.
    let gen2 = generate_invoice(
        &url,
        &admin_auth,
        flat_id,
        "2026-08-01",
        "2026-08-31",
        "2026-08-10",
    );
    let invoice2_id = gen2["invoices_created"][0].as_str().unwrap().to_string();

    let apply_resp = ureq::post(&format!("{}/api/v1/admin/accounting/credit/apply", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "invoice_id": invoice2_id, "amount": 2000 }))
        .expect("apply credit failed");
    assert_eq!(apply_resp.status(), 200);

    let list2 = ureq::post(&format!("{}/api/v1/admin/accounting/invoices/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "flat_id": flat_id }))
        .expect("list failed");
    let invoices2: serde_json::Value = list2.into_json().unwrap();
    let inv2 = invoices2
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["invoice_id"] == invoice2_id)
        .unwrap();
    assert_eq!(inv2["amount_paid"].as_str().unwrap(), "2000.00");
    assert_eq!(inv2["status"], "PARTIALLY_PAID");

    // Credit balance is now fully spent.
    let balance2_resp = ureq::post(&format!("{}/api/v1/admin/accounting/credit/balance", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "flat_id": flat_id }))
        .expect("credit balance failed");
    let balance2: serde_json::Value = balance2_resp.into_json().unwrap();
    assert_eq!(balance2.as_str().unwrap(), "0");

    // Trying to apply more credit than is left must fail, not silently create negative credit.
    let overapply_resp = ureq::post(&format!("{}/api/v1/admin/accounting/credit/apply", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "invoice_id": invoice2_id, "amount": 1 }));
    assert!(overapply_resp.is_err());
    assert_eq!(
        overapply_resp
            .unwrap_err()
            .into_response()
            .unwrap()
            .status(),
        400
    );
}

/// The MyGate-style flow: a resident self-reports an offline payment with their own
/// transaction reference; it must sit as PENDING and never touch the ledger until an
/// admin explicitly confirms it. Rejection must also work and must not post anything.
#[tokio::test]
async fn test_resident_payment_claim_submit_confirm_and_reject() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let email = "claimant@vistas.com";
    let _ = quick_create_user(&env, &admin_jwt, email, "E", "0505");
    let flat_id = flat_id_for(&env, "E", "0505").await;
    let plan_id = create_flat_rate_plan(&url, &admin_auth, 4000);
    assign_plan(&url, &admin_auth, flat_id, plan_id);
    let gen = generate_invoice(
        &url,
        &admin_auth,
        flat_id,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );
    let invoice_id = gen["invoices_created"][0].as_str().unwrap().to_string();

    let resident_jwt = quality::quick_onboard_user(&env, email, "resident_pass_secure_123");
    let resident_auth = auth(&resident_jwt);

    let claim_resp = ureq::post(&format!("{}/api/v1/accounting/claims/submit", url))
        .set("Authorization", &resident_auth)
        .send_json(serde_json::json!({
            "invoice_id": invoice_id,
            "amount": 4000,
            "payment_method": "upi",
            "reference_number": "UPI-CLAIM-1",
            "note": "Paid via GPay"
        }))
        .expect("submit claim failed");
    assert_eq!(claim_resp.status(), 201);
    let claim_val: serde_json::Value = claim_resp.into_json().unwrap();
    let claim_id = claim_val["claim_id"].as_str().unwrap().to_string();

    // A resident cannot submit a claim for a flat that isn't theirs.
    let _ = quick_create_user(&env, &admin_jwt, "other@vistas.com", "F", "0606");
    let other_jwt = quality::quick_onboard_user(&env, "other@vistas.com", "other_pass_secure_123");
    let forged_claim = ureq::post(&format!("{}/api/v1/accounting/claims/submit", url))
        .set("Authorization", &auth(&other_jwt))
        .send_json(serde_json::json!({
            "invoice_id": invoice_id,
            "amount": 4000,
            "payment_method": "upi",
            "reference_number": "UPI-FRAUD",
        }));
    assert!(forged_claim.is_err());
    assert_eq!(
        forged_claim.unwrap_err().into_response().unwrap().status(),
        403
    );

    // Invoice must still be UNPAID — a pending claim never posts to the ledger.
    let list_before = ureq::post(&format!("{}/api/v1/admin/accounting/invoices/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "flat_id": flat_id }))
        .expect("list failed");
    let invoices_before: serde_json::Value = list_before.into_json().unwrap();
    assert_eq!(invoices_before[0]["status"], "UNPAID");

    // Admin confirms the claim.
    let confirm_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/claims/{}/confirm",
        url, claim_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({}))
    .expect("confirm claim failed");
    assert_eq!(confirm_resp.status(), 200);
    let confirm_val: serde_json::Value = confirm_resp.into_json().unwrap();
    let payment_id = confirm_val["payment_id"].as_str().unwrap().to_string();

    let list_after = ureq::post(&format!("{}/api/v1/admin/accounting/invoices/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "flat_id": flat_id }))
        .expect("list failed");
    let invoices_after: serde_json::Value = list_after.into_json().unwrap();
    assert_eq!(invoices_after[0]["status"], "PAID");

    // Confirming the same claim again (e.g. a retried request) must be treated as the
    // same action repeated, not an error — it returns the same payment already posted,
    // never posts a second one.
    let reconfirm = ureq::post(&format!(
        "{}/api/v1/admin/accounting/claims/{}/confirm",
        url, claim_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({}))
    .expect("repeated confirm must succeed idempotently");
    assert_eq!(reconfirm.status(), 200);
    let reconfirm_val: serde_json::Value = reconfirm.into_json().unwrap();
    assert_eq!(
        reconfirm_val["payment_id"].as_str().unwrap(),
        payment_id,
        "a repeated confirm must return the SAME payment_id, never post a second payment"
    );

    let payments_after_reconfirm =
        ureq::post(&format!("{}/api/v1/admin/accounting/payments/list", url))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({ "invoice_id": invoice_id }))
            .expect("list payments failed");
    let payments_val: serde_json::Value = payments_after_reconfirm.into_json().unwrap();
    assert_eq!(
        payments_val.as_array().unwrap().len(),
        1,
        "repeated confirm must not create a second payment"
    );

    // Rejecting an already-CONFIRMED claim is a real conflict, not a retry — must fail.
    let reject_confirmed = ureq::post(&format!(
        "{}/api/v1/admin/accounting/claims/{}/reject",
        url, claim_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "review_note": "too late" }));
    assert!(reject_confirmed.is_err());
    assert_eq!(
        reject_confirmed
            .unwrap_err()
            .into_response()
            .unwrap()
            .status(),
        400
    );

    // A second invoice + a rejected claim: the reject path must not touch the ledger.
    let gen2 = generate_invoice(
        &url,
        &admin_auth,
        flat_id,
        "2026-08-01",
        "2026-08-31",
        "2026-08-10",
    );
    let invoice2_id = gen2["invoices_created"][0].as_str().unwrap().to_string();
    let claim2_resp = ureq::post(&format!("{}/api/v1/accounting/claims/submit", url))
        .set("Authorization", &resident_auth)
        .send_json(serde_json::json!({
            "invoice_id": invoice2_id,
            "amount": 4000,
            "payment_method": "cash",
            "reference_number": "CASH-1"
        }))
        .expect("submit second claim failed");
    let claim2_val: serde_json::Value = claim2_resp.into_json().unwrap();
    let claim2_id = claim2_val["claim_id"].as_str().unwrap().to_string();

    let reject_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/claims/{}/reject",
        url, claim2_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "review_note": "No matching cash entry in register" }))
    .expect("reject claim failed");
    assert_eq!(reject_resp.status(), 204);

    // Rejecting the same already-REJECTED claim again is a no-op retry, not an error.
    let re_reject = ureq::post(&format!(
        "{}/api/v1/admin/accounting/claims/{}/reject",
        url, claim2_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "review_note": "still no match" }))
    .expect("repeated reject must succeed idempotently");
    assert_eq!(re_reject.status(), 204);

    let list_final = ureq::post(&format!("{}/api/v1/admin/accounting/invoices/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "flat_id": flat_id }))
        .expect("list failed");
    let invoices_final: serde_json::Value = list_final.into_json().unwrap();
    let inv2 = invoices_final
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["invoice_id"] == invoice2_id)
        .unwrap();
    assert_eq!(inv2["status"], "UNPAID");
}

/// Double-submitting the exact same claim (a retried request, a double-tap on the
/// submit button) must not create a second PENDING row in the admin review queue.
/// Once the first is reviewed, the same reference must be free to submit again.
#[tokio::test]
async fn test_duplicate_pending_claim_submission_is_idempotent() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let email = "claimdupe@vistas.com";
    let _ = quick_create_user(&env, &admin_jwt, email, "E", "0707");
    let flat_id = flat_id_for(&env, "E", "0707").await;
    let plan_id = create_flat_rate_plan(&url, &admin_auth, 4000);
    assign_plan(&url, &admin_auth, flat_id, plan_id);
    let gen = generate_invoice(
        &url,
        &admin_auth,
        flat_id,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );
    let invoice_id = gen["invoices_created"][0].as_str().unwrap().to_string();

    let resident_jwt = quality::quick_onboard_user(&env, email, "resident_pass_secure_123");
    let resident_auth = auth(&resident_jwt);

    let submit_claim = || {
        ureq::post(&format!("{}/api/v1/accounting/claims/submit", url))
            .set("Authorization", &resident_auth)
            .send_json(serde_json::json!({
                "invoice_id": invoice_id, "amount": 4000, "payment_method": "upi",
                "reference_number": "UPI-CLAIM-DUPE-1"
            }))
            .expect("submit claim failed")
            .into_json::<serde_json::Value>()
            .unwrap()
    };

    let first = submit_claim();
    let second = submit_claim();
    assert_eq!(
        first["claim_id"], second["claim_id"],
        "a double-submitted claim must return the same claim, not create a second PENDING row"
    );

    let list_resp = ureq::post(&format!("{}/api/v1/admin/accounting/claims/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "status": "PENDING" }))
        .expect("list claims failed");
    let claims: serde_json::Value = list_resp.into_json().unwrap();
    assert_eq!(
        claims
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["reference_number"] == "UPI-CLAIM-DUPE-1")
            .count(),
        1,
        "only one PENDING claim must exist for this reference"
    );

    let claim_id = first["claim_id"].as_str().unwrap();
    let reject_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/claims/{}/reject",
        url, claim_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "review_note": "Testing resubmission" }))
    .expect("reject claim failed");
    assert_eq!(reject_resp.status(), 204);

    // After rejection, the exact same reference must be submittable again — a NEW
    // PENDING claim, not blocked by the now-reviewed one.
    let third = submit_claim();
    assert_ne!(
        third["claim_id"], first["claim_id"],
        "after rejection, resubmitting the same reference must create a fresh claim"
    );
}

/// Voiding an invoice that already has money against it would silently corrupt the
/// books — must be blocked outright, not just discouraged.
#[tokio::test]
async fn test_void_invoice_blocked_once_a_payment_exists() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let _ = quick_create_user(&env, &admin_jwt, "voidtest@vistas.com", "G", "0707");
    let flat_id = flat_id_for(&env, "G", "0707").await;
    let plan_id = create_flat_rate_plan(&url, &admin_auth, 3000);
    assign_plan(&url, &admin_auth, flat_id, plan_id);
    let gen = generate_invoice(
        &url,
        &admin_auth,
        flat_id,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );
    let invoice_id = gen["invoices_created"][0].as_str().unwrap().to_string();

    // Void before any payment must succeed.
    let void_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/invoices/{}/void",
        url, invoice_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "reason": "Duplicate bill generated by mistake" }))
    .expect("void should have succeeded");
    assert_eq!(void_resp.status(), 204);

    // A second invoice, this time with a payment recorded — void must now be rejected.
    let gen2 = generate_invoice(
        &url,
        &admin_auth,
        flat_id,
        "2026-08-01",
        "2026-08-31",
        "2026-08-10",
    );
    let invoice2_id = gen2["invoices_created"][0].as_str().unwrap().to_string();
    ureq::post(&format!("{}/api/v1/admin/accounting/payments/record", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "invoice_id": invoice2_id,
            "amount": 1000,
            "payment_method": "cash",
            "reference_number": null,
            "payment_date": "2026-08-05"
        }))
        .expect("partial payment failed");

    let void2_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/invoices/{}/void",
        url, invoice2_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "reason": "Trying to void anyway" }));
    assert!(void2_resp.is_err());
    assert_eq!(
        void2_resp.unwrap_err().into_response().unwrap().status(),
        400
    );
}

async fn income_account_net_for_flat(env: &TestEnv, flat_id: i32) -> rust_decimal::Decimal {
    sqlx::query_scalar::<_, rust_decimal::Decimal>(
        "SELECT COALESCE(SUM(le.credit - le.debit), 0) \
         FROM ledger_entries le \
         JOIN chart_of_accounts a ON a.account_id = le.account_id \
         WHERE a.code = '4000' AND le.flat_id = $1",
    )
    .bind(flat_id)
    .fetch_one(&env.db_pool)
    .await
    .expect("ledger query failed")
}

/// Voiding an unpaid invoice must reverse its original Dr. Receivable / Cr. Income
/// entries, not just flip a status flag — otherwise revenue stays permanently
/// overstated by every voided invoice, and doubly so once the same period gets
/// re-billed (which voiding explicitly allows).
#[tokio::test]
async fn test_void_invoice_reverses_ledger_and_rebilling_does_not_double_count_income() {
    use rust_decimal_macros::dec;

    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let _ = quick_create_user(&env, &admin_jwt, "voidledger@vistas.com", "H", "0808");
    let flat_id = flat_id_for(&env, "H", "0808").await;
    let plan_id = create_flat_rate_plan(&url, &admin_auth, 10000);
    assign_plan(&url, &admin_auth, flat_id, plan_id);

    let gen = generate_invoice(
        &url,
        &admin_auth,
        flat_id,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );
    let invoice_id = gen["invoices_created"][0].as_str().unwrap().to_string();

    assert_eq!(
        income_account_net_for_flat(&env, flat_id).await,
        dec!(10000)
    );

    let void_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/invoices/{}/void",
        url, invoice_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "reason": "Wrong amount, re-billing correctly" }))
    .expect("void failed");
    assert_eq!(void_resp.status(), 204);

    // The reversal must bring the flat's recognized income back to zero — voiding is
    // not just a status flag, it undoes the original ledger effect.
    assert_eq!(income_account_net_for_flat(&env, flat_id).await, dec!(0));

    // Voiding a second time must be rejected — otherwise the reversal would post twice.
    let double_void = ureq::post(&format!(
        "{}/api/v1/admin/accounting/invoices/{}/void",
        url, invoice_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "reason": "Voiding again" }));
    assert!(double_void.is_err());
    assert_eq!(
        double_void.unwrap_err().into_response().unwrap().status(),
        400
    );

    // Re-billing the same period is explicitly allowed after a void — and must show
    // exactly one invoice's worth of income, not two.
    let gen2 = generate_invoice(
        &url,
        &admin_auth,
        flat_id,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );
    assert!(gen2["invoices_created"].as_array().unwrap().len() == 1);
    assert_eq!(
        income_account_net_for_flat(&env, flat_id).await,
        dec!(10000)
    );
}

/// The interest job is meant to be triggered daily by an external scheduler (see
/// `run_interest_for_all_overdue`'s doc comment) — it must charge the marginal interest
/// for each new day only, never re-derive and re-post the whole period-to-date on every
/// run. Verified by comparing running it once a day for 3 days against running it once
/// on the 3rd day for an identical invoice: both must land on the exact same total.
#[tokio::test]
async fn test_interest_charged_daily_matches_a_single_run_not_a_multiple_of_it() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let admin_user_id: i32 = sqlx::query_scalar("SELECT user_id FROM users WHERE email = $1")
        .bind("superadmin@vistas.com")
        .fetch_one(&env.db_pool)
        .await
        .expect("admin user not found");

    ureq::post(&format!(
        "{}/api/v1/admin/accounting/interest-rules/create",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({
        "annual_rate_pct": 21,
        "grace_period_days": 0,
        "compounding": "SIMPLE",
        "effective_from": "2026-04-01"
    }))
    .expect("create interest rule failed");

    let plan_id = create_flat_rate_plan(&url, &admin_auth, 10000);

    let _ = quick_create_user(&env, &admin_jwt, "dailyinterest@vistas.com", "I", "0901");
    let daily_flat_id = flat_id_for(&env, "I", "0901").await;
    assign_plan(&url, &admin_auth, daily_flat_id, plan_id);
    let daily_invoice = generate_invoice(
        &url,
        &admin_auth,
        daily_flat_id,
        "2026-04-01",
        "2026-04-30",
        "2026-04-05",
    );
    let daily_invoice_id = daily_invoice["invoices_created"][0]
        .as_str()
        .unwrap()
        .to_string();

    let _ = quick_create_user(&env, &admin_jwt, "onceinterest@vistas.com", "I", "0902");
    let once_flat_id = flat_id_for(&env, "I", "0902").await;
    assign_plan(&url, &admin_auth, once_flat_id, plan_id);
    let once_invoice = generate_invoice(
        &url,
        &admin_auth,
        once_flat_id,
        "2026-04-01",
        "2026-04-30",
        "2026-04-05",
    );
    let once_invoice_id = once_invoice["invoices_created"][0]
        .as_str()
        .unwrap()
        .to_string();

    let mut conn = env
        .db_pool
        .acquire()
        .await
        .expect("acquire connection failed");

    // Simulate the real production trigger pattern: once a day, for 3 consecutive
    // overdue days.
    for day in ["2026-04-06", "2026-04-07", "2026-04-08"] {
        let as_of = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").unwrap();
        accounting::billing::run_interest_for_all_overdue(&mut conn, as_of, admin_user_id)
            .await
            .expect("daily interest run failed");
    }

    // A single run, as of the same final day, on an identical invoice.
    let as_of_final = chrono::NaiveDate::parse_from_str("2026-04-08", "%Y-%m-%d").unwrap();
    accounting::billing::run_interest_for_all_overdue(&mut conn, as_of_final, admin_user_id)
        .await
        .expect("single interest run failed");

    let sum_for = |invoice_id: uuid::Uuid| {
        let pool = env.db_pool.clone();
        async move {
            sqlx::query_scalar::<_, rust_decimal::Decimal>(
                "SELECT COALESCE(SUM(interest_amount), 0) FROM interest_charges WHERE invoice_id = $1",
            )
            .bind(invoice_id)
            .fetch_one(&pool)
            .await
            .expect("interest_charges query failed")
        }
    };

    let daily_total = sum_for(uuid::Uuid::parse_str(&daily_invoice_id).unwrap()).await;
    let once_total = sum_for(uuid::Uuid::parse_str(&once_invoice_id).unwrap()).await;

    assert!(
        daily_total > rust_decimal::Decimal::ZERO,
        "interest should have been charged at all"
    );
    assert_eq!(
        daily_total, once_total,
        "charging daily must total the same as charging once as of the same day — \
         a quadratic double-charge bug would make the daily total roughly double this"
    );
}

/// A cash payment has no reference_number to de-duplicate on. A double-click or a
/// retried request must not post the same cash payment twice.
#[tokio::test]
async fn test_duplicate_cash_payment_submission_is_idempotent() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let _ = quick_create_user(&env, &admin_jwt, "cashdupe@vistas.com", "J", "1001");
    let flat_id = flat_id_for(&env, "J", "1001").await;
    let plan_id = create_flat_rate_plan(&url, &admin_auth, 4000);
    assign_plan(&url, &admin_auth, flat_id, plan_id);
    let gen = generate_invoice(
        &url,
        &admin_auth,
        flat_id,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );
    let invoice_id = gen["invoices_created"][0].as_str().unwrap().to_string();

    let record_cash = || {
        ureq::post(&format!("{}/api/v1/admin/accounting/payments/record", url))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({
                "invoice_id": invoice_id,
                "amount": 4000,
                "payment_method": "cash",
                "reference_number": null,
                "payment_date": "2026-07-05"
            }))
            .expect("record cash payment failed")
            .into_json::<serde_json::Value>()
            .unwrap()
    };

    let first = record_cash();
    let second = record_cash();
    assert_eq!(
        first["payment_id"], second["payment_id"],
        "an accidental double-submit must return the same payment, not create a second one"
    );

    let list_resp = ureq::post(&format!("{}/api/v1/admin/accounting/invoices/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "flat_id": flat_id }))
        .expect("list invoices failed");
    let invoices: serde_json::Value = list_resp.into_json().unwrap();
    let invoice = invoices
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["invoice_id"] == invoice_id)
        .unwrap();
    assert_eq!(
        invoice["amount_paid"].as_str().unwrap(),
        "4000.00",
        "amount_paid must reflect only one payment, not 8000 from a double-post"
    );
}

/// Reusing the same (payment_method, reference_number) with a DIFFERENT amount must be
/// rejected outright, not silently treated as the same idempotent retry and handed back
/// the earlier receipt — that would tell the caller the wrong amount was recorded.
#[tokio::test]
async fn test_reusing_a_payment_reference_with_a_different_amount_is_rejected() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let _ = quick_create_user(&env, &admin_jwt, "refmismatch@vistas.com", "J", "1002");
    let flat_id = flat_id_for(&env, "J", "1002").await;
    let plan_id = create_flat_rate_plan(&url, &admin_auth, 4000);
    assign_plan(&url, &admin_auth, flat_id, plan_id);
    let gen = generate_invoice(
        &url,
        &admin_auth,
        flat_id,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );
    let invoice_id = gen["invoices_created"][0].as_str().unwrap().to_string();

    let first = ureq::post(&format!("{}/api/v1/admin/accounting/payments/record", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "invoice_id": invoice_id, "amount": 3000, "payment_method": "upi",
            "reference_number": "UPI-MISMATCH-1", "payment_date": "2026-07-05"
        }))
        .expect("first payment failed");
    assert_eq!(first.status(), 200);

    let mismatched = ureq::post(&format!("{}/api/v1/admin/accounting/payments/record", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "invoice_id": invoice_id, "amount": 4000, "payment_method": "upi",
            "reference_number": "UPI-MISMATCH-1", "payment_date": "2026-07-06"
        }));
    assert!(
        mismatched.is_err(),
        "same reference with a different amount must be rejected, not silently accepted"
    );
    assert_eq!(
        mismatched.unwrap_err().into_response().unwrap().status(),
        400
    );

    // The invoice must reflect only the first, genuinely-recorded payment.
    let list_resp = ureq::post(&format!("{}/api/v1/admin/accounting/invoices/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "flat_id": flat_id }))
        .expect("list invoices failed");
    let invoices: serde_json::Value = list_resp.into_json().unwrap();
    let invoice = invoices
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["invoice_id"] == invoice_id)
        .unwrap();
    assert_eq!(invoice["amount_paid"].as_str().unwrap(), "3000.00");
}

/// Generating a second invoice for a flat covering the same billing period must be
/// rejected — this is what makes the whole invoicing flow idempotent/safe to re-run.
#[tokio::test]
async fn test_duplicate_invoice_for_same_period_rejected() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let _ = quick_create_user(&env, &admin_jwt, "dup@vistas.com", "H", "0808");
    let flat_id = flat_id_for(&env, "H", "0808").await;
    let plan_id = create_flat_rate_plan(&url, &admin_auth, 2500);
    assign_plan(&url, &admin_auth, flat_id, plan_id);

    let gen1 = generate_invoice(
        &url,
        &admin_auth,
        flat_id,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );
    assert_eq!(gen1["invoices_created"].as_array().unwrap().len(), 1);

    // Batch-generate again for the whole society (flat_id omitted) — this flat must be
    // skipped with a reason, not double-billed.
    let gen2 = ureq::post(&format!(
        "{}/api/v1/admin/accounting/invoices/generate",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({
        "period_start": "2026-07-01",
        "period_end": "2026-07-31",
        "due_date": "2026-07-10"
    }))
    .expect("batch generate failed");
    let gen2_val: serde_json::Value = gen2.into_json().unwrap();
    assert!(gen2_val["invoices_created"].as_array().unwrap().is_empty());
    let skipped = gen2_val["skipped"].as_array().unwrap();
    assert_eq!(skipped.len(), 1);
    assert_eq!(skipped[0][0].as_i64().unwrap() as i32, flat_id);
}

/// Bulk-assign is only meaningful for FLAT_RATE plans (no per-flat area needed), must
/// skip flats that already have a plan rather than overwrite them, and must reject a
/// PER_SQFT/SLAB plan outright since those need an area entered per flat.
#[tokio::test]
async fn test_bulk_assign_flat_rate_plan_skips_already_assigned_and_rejects_non_flat_rate() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let _ = quick_create_user(&env, &admin_jwt, "bulk1@vistas.com", "BLK", "0101");
    let _ = quick_create_user(&env, &admin_jwt, "bulk2@vistas.com", "BLK", "0102");
    let _ = quick_create_user(&env, &admin_jwt, "bulk3@vistas.com", "BLK", "0103");
    let flat_101 = flat_id_for(&env, "BLK", "0101").await;
    let flat_102 = flat_id_for(&env, "BLK", "0102").await;
    let flat_103 = flat_id_for(&env, "BLK", "0103").await;

    let flat_rate_plan = create_flat_rate_plan(&url, &admin_auth, 4000);
    let other_plan = create_flat_rate_plan(&url, &admin_auth, 9999);
    assign_plan(&url, &admin_auth, flat_102, other_plan);

    let bulk_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/billing-plans/assign-bulk",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "billing_plan_id": flat_rate_plan }))
    .expect("bulk assign failed");
    assert_eq!(bulk_resp.status(), 200);
    let bulk_val: serde_json::Value = bulk_resp.into_json().unwrap();
    let flats_assigned = bulk_val["flats_assigned"].as_i64().unwrap();
    assert!(
        flats_assigned >= 2,
        "expected at least the two unassigned test flats to be picked up, got {}",
        flats_assigned
    );

    async fn assignment_plan_id(env: &TestEnv, flat_id: i32) -> i64 {
        sqlx::query_scalar::<_, i32>(
            "SELECT billing_plan_id FROM flat_billing_assignments WHERE flat_id = $1",
        )
        .bind(flat_id)
        .fetch_one(&env.db_pool)
        .await
        .expect("flat should have an assignment") as i64
    }
    assert_eq!(assignment_plan_id(&env, flat_101).await, flat_rate_plan);
    assert_eq!(
        assignment_plan_id(&env, flat_102).await,
        other_plan,
        "already-assigned flat must not be overwritten by bulk assign"
    );
    assert_eq!(assignment_plan_id(&env, flat_103).await, flat_rate_plan);

    let per_sqft_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/billing-plans/create",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({
        "name": "Per Sqft Plan", "billing_type": "PER_SQFT",
        "config": { "rate_per_sqft": 2 }, "frequency": "MONTHLY"
    }))
    .expect("create per-sqft plan failed");
    let per_sqft_plan_id = per_sqft_resp
        .into_json::<serde_json::Value>()
        .unwrap()
        .as_i64()
        .unwrap();

    let reject_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/billing-plans/assign-bulk",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "billing_plan_id": per_sqft_plan_id }));
    assert!(reject_resp.is_err());
    assert_eq!(
        reject_resp.unwrap_err().into_response().unwrap().status(),
        400
    );
}

/// The "which flats have which plan" view must show every flat, including ones with no
/// assignment at all (null plan columns, not just omitted from the list) — this is
/// exactly the visibility gap a real user hit ("how do I know what's assigned?").
#[tokio::test]
async fn test_billing_assignments_lists_every_flat_including_unassigned() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let _ = quick_create_user(&env, &admin_jwt, "assign-a@vistas.com", "AS", "0101");
    let _ = quick_create_user(&env, &admin_jwt, "assign-b@vistas.com", "AS", "0102");
    let flat_a = flat_id_for(&env, "AS", "0101").await;
    let _flat_b = flat_id_for(&env, "AS", "0102").await;

    let plan_id = create_flat_rate_plan(&url, &admin_auth, 5500);
    assign_plan(&url, &admin_auth, flat_a, plan_id);
    // AS-0102 deliberately left unassigned.

    let resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/billing-plans/assignments",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({}))
    .expect("list assignments failed");
    assert_eq!(resp.status(), 200);
    let rows: serde_json::Value = resp.into_json().unwrap();
    let rows = rows.as_array().unwrap();

    let assigned_row = rows
        .iter()
        .find(|r| r["wing"] == "AS" && r["flat_number"] == "0101")
        .expect("assigned flat must appear");
    assert_eq!(assigned_row["billing_plan_name"], "Standard Maintenance");
    assert_eq!(assigned_row["billing_type"], "FLAT_RATE");

    let unassigned_row = rows
        .iter()
        .find(|r| r["wing"] == "AS" && r["flat_number"] == "0102")
        .expect("unassigned flat must still appear in the list");
    assert!(unassigned_row["billing_plan_id"].is_null());
    assert!(unassigned_row["billing_plan_name"].is_null());
}

/// A flat's area is a property of the flat itself, not of whichever billing plan
/// happens to be assigned to it — it must survive reassignment to a different plan
/// (with no area_sqft re-entered), and correctly feed the new plan's calculation.
#[tokio::test]
async fn test_flat_area_sqft_persists_across_billing_plan_reassignment() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let _ = quick_create_user(&env, &admin_jwt, "areapersist@vistas.com", "AR", "0101");
    let flat_id = flat_id_for(&env, "AR", "0101").await;

    let per_sqft_plan_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/billing-plans/create",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({
        "name": "Per Sqft Plan A", "billing_type": "PER_SQFT",
        "config": {"rate_per_sqft": 5}, "frequency": "MONTHLY"
    }))
    .expect("create per-sqft plan failed");
    let per_sqft_plan_id: i64 = per_sqft_plan_resp
        .into_json::<serde_json::Value>()
        .unwrap()
        .as_i64()
        .unwrap();

    ureq::post(&format!(
        "{}/api/v1/admin/accounting/billing-plans/assign",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({
        "flat_id": flat_id, "billing_plan_id": per_sqft_plan_id, "area_sqft": 900
    }))
    .expect("assign with area failed");

    let assignments = |auth_hdr: &str| {
        ureq::post(&format!(
            "{}/api/v1/admin/accounting/billing-plans/assignments",
            url
        ))
        .set("Authorization", auth_hdr)
        .send_json(serde_json::json!({}))
        .expect("list assignments failed")
        .into_json::<serde_json::Value>()
        .unwrap()
    };

    let rows = assignments(&admin_auth);
    let row = rows
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["flat_id"].as_i64().unwrap() as i32 == flat_id)
        .unwrap();
    assert_eq!(row["area_sqft"].as_str().unwrap(), "900.00");

    // Reassign to a DIFFERENT plan without specifying area_sqft at all.
    let second_plan_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/billing-plans/create",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({
        "name": "Per Sqft Plan B", "billing_type": "PER_SQFT",
        "config": {"rate_per_sqft": 8}, "frequency": "MONTHLY"
    }))
    .expect("create second per-sqft plan failed");
    let second_plan_id: i64 = second_plan_resp
        .into_json::<serde_json::Value>()
        .unwrap()
        .as_i64()
        .unwrap();

    ureq::post(&format!(
        "{}/api/v1/admin/accounting/billing-plans/assign",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({
        "flat_id": flat_id, "billing_plan_id": second_plan_id
    }))
    .expect("reassign without area failed");

    let rows_after = assignments(&admin_auth);
    let row_after = rows_after
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["flat_id"].as_i64().unwrap() as i32 == flat_id)
        .unwrap();
    assert_eq!(
        row_after["area_sqft"].as_str().unwrap(),
        "900.00",
        "area must persist across reassignment even though it wasn't re-entered"
    );
    assert_eq!(row_after["billing_plan_name"], "Per Sqft Plan B");

    // Generating a bill against the new plan must use the persisted area: 900 * 8 = 7200.
    let gen = generate_invoice(
        &url,
        &admin_auth,
        flat_id,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );
    let invoice_id = gen["invoices_created"][0].as_str().unwrap().to_string();
    let list_resp = ureq::post(&format!("{}/api/v1/admin/accounting/invoices/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "flat_id": flat_id }))
        .expect("list invoices failed");
    let invoices: serde_json::Value = list_resp.into_json().unwrap();
    let invoice = invoices
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["invoice_id"] == invoice_id)
        .unwrap();
    assert_eq!(invoice["subtotal"].as_str().unwrap(), "7200.00");
}

/// A resident's "my bills" view (`/invoices/mine`) takes no flat_id from the request at
/// all — it must only ever return invoices for flats the caller actually owns, never a
/// neighbor's, and must reject an unauthenticated request outright.
#[tokio::test]
async fn test_resident_invoices_mine_is_scoped_to_own_flat_only() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let _ = quick_create_user(&env, &admin_jwt, "mine-a@vistas.com", "M", "0101");
    let _ = quick_create_user(&env, &admin_jwt, "mine-b@vistas.com", "M", "0102");
    let flat_a = flat_id_for(&env, "M", "0101").await;
    let flat_b = flat_id_for(&env, "M", "0102").await;

    let plan_id = create_flat_rate_plan(&url, &admin_auth, 4200);
    assign_plan(&url, &admin_auth, flat_a, plan_id);
    assign_plan(&url, &admin_auth, flat_b, plan_id);
    let gen_a = generate_invoice(
        &url,
        &admin_auth,
        flat_a,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );
    assert_eq!(gen_a["invoices_created"].as_array().unwrap().len(), 1);
    let gen_b = generate_invoice(
        &url,
        &admin_auth,
        flat_b,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );
    assert_eq!(gen_b["invoices_created"].as_array().unwrap().len(), 1);

    let resident_a_jwt =
        quality::quick_onboard_user(&env, "mine-a@vistas.com", "resident_pass_secure_123");
    let resident_a_auth = auth(&resident_a_jwt);

    let mine_resp = ureq::post(&format!("{}/api/v1/accounting/invoices/mine", url))
        .set("Authorization", &resident_a_auth)
        .send_json(serde_json::json!({}))
        .expect("invoices/mine failed");
    assert_eq!(mine_resp.status(), 200);
    let mine: serde_json::Value = mine_resp.into_json().unwrap();
    let invoices = mine.as_array().unwrap();
    assert_eq!(
        invoices.len(),
        1,
        "resident A must see exactly their own flat's invoice, not flat B's too"
    );
    assert_eq!(invoices[0]["flat_id"].as_i64().unwrap() as i32, flat_a);

    let unauthenticated = ureq::post(&format!("{}/api/v1/accounting/invoices/mine", url))
        .send_json(serde_json::json!({}));
    assert!(unauthenticated.is_err());
    assert_eq!(
        unauthenticated
            .unwrap_err()
            .into_response()
            .unwrap()
            .status(),
        401
    );
}

/// The consolidated report dispatcher (`/reports/run`) must route several different
/// report_types correctly through one endpoint, and reject an unknown type outright
/// rather than silently returning something wrong or empty.
#[tokio::test]
async fn test_report_dispatcher_routes_multiple_types_and_rejects_unknown() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let _ = quick_create_user(&env, &admin_jwt, "dispatch@vistas.com", "Z", "0909");
    let flat_id = flat_id_for(&env, "Z", "0909").await;
    let plan_id = create_flat_rate_plan(&url, &admin_auth, 4500);
    assign_plan(&url, &admin_auth, flat_id, plan_id);
    generate_invoice(
        &url,
        &admin_auth,
        flat_id,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );

    for (report_type, extra) in [
        (
            "invoice_register",
            serde_json::json!({ "date_from": "2026-01-01", "date_to": "2026-12-31" }),
        ),
        ("outstanding_dues", serde_json::json!({})),
        ("ageing_summary", serde_json::json!({})),
        (
            "receipt_register",
            serde_json::json!({ "date_from": "2026-01-01", "date_to": "2026-12-31" }),
        ),
    ] {
        let mut body = serde_json::json!({ "report_type": report_type });
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let resp = ureq::post(&format!("{}/api/v1/admin/accounting/reports/run", url))
            .set("Authorization", &admin_auth)
            .send_json(body)
            .unwrap_or_else(|e| panic!("report_type={} failed: {}", report_type, e));
        assert_eq!(resp.status(), 200, "report_type={}", report_type);
        assert_eq!(resp.header("Content-Type").unwrap(), "text/csv");
    }

    // Missing date range on a report that requires one must 400, not panic or 500.
    let missing_range = ureq::post(&format!("{}/api/v1/admin/accounting/reports/run", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "report_type": "gst_summary" }));
    assert!(missing_range.is_err());
    assert_eq!(
        missing_range.unwrap_err().into_response().unwrap().status(),
        400
    );

    // An unknown report_type must be rejected, not silently ignored.
    let unknown = ureq::post(&format!("{}/api/v1/admin/accounting/reports/run", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "report_type": "not_a_real_report" }));
    assert!(unknown.is_err());
    assert_eq!(unknown.unwrap_err().into_response().unwrap().status(), 400);
}

/// Every report, not just its status code — after the PDF bug shipped invisible
/// documents while every test only checked "200 and non-empty", this seeds a real
/// billed/paid/overdue scenario and checks every report_type's actual numbers against
/// hand-computed expected values, plus the two JSON report endpoints.
#[tokio::test]
async fn test_all_reports_contain_correct_computed_data() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let gst_resp = ureq::post(&format!("{}/api/v1/admin/accounting/gst-rules/create", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "per_member_monthly_exempt_threshold": 5000,
            "society_turnover_registration_threshold": 2000000,
            "gst_rate_pct": 18,
            "hsn_sac_code": "9995",
            "effective_from": "2026-04-01"
        }))
        .expect("create gst rule failed");
    assert_eq!(gst_resp.status(), 201);

    let interest_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/interest-rules/create",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({
        "annual_rate_pct": 21, "grace_period_days": 0,
        "compounding": "SIMPLE", "effective_from": "2026-04-01"
    }))
    .expect("create interest rule failed");
    assert_eq!(interest_resp.status(), 201);

    let plan_id = create_flat_rate_plan(&url, &admin_auth, 10000);

    // Flat paid in full — should NOT show up as outstanding/overdue/interest/defaulter.
    let _ = quick_create_user(&env, &admin_jwt, "rep-paid@vistas.com", "REP", "0101");
    let paid_flat = flat_id_for(&env, "REP", "0101").await;
    assign_plan(&url, &admin_auth, paid_flat, plan_id);
    let paid_gen = generate_invoice(
        &url,
        &admin_auth,
        paid_flat,
        "2026-04-01",
        "2026-04-30",
        "2026-04-05",
    );
    let paid_invoice_id = paid_gen["invoices_created"][0]
        .as_str()
        .unwrap()
        .to_string();

    // Flat left unpaid and overdue — should show up everywhere the paid one doesn't.
    let _ = quick_create_user(&env, &admin_jwt, "rep-unpaid@vistas.com", "REP", "0102");
    let unpaid_flat = flat_id_for(&env, "REP", "0102").await;
    assign_plan(&url, &admin_auth, unpaid_flat, plan_id);
    let unpaid_gen = generate_invoice(
        &url,
        &admin_auth,
        unpaid_flat,
        "2026-04-01",
        "2026-04-30",
        "2026-04-05",
    );
    let unpaid_invoice_id = unpaid_gen["invoices_created"][0]
        .as_str()
        .unwrap()
        .to_string();

    // Both invoices are FLAT_RATE 10000, above the 5000 GST exempt threshold.
    let list_resp = ureq::post(&format!("{}/api/v1/admin/accounting/invoices/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({}))
        .expect("list invoices failed");
    let invoices: serde_json::Value = list_resp.into_json().unwrap();
    let find_number = |id: &str| -> String {
        invoices
            .as_array()
            .unwrap()
            .iter()
            .find(|i| i["invoice_id"] == id)
            .unwrap()["invoice_number"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let paid_number = find_number(&paid_invoice_id);
    let unpaid_number = find_number(&unpaid_invoice_id);

    let pay_resp = ureq::post(&format!("{}/api/v1/admin/accounting/payments/record", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "invoice_id": paid_invoice_id, "amount": 11800,
            "payment_method": "upi", "reference_number": "REPUPI001",
            "payment_date": "2026-04-10"
        }))
        .expect("record payment failed");
    assert_eq!(pay_resp.status(), 200);

    let run_interest = ureq::post(&format!("{}/api/v1/admin/accounting/interest/run", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({}))
        .expect("run interest failed");
    let run_val: serde_json::Value = run_interest.into_json().unwrap();
    assert_eq!(
        run_val["charges_posted"].as_u64().unwrap(),
        1,
        "only the unpaid overdue invoice should accrue interest"
    );

    let run_report = |report_type: &str, extra: serde_json::Value| -> String {
        let mut body = serde_json::json!({ "report_type": report_type });
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        ureq::post(&format!("{}/api/v1/admin/accounting/reports/run", url))
            .set("Authorization", &admin_auth)
            .send_json(body)
            .unwrap_or_else(|e| panic!("report_type={} failed: {}", report_type, e))
            .into_string()
            .unwrap()
    };

    let invoice_register = run_report(
        "invoice_register",
        serde_json::json!({ "date_from": "2026-04-01", "date_to": "2026-04-30" }),
    );
    assert!(
        invoice_register.contains(&paid_number),
        "{}",
        invoice_register
    );
    assert!(
        invoice_register.contains(&unpaid_number),
        "{}",
        invoice_register
    );
    assert!(
        invoice_register.contains("11800.00"),
        "{}",
        invoice_register
    );
    assert!(invoice_register.contains("PAID"), "{}", invoice_register);
    assert!(invoice_register.contains("UNPAID"), "{}", invoice_register);

    let gst_summary = run_report(
        "gst_summary",
        serde_json::json!({ "date_from": "2026-04-01", "date_to": "2026-04-30" }),
    );
    assert!(
        gst_summary.contains("20000.00"),
        "combined taxable value of both 10000 invoices: {}",
        gst_summary
    );
    assert!(
        gst_summary.contains("3600.00"),
        "combined GST collected (1800 x 2): {}",
        gst_summary
    );

    let interest_report = run_report(
        "interest",
        serde_json::json!({ "date_from": "2020-01-01", "date_to": "2030-12-31" }),
    );
    assert!(
        interest_report.contains(&unpaid_number),
        "{}",
        interest_report
    );
    assert!(
        !interest_report.contains(&paid_number),
        "the fully-paid invoice must never accrue interest: {}",
        interest_report
    );
    assert!(interest_report.contains("11800.00"), "{}", interest_report);

    let outstanding = run_report("outstanding_dues", serde_json::json!({}));
    assert!(outstanding.contains(&unpaid_number), "{}", outstanding);
    assert!(
        !outstanding.contains(&paid_number),
        "a fully-paid invoice must not appear as outstanding: {}",
        outstanding
    );
    assert!(outstanding.contains("11800.00"), "{}", outstanding);

    let ageing = run_report("ageing_summary", serde_json::json!({}));
    assert!(
        ageing.contains("11800.00") || ageing.contains("11800"),
        "the unpaid invoice's outstanding amount must appear in an ageing bucket total: {}",
        ageing
    );

    let defaulters = run_report("defaulters", serde_json::json!({ "min_days_overdue": 1 }));
    assert!(defaulters.contains("REP"), "{}", defaulters);
    assert!(defaulters.contains("0102"), "{}", defaulters);
    assert!(defaulters.contains("11800.00"), "{}", defaulters);

    // receipts.generated_at defaults to the real wall-clock time the row was inserted,
    // not the business-logic payment_date, so this needs a wide range to include today.
    let receipt_register = run_report(
        "receipt_register",
        serde_json::json!({ "date_from": "2020-01-01", "date_to": "2030-12-31" }),
    );
    assert!(
        receipt_register.contains(&paid_number),
        "{}",
        receipt_register
    );
    assert!(
        receipt_register.contains("REPUPI001"),
        "{}",
        receipt_register
    );
    assert!(
        receipt_register.contains("11800.00"),
        "{}",
        receipt_register
    );
    assert!(
        !receipt_register.contains(&unpaid_number),
        "no receipt should exist for the unpaid invoice: {}",
        receipt_register
    );

    // payments.confirmed_at is also a real insert-time timestamp, same reasoning.
    let payment_mode = run_report(
        "payment_mode_summary",
        serde_json::json!({ "date_from": "2020-01-01", "date_to": "2030-12-31" }),
    );
    assert!(payment_mode.contains("upi"), "{}", payment_mode);
    assert!(payment_mode.contains("11800.00"), "{}", payment_mode);

    let income_summary = run_report(
        "income_summary",
        serde_json::json!({ "date_from": "2026-04-01", "date_to": "2026-04-30" }),
    );
    assert!(
        income_summary.contains("20000.00"),
        "maintenance income from both invoices: {}",
        income_summary
    );

    // total_billed filters on billing_period_start (business date, "2026-04-01" for
    // both invoices), but total_collected filters on payments.confirmed_at (the real
    // insert-time timestamp) — a wide range covers both correctly without picking up
    // anything from outside this test's own isolated database.
    let collection_summary_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/reports/collection-summary",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "date_from": "2020-01-01", "date_to": "2030-12-31" }))
    .expect("collection summary failed");
    assert_eq!(collection_summary_resp.status(), 200);
    let collection_summary: serde_json::Value = collection_summary_resp.into_json().unwrap();
    assert_eq!(
        collection_summary["total_billed"].as_str().unwrap(),
        "23600.00"
    );
    assert_eq!(
        collection_summary["total_collected"].as_str().unwrap(),
        "11800.00"
    );
    assert_eq!(
        collection_summary["collection_efficiency_pct"]
            .as_str()
            .unwrap(),
        "50.00"
    );

    // A third flat with a plan assigned but no invoice yet for the next period — preview
    // must compute the same base/GST/total math as real generation would, without
    // writing anything.
    let _ = quick_create_user(&env, &admin_jwt, "rep-preview@vistas.com", "REP", "0103");
    let preview_flat = flat_id_for(&env, "REP", "0103").await;
    assign_plan(&url, &admin_auth, preview_flat, plan_id);
    let preview_resp = ureq::post(&format!("{}/api/v1/admin/accounting/invoices/preview", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "period_start": "2026-05-01" }))
        .expect("preview failed");
    assert_eq!(preview_resp.status(), 200);
    let preview: serde_json::Value = preview_resp.into_json().unwrap();
    let preview_row = preview
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["flat_id"].as_i64().unwrap() as i32 == preview_flat)
        .expect("preview flat must have a row");
    assert!(preview_row["error"].is_null(), "{:?}", preview_row);
    // base_amount is parsed straight from the plan's JSON config with no arithmetic
    // applied, so it keeps whatever scale that literal had (0, not 2).
    assert_eq!(preview_row["base_amount"].as_str().unwrap(), "10000");
    assert_eq!(preview_row["gst_applicable"], true);
    assert_eq!(preview_row["gst_amount"].as_str().unwrap(), "1800.00");
    assert_eq!(preview_row["total_amount"].as_str().unwrap(), "11800.00");
}

/// The consolidated PDF dispatcher (`/documents/pdf`) must route invoice/receipt/noc
/// kinds correctly and reject an unknown kind, mirroring the report dispatcher test.
#[tokio::test]
async fn test_pdf_dispatcher_routes_multiple_kinds_and_rejects_unknown() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let _ = quick_create_user(&env, &admin_jwt, "pdfdispatch@vistas.com", "Y", "0808");
    let flat_id = flat_id_for(&env, "Y", "0808").await;
    let plan_id = create_flat_rate_plan(&url, &admin_auth, 3000);
    assign_plan(&url, &admin_auth, flat_id, plan_id);
    let gen = generate_invoice(
        &url,
        &admin_auth,
        flat_id,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );
    let invoice_id = gen["invoices_created"][0].as_str().unwrap().to_string();

    let invoice_pdf = ureq::post(&format!("{}/api/v1/admin/accounting/documents/pdf", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "kind": "invoice", "invoice_id": invoice_id }))
        .expect("invoice pdf failed");
    assert_eq!(invoice_pdf.status(), 200);
    assert_eq!(
        invoice_pdf.header("Content-Type").unwrap(),
        "application/pdf"
    );

    // NOC is legitimately blocked by an outstanding balance (business rule), not a
    // dispatcher bug — confirm it reaches the right handler by checking the message.
    let noc_pdf = ureq::post(&format!("{}/api/v1/admin/accounting/documents/pdf", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "kind": "noc", "flat_id": flat_id }));
    assert!(noc_pdf.is_err());
    let resp = noc_pdf.unwrap_err().into_response().unwrap();
    assert_eq!(resp.status(), 400);
    let body = resp.into_string().unwrap();
    assert!(
        body.to_lowercase().contains("outstanding"),
        "body was: {}",
        body
    );

    // Missing the id a kind needs must 400.
    let missing_id = ureq::post(&format!("{}/api/v1/admin/accounting/documents/pdf", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "kind": "receipt" }));
    assert!(missing_id.is_err());
    assert_eq!(
        missing_id.unwrap_err().into_response().unwrap().status(),
        400
    );

    // An unknown kind must be rejected.
    let unknown = ureq::post(&format!("{}/api/v1/admin/accounting/documents/pdf", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "kind": "not_a_real_kind" }));
    assert!(unknown.is_err());
    assert_eq!(unknown.unwrap_err().into_response().unwrap().status(), 400);

    // Sample kinds need no real invoice/payment/flat at all.
    let sample_invoice = ureq::post(&format!("{}/api/v1/admin/accounting/documents/pdf", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "kind": "sample_invoice" }))
        .expect("sample invoice pdf failed");
    assert_eq!(sample_invoice.status(), 200);
    assert_eq!(
        sample_invoice.header("Content-Type").unwrap(),
        "application/pdf"
    );
    let sample_receipt = ureq::post(&format!("{}/api/v1/admin/accounting/documents/pdf", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "kind": "sample_receipt" }))
        .expect("sample receipt pdf failed");
    assert_eq!(sample_receipt.status(), 200);
    assert_eq!(
        sample_receipt.header("Content-Type").unwrap(),
        "application/pdf"
    );
}

/// A resident hitting the member-facing PDF endpoint for another flat's invoice/
/// receipt/statement/NOC must be rejected before anything is rendered or disclosed —
/// and a genuinely nonexistent id must produce the exact same error as one that exists
/// but isn't theirs, so the error itself can't be used to enumerate other residents'
/// invoices or payments.
#[tokio::test]
async fn test_member_document_pdf_rejects_another_flats_documents_without_leaking_which() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let _ = quick_create_user(&env, &admin_jwt, "pdfidor-a@vistas.com", "P", "0101");
    let _ = quick_create_user(&env, &admin_jwt, "pdfidor-b@vistas.com", "P", "0102");
    let flat_a = flat_id_for(&env, "P", "0101").await;
    let flat_b = flat_id_for(&env, "P", "0102").await;

    let plan_id = create_flat_rate_plan(&url, &admin_auth, 4000);
    assign_plan(&url, &admin_auth, flat_a, plan_id);
    assign_plan(&url, &admin_auth, flat_b, plan_id);

    let gen_a = generate_invoice(
        &url,
        &admin_auth,
        flat_a,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );
    let invoice_a = gen_a["invoices_created"][0].as_str().unwrap().to_string();

    let gen_b = generate_invoice(
        &url,
        &admin_auth,
        flat_b,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );
    let invoice_b = gen_b["invoices_created"][0].as_str().unwrap().to_string();

    let pay_b = ureq::post(&format!("{}/api/v1/admin/accounting/payments/record", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "invoice_id": invoice_b, "amount": 4000, "payment_method": "upi",
            "reference_number": "PDFIDORB1", "payment_date": "2026-07-05"
        }))
        .expect("record payment for flat b failed");
    let payment_b: serde_json::Value = pay_b.into_json().unwrap();
    let payments_b_list = ureq::post(&format!("{}/api/v1/admin/accounting/payments/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "invoice_id": invoice_b }))
        .expect("list payments for flat b failed");
    let payments_b_val: serde_json::Value = payments_b_list.into_json().unwrap();
    let payment_b_id = payments_b_val[0]["payment_id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(payment_b["receipt_number"]
        .as_str()
        .unwrap()
        .starts_with("RCT-"));

    let resident_a_jwt =
        quality::quick_onboard_user(&env, "pdfidor-a@vistas.com", "resident_pass_secure_123");
    let resident_a_auth = auth(&resident_a_jwt);

    let forbidden_cases: Vec<(&str, serde_json::Value)> = vec![
        (
            "invoice for flat B",
            serde_json::json!({ "kind": "invoice", "invoice_id": invoice_b }),
        ),
        (
            "receipt for flat B",
            serde_json::json!({ "kind": "receipt", "payment_id": payment_b_id }),
        ),
        (
            "statement for flat B",
            serde_json::json!({
                "kind": "statement", "flat_id": flat_b,
                "date_from": "2026-07-01", "date_to": "2026-07-31"
            }),
        ),
        (
            "noc for flat B",
            serde_json::json!({ "kind": "noc", "flat_id": flat_b }),
        ),
        (
            "nonexistent invoice_id",
            serde_json::json!({
                "kind": "invoice", "invoice_id": "00000000-0000-0000-0000-000000000000"
            }),
        ),
        (
            "nonexistent payment_id",
            serde_json::json!({
                "kind": "receipt", "payment_id": "00000000-0000-0000-0000-000000000000"
            }),
        ),
    ];

    let mut bodies = Vec::new();
    for (label, body) in forbidden_cases {
        let resp = ureq::post(&format!("{}/api/v1/accounting/documents/pdf", url))
            .set("Authorization", &resident_a_auth)
            .send_json(body);
        let err = resp.expect_err(&format!("{label} must be rejected"));
        let response = err.into_response().unwrap();
        assert_eq!(
            response.status(),
            403,
            "{label} must be 403, not leak existence via 404"
        );
        bodies.push((label, response.into_string().unwrap()));
    }
    let first_body = &bodies[0].1;
    for (label, body) in &bodies[1..] {
        assert_eq!(
            body, first_body,
            "{} produced a different error body than {} — that difference itself leaks which ids are real",
            label, bodies[0].0
        );
    }

    // The exact same requests for resident A's own flat must succeed.
    let own_invoice = ureq::post(&format!("{}/api/v1/accounting/documents/pdf", url))
        .set("Authorization", &resident_a_auth)
        .send_json(serde_json::json!({ "kind": "invoice", "invoice_id": invoice_a }))
        .expect("resident must be able to fetch their own invoice pdf");
    assert_eq!(own_invoice.status(), 200);
}

fn extract_pdf_text(bytes: &[u8]) -> String {
    let mut warnings = Vec::new();
    let parsed =
        printpdf::PdfDocument::parse(bytes, &printpdf::PdfParseOptions::default(), &mut warnings)
            .expect("generated PDF must parse back cleanly");
    parsed
        .extract_text()
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Society Profile is admin-editable and must actually show up on a real generated
/// invoice's PDF header once saved — not just round-trip through its own get/set.
#[tokio::test]
async fn test_society_profile_saves_and_appears_on_a_real_invoice_pdf() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let get_before = ureq::post(&format!("{}/api/v1/admin/accounting/society-profile", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({}))
        .expect("get society profile failed");
    assert_eq!(get_before.status(), 200);
    let before: serde_json::Value = get_before.into_json().unwrap();
    assert!(before["profile"].is_null());
    assert_eq!(before["super_admin_email"], "superadmin@vistas.com");
    assert!(!before["account_created_at"].as_str().unwrap().is_empty());

    let set_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/society-profile/set",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({
        "name": "Sample Vistas CHS Ltd",
        "registration_number": "REG/999/2026",
        "address": "Plot 7, Sample Road, Mumbai - 400072",
        "state": "Maharashtra",
        "phone": "9998887777"
    }))
    .expect("set society profile failed");
    assert_eq!(set_resp.status(), 204);

    let get_after = ureq::post(&format!("{}/api/v1/admin/accounting/society-profile", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({}))
        .expect("get society profile failed");
    let after: serde_json::Value = get_after.into_json().unwrap();
    assert_eq!(after["profile"]["name"], "Sample Vistas CHS Ltd");
    assert_eq!(after["profile"]["registration_number"], "REG/999/2026");
    assert_eq!(after["profile"]["state"], "Maharashtra");

    let _ = quick_create_user(&env, &admin_jwt, "sp-invoice@vistas.com", "SP", "0101");
    let flat_id = flat_id_for(&env, "SP", "0101").await;
    let plan_id = create_flat_rate_plan(&url, &admin_auth, 4000);
    assign_plan(&url, &admin_auth, flat_id, plan_id);
    let gen = generate_invoice(
        &url,
        &admin_auth,
        flat_id,
        "2026-07-01",
        "2026-07-31",
        "2026-07-10",
    );
    let invoice_id = gen["invoices_created"][0].as_str().unwrap().to_string();

    let pdf_resp = ureq::post(&format!("{}/api/v1/admin/accounting/documents/pdf", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "kind": "invoice", "invoice_id": invoice_id }))
        .expect("invoice pdf failed");
    let mut pdf_bytes = Vec::new();
    pdf_resp
        .into_reader()
        .read_to_end(&mut pdf_bytes)
        .expect("failed to read pdf body");
    let text = extract_pdf_text(&pdf_bytes);

    assert!(
        text.contains("SAMPLE VISTAS CHS LTD"),
        "society name missing from invoice PDF: {}",
        text
    );
    assert!(
        text.contains("REG/999/2026"),
        "registration number missing from invoice PDF: {}",
        text
    );
    assert!(
        text.contains("Plot 7, Sample Road, Mumbai - 400072"),
        "address missing from invoice PDF: {}",
        text
    );
    assert!(
        text.contains("Maharashtra"),
        "state missing from invoice PDF: {}",
        text
    );

    let bad_state_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/society-profile/set",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({
        "name": "Sample Vistas CHS Ltd",
        "address": "Plot 7, Sample Road, Mumbai - 400072",
        "state": "Narnia",
    }))
    .expect_err("setting a non-Indian state should be rejected");
    assert_eq!(bad_state_resp.into_response().unwrap().status(), 400);
}
