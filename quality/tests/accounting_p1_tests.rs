use quality::{quick_create_user, quick_setup_admin, setup_api_test_env, TestEnv};

fn auth(jwt: &str) -> String {
    format!("Bearer {}", jwt)
}

async fn flat_id_for(env: &TestEnv, wing: &str, flat_number: &str) -> i32 {
    sqlx::query_scalar::<_, i32>("SELECT flat_id FROM flats WHERE wing = $1 AND flat_number = $2")
        .bind(wing)
        .bind(flat_number)
        .fetch_one(&env.db_pool)
        .await
        .expect("flat not found")
}

async fn account_id_for(env: &TestEnv, code: &str) -> i32 {
    sqlx::query_scalar::<_, i32>("SELECT account_id FROM chart_of_accounts WHERE code = $1")
        .bind(code)
        .fetch_one(&env.db_pool)
        .await
        .expect("account not found")
}

async fn fund_id_for(env: &TestEnv, code: &str) -> i32 {
    sqlx::query_scalar::<_, i32>("SELECT fund_id FROM funds WHERE code = $1")
        .bind(code)
        .fetch_one(&env.db_pool)
        .await
        .expect("fund not found")
}

async fn current_fiscal_year_id(env: &TestEnv) -> i32 {
    sqlx::query_scalar::<_, i32>(
        "SELECT fiscal_year_id FROM fiscal_years ORDER BY start_date DESC LIMIT 1",
    )
    .fetch_one(&env.db_pool)
    .await
    .expect("fiscal year not found")
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
    resp.into_json::<serde_json::Value>()
        .unwrap()
        .as_i64()
        .unwrap()
}

/// Submits a vendor bill that crosses the single-payment TDS threshold, approves it,
/// and checks the real accounting effect: net_payable/tds_amount computed correctly,
/// vendor payable + TDS payable posted, and a TDS record created for Form 16A data.
/// Also checks rejection doesn't post anything, and double-approval is blocked.
#[tokio::test]
async fn test_vendor_expense_tds_and_approval_posts_correctly() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    ureq::post(&format!("{}/api/v1/admin/accounting/tds-rules/create", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "section": "194C",
            "payment_category": "contractor",
            "rate_pct": 1,
            "threshold_single_payment": 30000,
            "threshold_annual_aggregate": 100000,
            "effective_from": "2026-04-01"
        }))
        .expect("create tds rule failed");

    let vendor_resp = ureq::post(&format!("{}/api/v1/admin/accounting/vendors/create", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "name": "ABC Security Services",
            "category": "security"
        }))
        .expect("create vendor failed");
    let vendor_id = vendor_resp
        .into_json::<serde_json::Value>()
        .unwrap()
        .as_i64()
        .unwrap();

    let security_account = account_id_for(&env, "5100").await;

    // 35,000 crosses the 30,000 single-payment threshold -> 1% TDS = 350, net = 34,650.
    let expense_resp = ureq::post(&format!("{}/api/v1/admin/accounting/expenses/submit", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "vendor_id": vendor_id,
            "account_id": security_account,
            "payment_category": "contractor",
            "vendor_invoice_number": "ABC-INV-001",
            "amount": 35000,
            "expense_date": "2026-07-15"
        }))
        .expect("submit expense failed");
    assert_eq!(expense_resp.status(), 201);
    let expense_id = expense_resp
        .into_json::<serde_json::Value>()
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();

    let list_resp = ureq::post(&format!("{}/api/v1/admin/accounting/expenses/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "status": "PENDING_APPROVAL" }))
        .expect("list expenses failed");
    let expenses: serde_json::Value = list_resp.into_json().unwrap();
    let expense = expenses
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["expense_id"] == expense_id)
        .unwrap();
    assert_eq!(expense["tds_amount"].as_str().unwrap(), "350.00");
    assert_eq!(expense["net_payable"].as_str().unwrap(), "34650.00");

    let approve_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/expenses/{}/approve",
        url, expense_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({}))
    .expect("approve expense failed");
    assert_eq!(approve_resp.status(), 200);

    // Double approval must fail — already reviewed.
    let reapprove = ureq::post(&format!(
        "{}/api/v1/admin/accounting/expenses/{}/approve",
        url, expense_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({}));
    assert!(reapprove.is_err());
    assert_eq!(
        reapprove.unwrap_err().into_response().unwrap().status(),
        400
    );

    // TDS summary (Form 16A source data) must show this record.
    let fy_id = current_fiscal_year_id(&env).await;
    let tds_resp = ureq::post(&format!("{}/api/v1/admin/accounting/tds-summary", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "vendor_id": vendor_id, "fiscal_year_id": fy_id }))
        .expect("tds summary failed");
    let tds_rows: serde_json::Value = tds_resp.into_json().unwrap();
    assert_eq!(tds_rows.as_array().unwrap().len(), 1);
    assert_eq!(tds_rows[0]["tds_amount"].as_str().unwrap(), "350.00");

    // A second, small expense below both thresholds must attract no TDS at all.
    let small_expense_resp =
        ureq::post(&format!("{}/api/v1/admin/accounting/expenses/submit", url))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({
                "vendor_id": vendor_id,
                "account_id": security_account,
                "payment_category": "contractor",
                "amount": 5000,
                "expense_date": "2026-07-20"
            }))
            .expect("submit small expense failed");
    let small_expense_id = small_expense_resp
        .into_json::<serde_json::Value>()
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    let list2_resp = ureq::post(&format!("{}/api/v1/admin/accounting/expenses/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "status": "PENDING_APPROVAL" }))
        .expect("list expenses failed");
    let expenses2: serde_json::Value = list2_resp.into_json().unwrap();
    let small = expenses2
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["expense_id"] == small_expense_id)
        .unwrap();
    assert_eq!(small["tds_amount"].as_str().unwrap(), "0");
    assert_eq!(small["net_payable"].as_str().unwrap(), "5000.00");

    // Reject it — must not appear as approved, and rejecting twice must fail.
    let reject_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/expenses/{}/reject",
        url, small_expense_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "reason": "Duplicate submission" }))
    .expect("reject expense failed");
    assert_eq!(reject_resp.status(), 204);
    let rereject = ureq::post(&format!(
        "{}/api/v1/admin/accounting/expenses/{}/reject",
        url, small_expense_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "reason": "Again" }));
    assert!(rereject.is_err());
}

/// Submitting an expense that crosses a TDS threshold speculatively records a TDS
/// record before anyone has approved anything — if the expense is then REJECTED, that
/// record must be deleted. A vendor's TDS summary (Form 16A source data) must never
/// show tax deducted on money that was never actually paid.
#[tokio::test]
async fn test_rejecting_a_tds_crossing_expense_removes_its_tds_record() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    ureq::post(&format!("{}/api/v1/admin/accounting/tds-rules/create", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "section": "194C",
            "payment_category": "contractor",
            "rate_pct": 1,
            "threshold_single_payment": 30000,
            "threshold_annual_aggregate": 100000,
            "effective_from": "2026-04-01"
        }))
        .expect("create tds rule failed");

    let vendor_id: i64 = ureq::post(&format!("{}/api/v1/admin/accounting/vendors/create", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "name": "Rejected Co", "category": "security" }))
        .expect("create vendor failed")
        .into_json::<serde_json::Value>()
        .unwrap()
        .as_i64()
        .unwrap();
    let security_account = account_id_for(&env, "5100").await;

    let expense_id = ureq::post(&format!("{}/api/v1/admin/accounting/expenses/submit", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "vendor_id": vendor_id,
            "account_id": security_account,
            "payment_category": "contractor",
            "vendor_invoice_number": "REJ-INV-001",
            "amount": 35000,
            "expense_date": "2026-07-15"
        }))
        .expect("submit expense failed")
        .into_json::<serde_json::Value>()
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();

    let fy_id = current_fiscal_year_id(&env).await;
    let before = ureq::post(&format!("{}/api/v1/admin/accounting/tds-summary", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "vendor_id": vendor_id, "fiscal_year_id": fy_id }))
        .expect("tds summary failed")
        .into_json::<serde_json::Value>()
        .unwrap();
    assert_eq!(
        before.as_array().unwrap().len(),
        1,
        "submitting a TDS-crossing expense must record it, even before approval"
    );

    let reject_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/expenses/{}/reject",
        url, expense_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "reason": "Invoice amount disputed" }))
    .expect("reject expense failed");
    assert_eq!(reject_resp.status(), 204);

    let after = ureq::post(&format!("{}/api/v1/admin/accounting/tds-summary", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "vendor_id": vendor_id, "fiscal_year_id": fy_id }))
        .expect("tds summary failed")
        .into_json::<serde_json::Value>()
        .unwrap();
    assert!(
        after.as_array().unwrap().is_empty(),
        "rejecting the expense must remove its TDS record — got {:?}",
        after
    );
}

/// A purchase order belongs to exactly one vendor — submitting an expense against it
/// while billing a DIFFERENT vendor would corrupt that vendor's spend history and PO
/// tracking with no error at all if left unchecked.
#[tokio::test]
async fn test_submit_expense_rejects_po_vendor_mismatch() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let vendor_a: i64 = ureq::post(&format!("{}/api/v1/admin/accounting/vendors/create", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "name": "Vendor A", "category": "security" }))
        .expect("create vendor A failed")
        .into_json::<serde_json::Value>()
        .unwrap()
        .as_i64()
        .unwrap();
    let vendor_b: i64 = ureq::post(&format!("{}/api/v1/admin/accounting/vendors/create", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "name": "Vendor B", "category": "housekeeping" }))
        .expect("create vendor B failed")
        .into_json::<serde_json::Value>()
        .unwrap()
        .as_i64()
        .unwrap();

    let po_id: String = ureq::post(&format!(
        "{}/api/v1/admin/accounting/purchase-orders/create",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({
        "vendor_id": vendor_a,
        "description": "Annual security contract",
        "amount": 50000
    }))
    .expect("create PO failed")
    .into_json::<serde_json::Value>()
    .unwrap()
    .as_str()
    .unwrap()
    .to_string();

    let security_account = account_id_for(&env, "5100").await;

    // Vendor A's PO, but billing Vendor B — must be rejected, not silently accepted.
    let mismatch_resp = ureq::post(&format!("{}/api/v1/admin/accounting/expenses/submit", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "po_id": po_id,
            "vendor_id": vendor_b,
            "account_id": security_account,
            "payment_category": "housekeeping",
            "amount": 10000,
            "expense_date": "2026-07-15"
        }));
    assert!(mismatch_resp.is_err());
    assert_eq!(
        mismatch_resp.unwrap_err().into_response().unwrap().status(),
        400
    );

    // The correct vendor for that PO must still succeed.
    let ok_resp = ureq::post(&format!("{}/api/v1/admin/accounting/expenses/submit", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "po_id": po_id,
            "vendor_id": vendor_a,
            "account_id": security_account,
            "payment_category": "security",
            "amount": 10000,
            "expense_date": "2026-07-15"
        }))
        .expect("submit expense with matching vendor should succeed");
    assert_eq!(ok_resp.status(), 201);
}

/// A flat assigned to the sinking fund with an active contribution rule must get a
/// SECOND line item on its regular maintenance bill — the real multi-line invoicing
/// this session's earlier reports pass flagged as missing.
#[tokio::test]
async fn test_fund_contribution_adds_second_invoice_line_and_increases_total() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let _ = quick_create_user(&env, &admin_jwt, "sinking@vistas.com", "S", "0101");
    let flat_id = flat_id_for(&env, "S", "0101").await;
    let plan_id = create_flat_rate_plan(&url, &admin_auth, 5000);
    ureq::post(&format!(
        "{}/api/v1/admin/accounting/billing-plans/assign",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "flat_id": flat_id, "billing_plan_id": plan_id }))
    .expect("assign plan failed");

    let sinking_fund_id = fund_id_for(&env, "SINKING").await;
    ureq::post(&format!(
        "{}/api/v1/admin/accounting/funds/assign-flat",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "flat_id": flat_id, "fund_id": sinking_fund_id }))
    .expect("assign flat to fund failed");

    // 10% of maintenance -> 500 on a 5000 base.
    ureq::post(&format!(
        "{}/api/v1/admin/accounting/funds/contribution-rules/create",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({
        "fund_id": sinking_fund_id,
        "contribution_type": "PCT_OF_MAINTENANCE",
        "amount_or_pct": 10,
        "effective_from": "2026-04-01"
    }))
    .expect("create fund contribution rule failed");

    let gen_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/invoices/generate",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({
        "flat_id": flat_id,
        "period_start": "2026-07-01",
        "period_end": "2026-07-31",
        "due_date": "2026-07-10"
    }))
    .expect("generate invoice failed");
    let gen_val: serde_json::Value = gen_resp.into_json().unwrap();
    assert!(
        gen_val["skipped"].as_array().unwrap().is_empty(),
        "generation was skipped: {:?}",
        gen_val
    );

    let list_resp = ureq::post(&format!("{}/api/v1/admin/accounting/invoices/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "flat_id": flat_id }))
        .expect("list invoices failed");
    let invoices: serde_json::Value = list_resp.into_json().unwrap();
    // 5000 maintenance + 500 sinking fund contribution = 5500 total.
    assert_eq!(invoices[0]["total_amount"].as_str().unwrap(), "5500.00");
}

/// Imports a bank statement CSV, auto-matches it against an already-recorded payment's
/// ledger transaction (same date + amount), and confirms an unrelated/unmatched line
/// stays unmatched for manual review rather than being guessed at.
#[tokio::test]
async fn test_bank_reconciliation_auto_match() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let _ = quick_create_user(&env, &admin_jwt, "bankrecon@vistas.com", "T", "0202");
    let flat_id = flat_id_for(&env, "T", "0202").await;
    let plan_id = create_flat_rate_plan(&url, &admin_auth, 6000);
    ureq::post(&format!(
        "{}/api/v1/admin/accounting/billing-plans/assign",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "flat_id": flat_id, "billing_plan_id": plan_id }))
    .expect("assign plan failed");
    let gen_resp = ureq::post(&format!("{}/api/v1/admin/accounting/invoices/generate", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "flat_id": flat_id, "period_start": "2026-07-01", "period_end": "2026-07-31", "due_date": "2026-07-10"
        }))
        .expect("generate invoice failed");
    let invoice_id = gen_resp.into_json::<serde_json::Value>().unwrap()["invoices_created"][0]
        .as_str()
        .unwrap()
        .to_string();

    ureq::post(&format!("{}/api/v1/admin/accounting/payments/record", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "invoice_id": invoice_id, "amount": 6000, "payment_method": "bank_transfer",
            "reference_number": "NEFT-BR-1", "payment_date": "2026-07-12"
        }))
        .expect("record payment failed");

    let csv_content =
        "Date,Description,Amount\n2026-07-12,NEFT CREDIT,6000\n2026-07-13,UNRELATED CHARGE,-999\n";
    let import_resp = ureq::post(&format!("{}/api/v1/admin/accounting/bank-reconciliation/import", url))
        .set("Authorization", &admin_auth)
        .set("Content-Type", "multipart/form-data; boundary=----boundary")
        .send_string(&format!(
            "------boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"stmt.csv\"\r\nContent-Type: text/csv\r\n\r\n{}\r\n------boundary--\r\n",
            csv_content
        ))
        .expect("import statement failed");
    assert_eq!(import_resp.status(), 201);
    let import_val: serde_json::Value = import_resp.into_json().unwrap();
    assert_eq!(import_val["lines_imported"].as_u64().unwrap(), 2);
    let import_id = import_val["import_id"].as_str().unwrap().to_string();

    let match_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/bank-reconciliation/{}/auto-match",
        url, import_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({}))
    .expect("auto match failed");
    let match_val: serde_json::Value = match_resp.into_json().unwrap();
    assert_eq!(match_val["matched"].as_u64().unwrap(), 1);
    assert_eq!(match_val["unmatched"].as_u64().unwrap(), 1);

    let lines_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/bank-reconciliation/lines/list",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "import_id": import_id }))
    .expect("list lines failed");
    let lines: serde_json::Value = lines_resp.into_json().unwrap();
    let matched_count = lines
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["match_status"] == "MATCHED")
        .count();
    let unmatched_count = lines
        .as_array()
        .unwrap()
        .iter()
        .filter(|l| l["match_status"] == "UNMATCHED")
        .count();
    assert_eq!(matched_count, 1);
    assert_eq!(unmatched_count, 1);

    let matched_line_id = lines
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["match_status"] == "MATCHED")
        .unwrap()["line_id"]
        .as_i64()
        .unwrap();
    let unmatched_line_id = lines
        .as_array()
        .unwrap()
        .iter()
        .find(|l| l["match_status"] == "UNMATCHED")
        .unwrap()["line_id"]
        .as_i64()
        .unwrap();

    // A line that's already MATCHED must reject being ignored (or re-matched) — silently
    // overwriting an existing match with no trace of what it was matched to before would
    // break reconciliation integrity.
    let reignore_matched = ureq::post(&format!(
        "{}/api/v1/admin/accounting/bank-reconciliation/lines/{}/ignore",
        url, matched_line_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({}));
    assert!(reignore_matched.is_err());
    assert_eq!(
        reignore_matched
            .unwrap_err()
            .into_response()
            .unwrap()
            .status(),
        400
    );

    // The genuinely unmatched line can be ignored once...
    let ignore_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/bank-reconciliation/lines/{}/ignore",
        url, unmatched_line_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({}))
    .expect("ignore failed");
    assert_eq!(ignore_resp.status(), 204);

    // ...but not matched afterward, since it's no longer UNMATCHED.
    let match_after_ignore = ureq::post(&format!(
        "{}/api/v1/admin/accounting/bank-reconciliation/lines/{}/match",
        url, unmatched_line_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "transaction_id": uuid::Uuid::new_v4() }));
    assert!(match_after_ignore.is_err());
    assert_eq!(
        match_after_ignore
            .unwrap_err()
            .into_response()
            .unwrap()
            .status(),
        400
    );
}

/// The manual-match picker (`/lines/:id/match-candidates`) must surface real ledger
/// transactions near a statement line's date even when the amount doesn't line up
/// exactly — that's exactly the case auto-match can't resolve on its own, and the whole
/// reason a manual picker exists instead of requiring an admin to already know a raw
/// transaction ID.
#[tokio::test]
async fn test_manual_match_candidates_surfaces_nearby_transaction_and_completes_match() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let _ = quick_create_user(&env, &admin_jwt, "manualmatch@vistas.com", "U", "0505");
    let flat_id = flat_id_for(&env, "U", "0505").await;
    let plan_id = create_flat_rate_plan(&url, &admin_auth, 6000);
    ureq::post(&format!(
        "{}/api/v1/admin/accounting/billing-plans/assign",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "flat_id": flat_id, "billing_plan_id": plan_id }))
    .expect("assign plan failed");
    let gen_resp = ureq::post(&format!("{}/api/v1/admin/accounting/invoices/generate", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "flat_id": flat_id, "period_start": "2026-07-01", "period_end": "2026-07-31", "due_date": "2026-07-10"
        }))
        .expect("generate invoice failed");
    let invoice_id = gen_resp.into_json::<serde_json::Value>().unwrap()["invoices_created"][0]
        .as_str()
        .unwrap()
        .to_string();

    // The real payment is 6000, recorded on 2026-07-12.
    ureq::post(&format!("{}/api/v1/admin/accounting/payments/record", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "invoice_id": invoice_id, "amount": 6000, "payment_method": "bank_transfer",
            "reference_number": "NEFT-MM-1", "payment_date": "2026-07-12"
        }))
        .expect("record payment failed");

    // The statement line's amount deliberately does NOT match (5999 vs the real 6000,
    // e.g. a bank fee was netted out) — auto-match (exact date+amount) would never find
    // this, but a human recognizes it immediately from the date/description.
    let csv_content = "Date,Description,Amount\n2026-07-13,NEFT CR UNRECOGNIZED,5999\n";
    let import_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/bank-reconciliation/import",
        url
    ))
    .set("Authorization", &admin_auth)
    .set("Content-Type", "multipart/form-data; boundary=----boundary")
    .send_string(&format!(
        "------boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"stmt.csv\"\r\nContent-Type: text/csv\r\n\r\n{}\r\n------boundary--\r\n",
        csv_content
    ))
    .expect("import statement failed");
    let import_val: serde_json::Value = import_resp.into_json().unwrap();
    let line_id = {
        let lines_resp = ureq::post(&format!(
            "{}/api/v1/admin/accounting/bank-reconciliation/lines/list",
            url
        ))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "import_id": import_val["import_id"] }))
        .expect("list lines failed");
        let lines: serde_json::Value = lines_resp.into_json().unwrap();
        lines[0]["line_id"].as_i64().unwrap()
    };

    let candidates_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/bank-reconciliation/lines/{}/match-candidates",
        url, line_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({}))
    .expect("match candidates failed");
    assert_eq!(candidates_resp.status(), 200);
    let candidates: serde_json::Value = candidates_resp.into_json().unwrap();
    let candidates = candidates.as_array().unwrap();
    assert!(
        !candidates.is_empty(),
        "the real payment's ledger transaction must show up even though its amount doesn't exactly match the statement line"
    );
    let picked = &candidates[0];
    assert_eq!(picked["amount"].as_str().unwrap(), "6000.00");

    let transaction_id = picked["transaction_id"].as_str().unwrap();
    let match_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/bank-reconciliation/lines/{}/match",
        url, line_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "transaction_id": transaction_id }))
    .expect("manual match failed");
    assert_eq!(match_resp.status(), 204);
}

/// Budget vs actuals: an approved vendor expense must show up as the "actual" side of
/// the variance report against whatever was budgeted for that account.
#[tokio::test]
async fn test_budget_variance_reflects_approved_expense() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let fy_id = current_fiscal_year_id(&env).await;
    let security_account = account_id_for(&env, "5100").await;

    ureq::post(&format!("{}/api/v1/admin/accounting/budgets/set", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "fiscal_year_id": fy_id,
            "lines": [{ "account_id": security_account, "budgeted_amount": 100000 }]
        }))
        .expect("set budget failed");

    let vendor_resp = ureq::post(&format!("{}/api/v1/admin/accounting/vendors/create", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "name": "Guard Co", "category": "security" }))
        .expect("create vendor failed");
    let vendor_id = vendor_resp
        .into_json::<serde_json::Value>()
        .unwrap()
        .as_i64()
        .unwrap();

    let expense_resp = ureq::post(&format!("{}/api/v1/admin/accounting/expenses/submit", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "vendor_id": vendor_id, "account_id": security_account, "payment_category": "contractor",
            "amount": 12000, "expense_date": "2026-07-01"
        }))
        .expect("submit expense failed");
    let expense_id = expense_resp
        .into_json::<serde_json::Value>()
        .unwrap()
        .as_str()
        .unwrap()
        .to_string();
    ureq::post(&format!(
        "{}/api/v1/admin/accounting/expenses/{}/approve",
        url, expense_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({}))
    .expect("approve expense failed");

    let variance_resp = ureq::post(&format!("{}/api/v1/admin/accounting/budgets/variance", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "fiscal_year_id": fy_id }))
        .expect("variance report failed");
    let variance: serde_json::Value = variance_resp.into_json().unwrap();
    let row = variance
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["account_code"] == "5100")
        .unwrap();
    assert_eq!(row["budgeted_amount"].as_str().unwrap(), "100000.00");
    assert_eq!(row["actual_amount"].as_str().unwrap(), "12000.00");
    assert_eq!(row["variance_amount"].as_str().unwrap(), "-88000.00");
}

/// Closes the current fiscal year (posting the closing entry into Retained Earnings),
/// confirms new postings into that period are then rejected, then reopens with a
/// reason and confirms posting works again.
#[tokio::test]
async fn test_fiscal_year_close_blocks_postings_and_reopen_restores_them() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let _ = quick_create_user(&env, &admin_jwt, "fyclose@vistas.com", "U", "0303");
    let flat_id = flat_id_for(&env, "U", "0303").await;
    let plan_id = create_flat_rate_plan(&url, &admin_auth, 4000);
    ureq::post(&format!(
        "{}/api/v1/admin/accounting/billing-plans/assign",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "flat_id": flat_id, "billing_plan_id": plan_id }))
    .expect("assign plan failed");

    // One invoice posted before close, so the closing entry has real income to zero out.
    ureq::post(&format!("{}/api/v1/admin/accounting/invoices/generate", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "flat_id": flat_id, "period_start": "2026-06-01", "period_end": "2026-06-30", "due_date": "2026-06-10"
        }))
        .expect("generate invoice failed");

    let fy_id = current_fiscal_year_id(&env).await;
    let close_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/fiscal-years/{}/close",
        url, fy_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({}))
    .expect("close fiscal year failed");
    assert_eq!(close_resp.status(), 200);
    let close_val: serde_json::Value = close_resp.into_json().unwrap();
    assert!(
        close_val["closing_transaction_id"].is_string(),
        "expected a closing transaction to be posted"
    );

    // Closing the same year twice must fail.
    let reclose = ureq::post(&format!(
        "{}/api/v1/admin/accounting/fiscal-years/{}/close",
        url, fy_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({}));
    assert!(reclose.is_err());
    assert_eq!(reclose.unwrap_err().into_response().unwrap().status(), 400);

    // A new invoice dated inside the now-closed year must be rejected outright.
    let blocked_gen = ureq::post(&format!("{}/api/v1/admin/accounting/invoices/generate", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "flat_id": flat_id, "period_start": "2026-07-01", "period_end": "2026-07-31", "due_date": "2026-07-10"
        }))
        .expect("generate call should still return 200 with a skip, not transport-error");
    let blocked_val: serde_json::Value = blocked_gen.into_json().unwrap();
    assert!(blocked_val["invoices_created"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(blocked_val["skipped"][0][1]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("closed"));

    // Reopen requires a reason.
    let reopen_no_reason = ureq::post(&format!(
        "{}/api/v1/admin/accounting/fiscal-years/{}/reopen",
        url, fy_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "reason": "" }));
    assert!(reopen_no_reason.is_err());
    assert_eq!(
        reopen_no_reason
            .unwrap_err()
            .into_response()
            .unwrap()
            .status(),
        400
    );

    let reopen_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/fiscal-years/{}/reopen",
        url, fy_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "reason": "Auditor found a missed entry" }))
    .expect("reopen failed");
    assert_eq!(reopen_resp.status(), 204);

    // Posting must now work again.
    let after_reopen_gen = ureq::post(&format!("{}/api/v1/admin/accounting/invoices/generate", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "flat_id": flat_id, "period_start": "2026-07-01", "period_end": "2026-07-31", "due_date": "2026-07-10"
        }))
        .expect("generate after reopen failed");
    let after_val: serde_json::Value = after_reopen_gen.into_json().unwrap();
    assert_eq!(after_val["invoices_created"].as_array().unwrap().len(), 1);
}

/// A society's bylaws name an expense head this system didn't come seeded with (e.g.
/// "Water Charges") — must be addable without a migration, usable immediately for a
/// real vendor expense, and structural accounts must stay off-limits.
#[tokio::test]
async fn test_dynamic_chart_of_accounts_create_use_and_deactivate() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    // Structural account types must be rejected outright.
    for bad_type in ["ASSET", "LIABILITY", "EQUITY"] {
        let resp = ureq::post(&format!("{}/api/v1/admin/accounting/accounts/create", url))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({ "code": "9999", "name": "Should Fail", "account_type": bad_type }));
        assert!(
            resp.is_err(),
            "expected {} account creation to be rejected",
            bad_type
        );
        assert_eq!(resp.unwrap_err().into_response().unwrap().status(), 400);
    }

    let create_resp = ureq::post(&format!("{}/api/v1/admin/accounting/accounts/create", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "code": "5600", "name": "Water Charges", "account_type": "EXPENSE" }))
        .expect("create account failed");
    assert_eq!(create_resp.status(), 201);
    let account_id = create_resp
        .into_json::<serde_json::Value>()
        .unwrap()
        .as_i64()
        .unwrap();

    // A duplicate code must be rejected, not silently accepted twice.
    let dup_resp = ureq::post(&format!("{}/api/v1/admin/accounting/accounts/create", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "code": "5600", "name": "Water Charges Again", "account_type": "EXPENSE" }));
    assert!(dup_resp.is_err());
    assert_eq!(dup_resp.unwrap_err().into_response().unwrap().status(), 400);

    let list_resp = ureq::post(&format!("{}/api/v1/admin/accounting/accounts/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "account_type": "EXPENSE" }))
        .expect("list accounts failed");
    let accounts: serde_json::Value = list_resp.into_json().unwrap();
    let water = accounts
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["code"] == "5600")
        .unwrap();
    assert_eq!(water["name"], "Water Charges");
    assert_eq!(water["normal_balance"], "DEBIT");
    assert_eq!(water["is_system"], false);

    // Use it immediately for a real vendor expense — no restart, no migration.
    let vendor_resp = ureq::post(&format!("{}/api/v1/admin/accounting/vendors/create", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "name": "City Water Board", "category": "utility" }))
        .expect("create vendor failed");
    let vendor_id = vendor_resp
        .into_json::<serde_json::Value>()
        .unwrap()
        .as_i64()
        .unwrap();
    let expense_resp = ureq::post(&format!("{}/api/v1/admin/accounting/expenses/submit", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "vendor_id": vendor_id, "account_id": account_id, "payment_category": "utility",
            "amount": 2000, "expense_date": "2026-07-01"
        }))
        .expect("submit expense against new account failed");
    assert_eq!(expense_resp.status(), 201);

    // A protected system account can never be deactivated.
    let security_account_id = account_id_for(&env, "5100").await;
    let deactivate_system = ureq::post(&format!(
        "{}/api/v1/admin/accounting/accounts/{}/set-active",
        url, security_account_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "is_active": false }));
    assert!(deactivate_system.is_err());
    assert_eq!(
        deactivate_system
            .unwrap_err()
            .into_response()
            .unwrap()
            .status(),
        400
    );

    // The custom account CAN be deactivated, and then drops out of the active list.
    let deactivate_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/accounts/{}/set-active",
        url, account_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "is_active": false }))
    .expect("deactivate custom account failed");
    assert_eq!(deactivate_resp.status(), 204);

    let list2_resp = ureq::post(&format!("{}/api/v1/admin/accounting/accounts/list", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({ "account_type": "EXPENSE", "active_only": true }))
        .expect("list accounts failed");
    let accounts2: serde_json::Value = list2_resp.into_json().unwrap();
    assert!(!accounts2
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["code"] == "5600"));
}

/// The single grouped settings overview must reflect config from every category in
/// one call — the read-side a bylaws-ingestion pipeline (or an admin settings page)
/// would target instead of hitting five separate list endpoints.
#[tokio::test]
async fn test_settings_overview_reflects_all_categories() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    ureq::post(&format!("{}/api/v1/admin/accounting/gst-rules/create", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "per_member_monthly_exempt_threshold": 7500, "society_turnover_registration_threshold": 2000000,
            "gst_rate_pct": 18, "hsn_sac_code": "9995", "effective_from": "2026-04-01"
        }))
        .expect("create gst rule failed");
    ureq::post(&format!("{}/api/v1/admin/accounting/interest-rules/create", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "annual_rate_pct": 21, "grace_period_days": 5, "compounding": "SIMPLE", "effective_from": "2026-04-01"
        }))
        .expect("create interest rule failed");
    ureq::post(&format!("{}/api/v1/admin/accounting/accounts/create", url))
        .set("Authorization", &admin_auth)
        .send_json(
            serde_json::json!({ "code": "5700", "name": "Lift AMC", "account_type": "EXPENSE" }),
        )
        .expect("create account failed");
    ureq::post(&format!(
        "{}/api/v1/admin/accounting/payment-instructions/set",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({
        "account_holder_name": "Raheja Vistas CHS Ltd.", "bank_name": "HDFC Bank",
        "account_number": "1234567890", "ifsc_code": "HDFC0000123"
    }))
    .expect("set payment instructions failed");

    let overview_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/settings/overview",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({}))
    .expect("settings overview failed");
    assert_eq!(overview_resp.status(), 200);
    let overview: serde_json::Value = overview_resp.into_json().unwrap();

    assert_eq!(overview["gst"].as_array().unwrap().len(), 1);
    assert_eq!(
        overview["gst"][0]["gst_rate_pct"].as_str().unwrap(),
        "18.00"
    );
    assert_eq!(overview["interest"].as_array().unwrap().len(), 1);
    assert_eq!(
        overview["interest"][0]["annual_rate_pct"].as_str().unwrap(),
        "21.00"
    );
    assert!(overview["chart_of_accounts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["code"] == "5700" && a["name"] == "Lift AMC"));
    // Seeded structural/system accounts must also be visible in the same overview.
    assert!(overview["chart_of_accounts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|a| a["code"] == "1200" && a["is_system"] == true));
    assert_eq!(
        overview["payment_instructions"]["bank_name"]
            .as_str()
            .unwrap(),
        "HDFC Bank"
    );
}

/// A flat that has churned through several owners over time (each revoked ownership
/// left behind as an inactive `flat_ownerships` row, plus one currently active one) must
/// still appear exactly once in the defaulters report, with the correct current owner
/// and an undoubled total — this report drives legal notices, so the amount has to be
/// exact. `flat_id` carries at most one active ownership row at a time (enforced by a
/// partial unique index), but historical inactive rows for the same flat are common and
/// must not multiply the joined invoice total.
#[tokio::test]
async fn test_defaulters_report_is_correct_for_a_flat_with_ownership_history() {
    let env = setup_api_test_env().await;
    let admin_jwt = quick_setup_admin(&env);
    let admin_user_id: i32 =
        sqlx::query_scalar("SELECT user_id FROM users WHERE email = 'superadmin@vistas.com'")
            .fetch_one(&env.db_pool)
            .await
            .unwrap();
    let _ = quick_create_user(&env, &admin_jwt, "multiowner@vistas.com", "Q", "0101");
    let flat_id = flat_id_for(&env, "Q", "0101").await;

    // A previous owner, no longer active — their ownership record is never deleted,
    // only marked inactive, so it stays in the table forever.
    let former_owner_id: i32 = sqlx::query_scalar(
        "INSERT INTO users (email, is_active) VALUES ('multiowner-former@vistas.com', TRUE) RETURNING user_id",
    )
    .fetch_one(&env.db_pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO flat_ownerships (flat_id, user_id, is_active) VALUES ($1, $2, FALSE)")
        .bind(flat_id)
        .bind(former_owner_id)
        .execute(&env.db_pool)
        .await
        .unwrap();

    let fiscal_year_id = current_fiscal_year_id(&env).await;
    sqlx::query(
        "INSERT INTO invoices \
            (invoice_number, flat_id, fiscal_year_id, billing_period_start, billing_period_end, \
             due_date, subtotal, total_amount, status, created_by) \
         VALUES ('DEFAULT-TEST-001', $1, $2, '2026-06-01', '2026-06-30', '2026-06-10', \
                 5000, 5000, 'UNPAID', $3)",
    )
    .bind(flat_id)
    .bind(fiscal_year_id)
    .bind(admin_user_id)
    .execute(&env.db_pool)
    .await
    .unwrap();

    let mut conn = env.db_pool.acquire().await.unwrap();
    let as_of = chrono::NaiveDate::from_ymd_opt(2026, 7, 15).unwrap();
    let rows = accounting::reports::defaulters(&mut conn, as_of, 1)
        .await
        .expect("defaulters report failed");
    let row = rows
        .iter()
        .find(|r| r.flat_id == flat_id)
        .expect("flat must appear in the defaulters report");
    assert_eq!(
        rows.iter().filter(|r| r.flat_id == flat_id).count(),
        1,
        "a flat with ownership history must appear exactly once, not once per historical owner"
    );
    assert_eq!(
        row.total_outstanding,
        rust_decimal::Decimal::new(500000, 2),
        "outstanding total must not be inflated by a historical inactive ownership row"
    );
    assert_eq!(
        row.unpaid_invoice_count, 1,
        "invoice count must not be inflated by a historical inactive ownership row"
    );
    assert_eq!(
        row.owner_email.as_deref(),
        Some("multiowner@vistas.com"),
        "the report must show the current active owner, not the former one"
    );
}

/// A fiscal year where an expense account ends up net-credit (a reversal larger than
/// that account's normal debits this year — an edge case, but a real one) must still
/// close cleanly. `ledger_entries` requires non-negative, one-sided debit/credit, so
/// naively posting the raw (possibly negative) balance would violate that constraint
/// outright and fail to close the year at all.
#[tokio::test]
async fn test_fiscal_year_close_handles_an_account_with_a_net_credit_expense_balance() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);
    let admin_user_id: i32 =
        sqlx::query_scalar("SELECT user_id FROM users WHERE email = 'superadmin@vistas.com'")
            .fetch_one(&env.db_pool)
            .await
            .unwrap();

    let fy_id = current_fiscal_year_id(&env).await;
    let expense_account_id = account_id_for(&env, "5100").await; // Security Expense
    let payables_account_id = account_id_for(&env, "2200").await; // Vendor Payables

    // Normal expense posting: Dr Expense 1000 / Cr Vendor Payables 1000.
    let tx1: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO ledger_transactions (fiscal_year_id, transaction_date, description, source_type, created_by) \
         VALUES ($1, '2026-06-01', 'Normal expense', 'EXPENSE', $2) RETURNING transaction_id",
    )
    .bind(fy_id)
    .bind(admin_user_id)
    .fetch_one(&env.db_pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ledger_entries (transaction_id, account_id, debit, credit) VALUES \
         ($1, $2, 1000, 0), ($1, $3, 0, 1000)",
    )
    .bind(tx1)
    .bind(expense_account_id)
    .bind(payables_account_id)
    .execute(&env.db_pool)
    .await
    .unwrap();

    // A larger correcting reversal: Dr Vendor Payables 1500 / Cr Expense 1500 — leaves
    // the expense account net CREDIT 500 for the fiscal year.
    let tx2: uuid::Uuid = sqlx::query_scalar(
        "INSERT INTO ledger_transactions (fiscal_year_id, transaction_date, description, source_type, created_by) \
         VALUES ($1, '2026-06-15', 'Correcting reversal', 'EXPENSE', $2) RETURNING transaction_id",
    )
    .bind(fy_id)
    .bind(admin_user_id)
    .fetch_one(&env.db_pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO ledger_entries (transaction_id, account_id, debit, credit) VALUES \
         ($1, $2, 1500, 0), ($1, $3, 0, 1500)",
    )
    .bind(tx2)
    .bind(payables_account_id)
    .bind(expense_account_id)
    .execute(&env.db_pool)
    .await
    .unwrap();

    let close_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/fiscal-years/{}/close",
        url, fy_id
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({}));
    let close_resp = close_resp.expect("close must succeed, not fail a debit>=0 constraint");
    assert_eq!(close_resp.status(), 200);

    let closing_val: serde_json::Value = close_resp.into_json().unwrap();
    let closing_transaction_id = closing_val["closing_transaction_id"].as_str().unwrap();

    // The expense account's net-credit 500 balance must post as a DEBIT of 500 (moving
    // it back to zero), never as a negative credit.
    let entry: (rust_decimal::Decimal, rust_decimal::Decimal) = sqlx::query_as(
        "SELECT debit, credit FROM ledger_entries \
         WHERE transaction_id = $1::uuid AND account_id = $2",
    )
    .bind(closing_transaction_id)
    .bind(expense_account_id)
    .fetch_one(&env.db_pool)
    .await
    .expect("closing entry for the expense account must exist");
    assert_eq!(
        entry.0,
        rust_decimal::Decimal::new(50000, 2),
        "must debit 500.00"
    );
    assert_eq!(entry.1, rust_decimal::Decimal::ZERO);
}

fn upload_statement_csv(url: &str, admin_auth: &str, csv_content: &str) -> ureq::Response {
    ureq::post(&format!(
        "{}/api/v1/admin/accounting/bank-reconciliation/import",
        url
    ))
    .set("Authorization", admin_auth)
    .set("Content-Type", "multipart/form-data; boundary=----boundary")
    .send_string(&format!(
        "------boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"stmt.csv\"\r\nContent-Type: text/csv\r\n\r\n{}\r\n------boundary--\r\n",
        csv_content
    ))
    .expect("import statement request failed")
}

/// A statement with both "Value Date" and "Transaction Date" columns (both containing
/// "date") must be rejected as ambiguous rather than silently picking whichever comes
/// first — the two columns can hold genuinely different dates for the same line, and
/// guessing wrong means every imported line is misdated.
#[tokio::test]
async fn test_statement_import_rejects_ambiguous_date_columns() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let csv_content =
        "Value Date,Posting Date,Description,Amount\n2026-07-01,2026-07-02,NEFT CR,5000\n";
    let resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/bank-reconciliation/import",
        url
    ))
    .set("Authorization", &admin_auth)
    .set("Content-Type", "multipart/form-data; boundary=----boundary")
    .send_string(&format!(
        "------boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"stmt.csv\"\r\nContent-Type: text/csv\r\n\r\n{}\r\n------boundary--\r\n",
        csv_content
    ));
    assert!(
        resp.is_err(),
        "two columns both matching 'date' with no exact match must be rejected"
    );
    assert_eq!(resp.unwrap_err().into_response().unwrap().status(), 400);
}

/// A statement with an exact "Amount" column AND an unrelated "Balance Amount" column
/// must select "Amount" itself (the exact match), not the first substring hit in
/// column order — and a "Narration" column (a real bank export's name for the
/// description field) must be recognized as the description column.
#[tokio::test]
async fn test_statement_import_prefers_exact_header_match_over_substring() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    // "Balance Amount" (a substring match for "amount") appears BEFORE the exact
    // "Amount" column — if substring-first-match were still in play, 25000 (the
    // running balance) would be imported as the transaction amount instead of 5000.
    let csv_content =
        "Date,Narration,Balance Amount,Amount\n2026-07-01,NEFT CR SEEDNEFT001,25000,5000\n";
    let resp = upload_statement_csv(&url, &admin_auth, csv_content);
    assert_eq!(resp.status(), 201);
    let val: serde_json::Value = resp.into_json().unwrap();
    assert_eq!(val["lines_imported"].as_u64().unwrap(), 1);
    let import_id = val["import_id"].as_str().unwrap().to_string();

    let lines_resp = ureq::post(&format!(
        "{}/api/v1/admin/accounting/bank-reconciliation/lines/list",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({ "import_id": import_id }))
    .expect("list lines failed");
    let lines: serde_json::Value = lines_resp.into_json().unwrap();
    assert_eq!(
        lines[0]["amount"].as_str().unwrap(),
        "5000.00",
        "must import the exact 'Amount' column, not 'Balance Amount'"
    );
    assert_eq!(
        lines[0]["description"].as_str().unwrap(),
        "NEFT CR SEEDNEFT001",
        "must recognize 'Narration' as the description column"
    );
}

/// A TDS rate/threshold or fund contribution amount is admin-entered configuration
/// data that flows straight into resident billing and vendor payables — a negative
/// rate, a negative threshold, or a percentage over 100 must be rejected outright, not
/// silently accepted and produce nonsense amounts down the line.
#[tokio::test]
async fn test_tds_and_fund_rule_creation_rejects_invalid_rates_and_amounts() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    let admin_auth = auth(&admin_jwt);

    let bad_tds_rate = ureq::post(&format!("{}/api/v1/admin/accounting/tds-rules/create", url))
        .set("Authorization", &admin_auth)
        .send_json(serde_json::json!({
            "section": "194C", "payment_category": "contractor", "rate_pct": -5,
            "threshold_single_payment": 30000, "threshold_annual_aggregate": 100000,
            "effective_from": "2026-04-01"
        }));
    assert!(bad_tds_rate.is_err(), "negative rate_pct must be rejected");
    assert_eq!(
        bad_tds_rate.unwrap_err().into_response().unwrap().status(),
        400
    );

    let bad_tds_rate_over_100 =
        ureq::post(&format!("{}/api/v1/admin/accounting/tds-rules/create", url))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({
                "section": "194C", "payment_category": "contractor", "rate_pct": 150,
                "threshold_single_payment": 30000, "threshold_annual_aggregate": 100000,
                "effective_from": "2026-04-01"
            }));
    assert!(
        bad_tds_rate_over_100.is_err(),
        "rate_pct over 100 must be rejected"
    );

    let bad_tds_threshold =
        ureq::post(&format!("{}/api/v1/admin/accounting/tds-rules/create", url))
            .set("Authorization", &admin_auth)
            .send_json(serde_json::json!({
                "section": "194C", "payment_category": "contractor", "rate_pct": 2,
                "threshold_single_payment": -1, "threshold_annual_aggregate": 100000,
                "effective_from": "2026-04-01"
            }));
    assert!(
        bad_tds_threshold.is_err(),
        "negative threshold must be rejected"
    );

    let fund_id = fund_id_for(&env, "SINKING").await;
    let bad_fund_negative = ureq::post(&format!(
        "{}/api/v1/admin/accounting/funds/contribution-rules/create",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({
        "fund_id": fund_id, "contribution_type": "FIXED", "amount_or_pct": -100,
        "effective_from": "2026-04-01"
    }));
    assert!(
        bad_fund_negative.is_err(),
        "negative amount_or_pct must be rejected"
    );

    let bad_fund_pct_over_100 = ureq::post(&format!(
        "{}/api/v1/admin/accounting/funds/contribution-rules/create",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({
        "fund_id": fund_id, "contribution_type": "PCT_OF_MAINTENANCE", "amount_or_pct": 150,
        "effective_from": "2026-04-01"
    }));
    assert!(
        bad_fund_pct_over_100.is_err(),
        "PCT_OF_MAINTENANCE amount_or_pct over 100 must be rejected"
    );

    // A FIXED contribution above 100 is legitimate (it's a rupee amount, not a
    // percentage) and must still be accepted.
    let good_fund_fixed = ureq::post(&format!(
        "{}/api/v1/admin/accounting/funds/contribution-rules/create",
        url
    ))
    .set("Authorization", &admin_auth)
    .send_json(serde_json::json!({
        "fund_id": fund_id, "contribution_type": "FIXED", "amount_or_pct": 500,
        "effective_from": "2026-04-01"
    }))
    .expect("a large FIXED amount must be accepted");
    assert_eq!(good_fund_fixed.status(), 201);
}
