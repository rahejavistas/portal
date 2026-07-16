use authentication::{AuthResult, PermissionsManager};
use quality::setup_db_test_env;

/// Tests programmatic user creation, email checking, and user details retrieval by ID.
#[tokio::test]
async fn test_user_creation_and_retrieval() {
    let env = setup_db_test_env().await;
    let pm = PermissionsManager::new(env.db_pool.clone());

    let email = "user@vistas.com";
    let exists = pm.email_exists(email).await.unwrap();
    assert!(!exists);

    let created = pm
        .create_user_with_credentials(email, Some("password123"), "A", "101")
        .await
        .unwrap();
    assert_eq!(created.email, email);
    assert_eq!(created.wing, "A");
    assert_eq!(created.flat_number, "101");

    let retrieved = pm.get_user_by_id(created.user_id).await.unwrap().unwrap();
    assert_eq!(retrieved.user_id, created.user_id);
    assert_eq!(retrieved.email, email);
    assert_eq!(retrieved.wing, "A");
    assert_eq!(retrieved.flat_number, "101");
    assert!(retrieved.designations.is_empty());
}

/// Tests password hashing, correct verification, and incorrect password authentication blocks.
#[tokio::test]
async fn test_authentication_verification() {
    let env = setup_db_test_env().await;
    let pm = PermissionsManager::new(env.db_pool.clone());

    let email = "auth_test@vistas.com";
    let _ = pm
        .create_user_with_credentials(email, Some("secure_pass_123"), "B", "202")
        .await
        .unwrap();

    let success = pm
        .check_authenticated(email, "secure_pass_123")
        .await
        .unwrap();
    let user = match success {
        AuthResult::Success(u) => u,
        _ => panic!("Expected Success"),
    };
    assert_eq!(user.email, email);

    let failure = pm
        .check_authenticated(email, "wrong_password")
        .await
        .unwrap();
    assert!(matches!(failure, AuthResult::InvalidCredentials));

    let invalid_email = pm
        .check_authenticated("unknown@vistas.com", "secure_pass_123")
        .await
        .unwrap();
    assert!(matches!(invalid_email, AuthResult::InvalidCredentials));
}

/// Tests programmatic admin creation (seeding) and role allocation.
#[tokio::test]
async fn test_admin_seeding_and_role() {
    let env = setup_db_test_env().await;
    let pm = PermissionsManager::new(env.db_pool.clone());

    let email = "admin_seed@vistas.com";
    let admin = pm.create_seeded_user(email, "super_admin").await.unwrap();
    assert_eq!(admin.email, email);
    assert_eq!(admin.wing, "TBD");
    assert_eq!(admin.flat_number, "TBD");
    assert_eq!(admin.designations, vec!["super_admin".to_string()]);

    let retrieved = pm.get_user_by_id(admin.user_id).await.unwrap().unwrap();
    assert!(retrieved.has_designation("super_admin"));
    assert!(retrieved.is_committee_or_admin());
}

/// Tests password setup reset token generation, token verification, and password updates.
#[tokio::test]
async fn test_reset_tokens_and_password_updates() {
    let env = setup_db_test_env().await;
    let pm = PermissionsManager::new(env.db_pool.clone());

    let email = "token_user@vistas.com";
    let _ = pm
        .create_user_with_credentials(email, Some("old_password1"), "A", "101")
        .await
        .unwrap();

    let token = "some_secure_reset_token_xyz_123";
    pm.create_reset_token(email, token, 300).await.unwrap();

    let invalid_reset = pm
        .reset_password_with_token("wrong_token", "new_password1")
        .await
        .unwrap();
    assert!(!invalid_reset);

    let valid_reset = pm
        .reset_password_with_token(token, "new_password1")
        .await
        .unwrap();
    assert!(valid_reset);

    let success = pm
        .check_authenticated(email, "new_password1")
        .await
        .unwrap();
    assert!(matches!(success, AuthResult::Success(_)));

    let old_fail = pm
        .check_authenticated(email, "old_password1")
        .await
        .unwrap();
    assert!(matches!(old_fail, AuthResult::InvalidCredentials));
}

/// Tests role assignment changes, successive promotions/demotions, and dynamic audit history queries.
#[tokio::test]
async fn test_role_promotions_and_demotions() {
    let env = setup_db_test_env().await;
    let pm = PermissionsManager::new(env.db_pool.clone());

    let user = pm
        .create_user_with_credentials("promoted@vistas.com", Some("password123"), "C", "303")
        .await
        .unwrap();
    assert!(user.designations.is_empty());

    // 1. Promote to Committee
    pm.set_user_target_role(user.user_id, Some("committee"))
        .await
        .unwrap();
    let u_committee1 = pm.get_user_by_id(user.user_id).await.unwrap().unwrap();
    assert!(u_committee1.has_designation("committee"));
    assert!(u_committee1.is_committee_or_admin());

    // 2. Demote back to Resident
    pm.set_user_target_role(user.user_id, None).await.unwrap();
    let u_resident1 = pm.get_user_by_id(user.user_id).await.unwrap().unwrap();
    assert!(u_resident1.designations.is_empty());
    assert!(!u_resident1.is_committee_or_admin());

    // 3. Re-promote to Committee
    pm.set_user_target_role(user.user_id, Some("committee"))
        .await
        .unwrap();
    let u_committee2 = pm.get_user_by_id(user.user_id).await.unwrap().unwrap();
    assert!(u_committee2.has_designation("committee"));
    assert!(u_committee2.is_committee_or_admin());

    // 4. "admin" is no longer a valid target — super_admin is seed-only, promoted via this
    // endpoint only supports "committee".
    assert!(pm
        .set_user_target_role(user.user_id, Some("admin"))
        .await
        .is_err());

    // 5. Demote to Resident
    pm.set_user_target_role(user.user_id, None).await.unwrap();
    let u_resident2 = pm.get_user_by_id(user.user_id).await.unwrap().unwrap();
    assert!(u_resident2.designations.is_empty());
    assert!(!u_resident2.is_committee_or_admin());

    // 6. Re-promote to Committee again
    pm.set_user_target_role(user.user_id, Some("committee"))
        .await
        .unwrap();
    let u_committee3 = pm.get_user_by_id(user.user_id).await.unwrap().unwrap();
    assert!(u_committee3.has_designation("committee"));
    assert!(u_committee3.is_committee_or_admin());
}

/// Tests aggregated list fetching for all registered users.
#[tokio::test]
async fn test_list_all_users_aggregation() {
    let env = setup_db_test_env().await;
    let pm = PermissionsManager::new(env.db_pool.clone());

    let u1 = pm
        .create_user_with_credentials("member1@vistas.com", Some("password123"), "A", "101")
        .await
        .unwrap();
    let _u2 = pm
        .create_user_with_credentials("member2@vistas.com", Some("password123"), "B", "202")
        .await
        .unwrap();
    let admin = pm
        .create_seeded_user("owner_admin@vistas.com", "super_admin")
        .await
        .unwrap();

    let all = pm.get_all_users().await.unwrap();
    assert_eq!(all.len(), 3);

    let f1 = all.iter().find(|u| u.user_id == u1.user_id).unwrap();
    assert_eq!(f1.email, "member1@vistas.com");
    assert_eq!(f1.wing, "A");
    assert_eq!(f1.flat_number, "101");
    assert!(f1.designations.is_empty());

    let f_admin = all.iter().find(|u| u.user_id == admin.user_id).unwrap();
    assert_eq!(f_admin.email, "owner_admin@vistas.com");
    assert_eq!(f_admin.designations, vec!["super_admin".to_string()]);
}

/// Tests revoking a resident's flat ownership mapping and their subsequent access loss.
#[tokio::test]
async fn test_revoke_flat_ownership() {
    let env = setup_db_test_env().await;
    let pm = PermissionsManager::new(env.db_pool.clone());

    let user = pm
        .create_user_with_credentials("revoke_me@vistas.com", Some("password123"), "C", "303")
        .await
        .unwrap();

    let u1 = pm.get_user_by_id(user.user_id).await.unwrap().unwrap();
    assert_eq!(u1.wing, "C");
    assert_eq!(u1.flat_number, "303");

    // Check they are in the list
    let all_before = pm.get_all_users().await.unwrap();
    assert!(all_before.iter().any(|u| u.user_id == user.user_id));

    // Revoke their only flat
    pm.revoke_flat_ownership(user.user_id, "C", "303")
        .await
        .unwrap();

    let u2 = pm.get_user_by_id(user.user_id).await.unwrap().unwrap();
    assert_eq!(u2.wing, "TBD");
    assert_eq!(u2.flat_number, "TBD");
    assert!(
        !u2.is_active,
        "User should be marked inactive after losing last flat"
    );

    // Check they are unable to login (orphaned)
    let login_result = pm
        .check_authenticated("revoke_me@vistas.com", "password123")
        .await
        .unwrap();
    assert!(matches!(login_result, AuthResult::InvalidCredentials));

    // Check they do not show up in the admin list
    let all_after = pm.get_all_users().await.unwrap();
    assert!(!all_after.iter().any(|u| u.user_id == user.user_id));
}

/// Tests that a user can own multiple flats, and that another user cannot claim an already owned flat.
#[tokio::test]
async fn test_multiple_flat_ownership_and_conflict() {
    let env = setup_db_test_env().await;
    let pm = PermissionsManager::new(env.db_pool.clone());

    // User 1 gets flat D-404
    let user1 = pm
        .create_user_with_credentials("investor@vistas.com", Some("password1"), "D", "404")
        .await
        .unwrap();

    // User 1 ALSO gets flat D-405 (same email, new flat)
    let user1_second = pm
        .create_user_with_credentials("investor@vistas.com", Some("pass2_ignored"), "D", "405")
        .await
        .unwrap();

    // Should be the exact same user ID
    assert_eq!(user1.user_id, user1_second.user_id);

    // The user should now be mapped to both flats
    let u1 = pm.get_user_by_id(user1.user_id).await.unwrap().unwrap();
    assert!(u1.wing.contains("D"));
    assert!(u1.flat_number.contains("404"));
    assert!(u1.flat_number.contains("405"));

    // User 2 tries to claim D-404 which is owned by User 1
    let conflict = pm
        .create_user_with_credentials("thief@vistas.com", Some("password3"), "D", "404")
        .await;

    assert!(conflict.is_err());
    assert_eq!(
        conflict.unwrap_err(),
        "Flat is already actively owned by another user"
    );
}

/// Tests that creating an old orphaned user with a new flat restores their is_active flag.
#[tokio::test]
async fn test_restore_orphaned_user() {
    let env = setup_db_test_env().await;
    let pm = PermissionsManager::new(env.db_pool.clone());

    // 1. Create a user
    let user = pm
        .create_user_with_credentials("comeback@vistas.com", Some("password123"), "E", "505")
        .await
        .unwrap();

    assert!(user.is_active);

    // 2. Revoke their flat -> Should become inactive
    pm.revoke_flat_ownership(user.user_id, "E", "505")
        .await
        .unwrap();
    let u1 = pm.get_user_by_id(user.user_id).await.unwrap().unwrap();
    assert!(!u1.is_active);

    // 3. Admin re-adds them to a new flat (F-606) using the same email
    let restored = pm
        .create_user_with_credentials("comeback@vistas.com", None, "F", "606")
        .await
        .unwrap();

    // 4. Verify their ID is the same, and they are active again
    assert_eq!(restored.user_id, user.user_id);
    assert!(
        restored.is_active,
        "User should be reactivated upon getting a new flat"
    );

    let u2 = pm.get_user_by_id(user.user_id).await.unwrap().unwrap();
    assert!(u2.is_active);
    assert_eq!(u2.wing, "F");
    assert_eq!(u2.flat_number, "606");
}

/// Tests the designation/permission seed data and the assign/check/revoke flow.
#[tokio::test]
async fn test_designations_and_permissions() {
    let env = setup_db_test_env().await;
    let pm = PermissionsManager::new(env.db_pool.clone());

    let designations = pm.list_designations().await.unwrap();
    let codes: Vec<&str> = designations.iter().map(|d| d.code.as_str()).collect();
    assert!(codes.contains(&"super_admin"));
    assert!(codes.contains(&"staff"));
    assert!(!codes.contains(&"admin"));
    assert!(!codes.contains(&"resident"));
    assert!(!codes.contains(&"tenant"));

    // Only super_admin is seeded with permissions; other designations (e.g. staff)
    // get none by default.
    let super_admin_perms = pm.get_designation_permissions("super_admin").await.unwrap();
    assert!(super_admin_perms.contains(&"users.manage".to_string()));
    assert!(super_admin_perms.contains(&"documents.all.manage".to_string()));
    assert!(super_admin_perms.contains(&"process.reviewer".to_string()));
    assert_eq!(super_admin_perms.len(), 13);

    let staff_perms = pm.get_designation_permissions("staff").await.unwrap();
    assert!(staff_perms.is_empty());

    let user = pm
        .create_user_with_credentials("staffer@vistas.com", Some("password123"), "A", "101")
        .await
        .unwrap();

    assert!(!pm
        .user_has_permission(user.user_id, "users.manage")
        .await
        .unwrap());

    pm.assign_designation(user.user_id, "staff", None)
        .await
        .unwrap();

    // Staff has no permissions seeded, so this remains false even though assigned.
    assert!(!pm
        .user_has_permission(user.user_id, "users.manage")
        .await
        .unwrap());
    assert_eq!(
        pm.get_user_designations(user.user_id).await.unwrap(),
        vec!["staff".to_string()]
    );

    pm.revoke_designation(user.user_id, "staff").await.unwrap();

    assert!(pm
        .get_user_designations(user.user_id)
        .await
        .unwrap()
        .is_empty());
}
