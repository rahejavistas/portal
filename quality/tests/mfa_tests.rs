//! Coverage for email-OTP MFA: opt-in per user (via `/profile/mfa/toggle`), a login
//! with MFA off gets a token immediately, a login with MFA on gets `mfa_required` +
//! `pending_token` instead and must complete `/mfa/verify` to actually get a token.

use quality::{quick_create_user, quick_onboard_user, quick_setup_admin, setup_api_test_env};

fn auth(jwt: &str) -> String {
    format!("Bearer {}", jwt)
}

/// Reads the current OTP code directly from the DB — the real flow only ever emails
/// it, so tests need a side channel, same as `quick_onboard_user` does for reset tokens.
async fn fetch_otp_code(env: &quality::TestEnv, email: &str) -> String {
    sqlx::query_scalar::<_, String>(
        "SELECT mfa_otp_code FROM user_credentials \
         WHERE user_id = (SELECT user_id FROM users WHERE email = $1)",
    )
    .bind(email.trim().to_lowercase())
    .fetch_one(&env.db_pool)
    .await
    .expect("otp code not found in db")
}

fn login(url: &str, email: &str, password: &str) -> serde_json::Value {
    ureq::post(&format!("{}/api/v1/login", url))
        .send_json(serde_json::json!({ "email": email, "password": password }))
        .expect("login request failed")
        .into_json()
        .unwrap()
}

#[tokio::test]
async fn test_login_without_mfa_enabled_issues_token_directly() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    quick_create_user(&env, &admin_jwt, "nomfa@mfatest.test", "MF", "101");
    let jwt = quick_onboard_user(&env, "nomfa@mfatest.test", "password123");
    assert!(!jwt.is_empty());

    let resp = login(&url, "nomfa@mfatest.test", "password123");
    assert_eq!(resp["mfa_required"], false);
    assert!(resp["token"].is_string());
}

#[tokio::test]
async fn test_login_with_mfa_enabled_requires_otp_verification() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    quick_create_user(&env, &admin_jwt, "mfaon@mfatest.test", "MF", "102");
    let jwt = quick_onboard_user(&env, "mfaon@mfatest.test", "password123");

    ureq::post(&format!("{}/api/v1/profile/mfa/toggle", url))
        .set("Authorization", &auth(&jwt))
        .send_json(serde_json::json!({ "enabled": true }))
        .expect("enabling mfa should succeed");

    let resp = login(&url, "mfaon@mfatest.test", "password123");
    assert_eq!(resp["mfa_required"], true);
    assert!(resp["token"].is_null());
    let pending_token = resp["pending_token"].as_str().unwrap().to_string();

    let code = fetch_otp_code(&env, "mfaon@mfatest.test").await;
    let verify_resp: serde_json::Value = ureq::post(&format!("{}/api/v1/mfa/verify", url))
        .send_json(serde_json::json!({ "pending_token": pending_token, "code": code }))
        .expect("verify with correct code should succeed")
        .into_json()
        .unwrap();
    assert!(verify_resp["token"].is_string());
}

#[tokio::test]
async fn test_mfa_verify_rejects_wrong_code() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    quick_create_user(&env, &admin_jwt, "mfawrong@mfatest.test", "MF", "103");
    let jwt = quick_onboard_user(&env, "mfawrong@mfatest.test", "password123");
    ureq::post(&format!("{}/api/v1/profile/mfa/toggle", url))
        .set("Authorization", &auth(&jwt))
        .send_json(serde_json::json!({ "enabled": true }))
        .unwrap();

    let resp = login(&url, "mfawrong@mfatest.test", "password123");
    let pending_token = resp["pending_token"].as_str().unwrap().to_string();

    let err = ureq::post(&format!("{}/api/v1/mfa/verify", url))
        .send_json(serde_json::json!({ "pending_token": pending_token, "code": "000000" }));
    assert!(err.is_err(), "a wrong code must be rejected");
}

#[tokio::test]
async fn test_mfa_verify_locks_out_after_five_wrong_attempts() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    quick_create_user(&env, &admin_jwt, "mfalock@mfatest.test", "MF", "104");
    let jwt = quick_onboard_user(&env, "mfalock@mfatest.test", "password123");
    ureq::post(&format!("{}/api/v1/profile/mfa/toggle", url))
        .set("Authorization", &auth(&jwt))
        .send_json(serde_json::json!({ "enabled": true }))
        .unwrap();

    let resp = login(&url, "mfalock@mfatest.test", "password123");
    let pending_token = resp["pending_token"].as_str().unwrap().to_string();
    let real_code = fetch_otp_code(&env, "mfalock@mfatest.test").await;

    for _ in 0..5 {
        let _ = ureq::post(&format!("{}/api/v1/mfa/verify", url))
            .send_json(serde_json::json!({ "pending_token": pending_token, "code": "000000" }));
    }

    // Even the REAL code must now be rejected — the challenge is dead after 5 misses.
    let err = ureq::post(&format!("{}/api/v1/mfa/verify", url))
        .send_json(serde_json::json!({ "pending_token": pending_token, "code": real_code }));
    assert!(
        err.is_err(),
        "the correct code must still be rejected once the attempt limit is hit"
    );
}

#[tokio::test]
async fn test_mfa_resend_issues_a_working_new_code() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    quick_create_user(&env, &admin_jwt, "mfaresend@mfatest.test", "MF", "105");
    let jwt = quick_onboard_user(&env, "mfaresend@mfatest.test", "password123");
    ureq::post(&format!("{}/api/v1/profile/mfa/toggle", url))
        .set("Authorization", &auth(&jwt))
        .send_json(serde_json::json!({ "enabled": true }))
        .unwrap();

    let resp = login(&url, "mfaresend@mfatest.test", "password123");
    let pending_token = resp["pending_token"].as_str().unwrap().to_string();
    let first_code = fetch_otp_code(&env, "mfaresend@mfatest.test").await;

    ureq::post(&format!("{}/api/v1/mfa/resend", url))
        .send_json(serde_json::json!({ "pending_token": pending_token }))
        .expect("resend should succeed");
    let second_code = fetch_otp_code(&env, "mfaresend@mfatest.test").await;
    assert_ne!(first_code, second_code, "resend should issue a fresh code");

    let verify_resp: serde_json::Value = ureq::post(&format!("{}/api/v1/mfa/verify", url))
        .send_json(serde_json::json!({ "pending_token": pending_token, "code": second_code }))
        .expect("verifying with the resent code should succeed")
        .into_json()
        .unwrap();
    assert!(verify_resp["token"].is_string());
}

#[tokio::test]
async fn test_mfa_settings_default_off_and_toggle_roundtrips() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    quick_create_user(&env, &admin_jwt, "mfasettings@mfatest.test", "MF", "106");
    let jwt = quick_onboard_user(&env, "mfasettings@mfatest.test", "password123");

    let initial: serde_json::Value = ureq::post(&format!("{}/api/v1/profile/mfa", url))
        .set("Authorization", &auth(&jwt))
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(initial["mfa_enabled"], false);
    assert_eq!(initial["forced"], false);

    ureq::post(&format!("{}/api/v1/profile/mfa/toggle", url))
        .set("Authorization", &auth(&jwt))
        .send_json(serde_json::json!({ "enabled": true }))
        .unwrap();

    let after: serde_json::Value = ureq::post(&format!("{}/api/v1/profile/mfa", url))
        .set("Authorization", &auth(&jwt))
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(after["mfa_enabled"], true);
}

#[tokio::test]
async fn test_notification_prefs_default_on_and_toggle_roundtrips() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(&env);
    quick_create_user(&env, &admin_jwt, "notifsettings@mfatest.test", "MF", "107");
    let jwt = quick_onboard_user(&env, "notifsettings@mfatest.test", "password123");

    let initial: serde_json::Value = ureq::post(&format!("{}/api/v1/profile/notifications", url))
        .set("Authorization", &auth(&jwt))
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(initial["notify_process_updates"], true);
    assert_eq!(initial["notify_signature_events"], true);

    ureq::post(&format!("{}/api/v1/profile/notifications/toggle", url))
        .set("Authorization", &auth(&jwt))
        .send_json(serde_json::json!({
            "notify_process_updates": false,
            "notify_signature_events": true
        }))
        .unwrap();

    let after: serde_json::Value = ureq::post(&format!("{}/api/v1/profile/notifications", url))
        .set("Authorization", &auth(&jwt))
        .send_json(serde_json::json!({}))
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(after["notify_process_updates"], false);
    assert_eq!(after["notify_signature_events"], true);
}

// ============================================================
// Security: with MFA on, there must be NO path to a session token that
// doesn't go through a correct, unexpired, not-already-used OTP code.
// ============================================================

/// Sets up a fresh user with MFA on, logs in, and returns (jwt, pending_token, real_code).
async fn setup_mfa_login(
    env: &quality::TestEnv,
    email: &str,
    flat: &str,
) -> (String, String, String) {
    let url = env.server_url();
    let admin_jwt = quick_setup_admin(env);
    quick_create_user(env, &admin_jwt, email, "MF", flat);
    let jwt = quick_onboard_user(env, email, "password123");
    ureq::post(&format!("{}/api/v1/profile/mfa/toggle", url))
        .set("Authorization", &auth(&jwt))
        .send_json(serde_json::json!({ "enabled": true }))
        .unwrap();

    let resp = login(&url, email, "password123");
    assert_eq!(
        resp["mfa_required"], true,
        "setup helper expects MFA to be required"
    );
    assert!(
        resp["token"].is_null(),
        "no token may be issued before OTP verification"
    );
    assert!(
        resp["user"].is_null(),
        "no user payload may be issued before OTP verification"
    );
    let pending_token = resp["pending_token"].as_str().unwrap().to_string();
    let code = fetch_otp_code(env, email).await;
    (jwt, pending_token, code)
}

#[tokio::test]
async fn test_correct_password_alone_never_yields_a_token_when_mfa_is_on() {
    let env = setup_api_test_env().await;
    let (_jwt, _pending, _code) = setup_mfa_login(&env, "sec1@mfatest.test", "201").await;
    // setup_mfa_login already asserts token/user are null on the raw login response —
    // this test exists to name that invariant explicitly as its own case.
}

#[tokio::test]
async fn test_empty_code_rejected() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (_jwt, pending_token, _code) = setup_mfa_login(&env, "sec2@mfatest.test", "202").await;

    let err = ureq::post(&format!("{}/api/v1/mfa/verify", url))
        .send_json(serde_json::json!({ "pending_token": pending_token, "code": "" }));
    assert!(err.is_err(), "an empty code must never verify");
}

#[tokio::test]
async fn test_near_miss_codes_rejected() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (_jwt, pending_token, code) = setup_mfa_login(&env, "sec3@mfatest.test", "203").await;

    // Off-by-one on the last digit — makes sure comparison isn't doing anything loose
    // (prefix match, numeric equality ignoring leading zeros, etc).
    let last_digit = code.chars().last().unwrap();
    let bumped = (last_digit.to_digit(10).unwrap() + 1) % 10;
    let near_miss = format!("{}{}", &code[..5], bumped);
    assert_ne!(near_miss, code);

    let err = ureq::post(&format!("{}/api/v1/mfa/verify", url))
        .send_json(serde_json::json!({ "pending_token": pending_token, "code": near_miss }));
    assert!(
        err.is_err(),
        "a near-miss code must be rejected, not loosely matched"
    );
}

#[tokio::test]
async fn test_cannot_replay_pending_token_after_successful_verify() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (_jwt, pending_token, code) = setup_mfa_login(&env, "sec4@mfatest.test", "204").await;

    let first: serde_json::Value = ureq::post(&format!("{}/api/v1/mfa/verify", url))
        .send_json(serde_json::json!({ "pending_token": pending_token, "code": code }))
        .expect("first verify with the correct code should succeed")
        .into_json()
        .unwrap();
    assert!(first["token"].is_string());

    // Replaying the exact same (pending_token, code) pair a second time must fail —
    // both fields get cleared server-side on success specifically to prevent this.
    let replay = ureq::post(&format!("{}/api/v1/mfa/verify", url))
        .send_json(serde_json::json!({ "pending_token": pending_token, "code": code }));
    assert!(
        replay.is_err(),
        "a consumed pending_token+code pair must not be replayable"
    );
}

#[tokio::test]
async fn test_expired_challenge_rejected_even_with_correct_code() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (_jwt, pending_token, code) = setup_mfa_login(&env, "sec5@mfatest.test", "205").await;

    // Force the challenge into the past directly in the DB — simulates real expiry
    // without needing the test to actually sleep 10 minutes.
    sqlx::query(
        "UPDATE user_credentials SET mfa_otp_expires_at = NOW() - INTERVAL '1 minute' \
                 WHERE mfa_pending_token = $1",
    )
    .bind(&pending_token)
    .execute(&env.db_pool)
    .await
    .unwrap();

    let err = ureq::post(&format!("{}/api/v1/mfa/verify", url))
        .send_json(serde_json::json!({ "pending_token": pending_token, "code": code }));
    assert!(
        err.is_err(),
        "an expired challenge must be rejected even with the exactly correct code"
    );
}

#[tokio::test]
async fn test_correct_code_with_wrong_users_pending_token_rejected() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (_jwt_a, pending_a, _code_a) = setup_mfa_login(&env, "sec6a@mfatest.test", "206").await;
    let (_jwt_b, _pending_b, code_b) = setup_mfa_login(&env, "sec6b@mfatest.test", "207").await;

    // User B's real code, submitted against user A's pending_token — must not work,
    // proving the code is checked against the SPECIFIC challenge the token names, not
    // just "some" currently-valid code anywhere in the system.
    let err = ureq::post(&format!("{}/api/v1/mfa/verify", url))
        .send_json(serde_json::json!({ "pending_token": pending_a, "code": code_b }));
    assert!(
        err.is_err(),
        "another user's correct code must not verify a different pending_token"
    );
}

#[tokio::test]
async fn test_garbage_pending_token_rejected() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let _ = setup_mfa_login(&env, "sec7@mfatest.test", "208").await;

    let err = ureq::post(&format!("{}/api/v1/mfa/verify", url)).send_json(
        serde_json::json!({ "pending_token": "totally-made-up-token-xyz", "code": "123456" }),
    );
    assert!(
        err.is_err(),
        "a made-up pending_token must never verify, regardless of code"
    );
}

#[tokio::test]
async fn test_pending_token_is_not_usable_as_a_session_token() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (_jwt, pending_token, _code) = setup_mfa_login(&env, "sec8@mfatest.test", "209").await;

    // The pending_token is an opaque OTP-challenge identifier, not a JWT — it must not
    // grant access to any authenticated endpoint if used as a Bearer token.
    let resp = ureq::post(&format!("{}/api/v1/profile", url))
        .set("Authorization", &format!("Bearer {}", pending_token))
        .send_json(serde_json::json!({}));
    assert!(
        resp.is_err(),
        "pending_token must not work as a session Bearer token"
    );
}

#[tokio::test]
async fn test_resend_capped_then_requires_fresh_login() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (_jwt, pending_token, _code) = setup_mfa_login(&env, "sec9@mfatest.test", "210").await;

    // create_mfa_challenge (from login) counts as attempt 0; 3 resends are allowed.
    for _ in 0..3 {
        let resp = ureq::post(&format!("{}/api/v1/mfa/resend", url))
            .send_json(serde_json::json!({ "pending_token": pending_token }));
        assert!(resp.is_ok(), "resends within the cap should succeed");
    }

    // The 4th resend must be refused.
    let over_cap = ureq::post(&format!("{}/api/v1/mfa/resend", url))
        .send_json(serde_json::json!({ "pending_token": pending_token }));
    assert!(over_cap.is_err(), "resends past the cap must be refused");

    // The most recent (3rd) code must still work — being over the resend cap doesn't
    // invalidate the current still-valid code, it just blocks getting ANOTHER one.
    let latest_code = fetch_otp_code(&env, "sec9@mfatest.test").await;
    let verify: serde_json::Value = ureq::post(&format!("{}/api/v1/mfa/verify", url))
        .send_json(serde_json::json!({ "pending_token": pending_token, "code": latest_code }))
        .expect("the last successfully-resent code should still verify")
        .into_json()
        .unwrap();
    assert!(verify["token"].is_string());
}

#[tokio::test]
async fn test_second_login_issues_a_fresh_challenge_invalidating_the_old_one() {
    let env = setup_api_test_env().await;
    let url = env.server_url();
    let (_jwt, first_pending, first_code) =
        setup_mfa_login(&env, "sec10@mfatest.test", "211").await;

    // Log in again without ever completing the first challenge.
    let second_resp = login(&url, "sec10@mfatest.test", "password123");
    let second_pending = second_resp["pending_token"].as_str().unwrap().to_string();
    assert_ne!(
        first_pending, second_pending,
        "each login must mint a new pending_token"
    );

    // Even the FIRST challenge's own correct code, against its own original
    // pending_token, must now fail — the second login overwrote that user's single
    // challenge row entirely, so the first pending_token no longer matches anything.
    let err = ureq::post(&format!("{}/api/v1/mfa/verify", url))
        .send_json(serde_json::json!({ "pending_token": first_pending, "code": first_code }));
    assert!(err.is_err(), "an abandoned earlier pending_token must not still be usable, even with its own correct code");
}
