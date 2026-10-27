use authentication::{AuthResult, DesignationChangeError, PermissionsManager};
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
        .create_user_with_credentials(email, Some("password123"), "A", "101", None)
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
        .create_user_with_credentials(email, Some("secure_pass_123"), "B", "202", None)
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
    assert!(retrieved.is_admin());
}

/// Tests password setup reset token generation, token verification, and password updates.
#[tokio::test]
async fn test_reset_tokens_and_password_updates() {
    let env = setup_db_test_env().await;
    let pm = PermissionsManager::new(env.db_pool.clone());

    let email = "token_user@vistas.com";
    let _ = pm
        .create_user_with_credentials(email, Some("old_password1"), "A", "101", None)
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

/// Tests generic designation assign/revoke, successive promotions/demotions, idempotent
/// re-assignment, and rejection of unknown designation codes.
#[tokio::test]
async fn test_role_promotions_and_demotions() {
    let env = setup_db_test_env().await;
    let pm = PermissionsManager::new(env.db_pool.clone());

    let user = pm
        .create_user_with_credentials("promoted@vistas.com", Some("password123"), "C", "303", None)
        .await
        .unwrap();
    assert!(user.designations.is_empty());

    pm.create_designation("committee", "Committee", None)
        .await
        .unwrap();

    // 1. Promote to Committee
    pm.assign_designation(user.user_id, "committee", None)
        .await
        .unwrap();
    let u_committee1 = pm.get_user_by_id(user.user_id).await.unwrap().unwrap();
    assert!(u_committee1.has_designation("committee"));

    // 2. Demote back to Resident
    pm.revoke_designation(user.user_id, "committee", None)
        .await
        .unwrap();
    let u_resident1 = pm.get_user_by_id(user.user_id).await.unwrap().unwrap();
    assert!(u_resident1.designations.is_empty());

    // 3. Re-promote to Committee
    pm.assign_designation(user.user_id, "committee", None)
        .await
        .unwrap();
    let u_committee2 = pm.get_user_by_id(user.user_id).await.unwrap().unwrap();
    assert!(u_committee2.has_designation("committee"));

    // 4. Assigning an unknown designation code is rejected.
    assert!(pm
        .assign_designation(user.user_id, "admin", None)
        .await
        .is_err());

    // 5. Re-assigning a designation the user already actively holds is a no-op, not a
    // duplicate active row (exercises the partial-unique-index ON CONFLICT fix).
    pm.assign_designation(user.user_id, "committee", None)
        .await
        .unwrap();
    assert_eq!(
        pm.get_user_designations(user.user_id).await.unwrap(),
        vec!["committee".to_string()]
    );

    // 6. Demote to Resident
    pm.revoke_designation(user.user_id, "committee", None)
        .await
        .unwrap();
    let u_resident2 = pm.get_user_by_id(user.user_id).await.unwrap().unwrap();
    assert!(u_resident2.designations.is_empty());
}

/// Tests aggregated list fetching for all registered users.
#[tokio::test]
async fn test_list_all_users_aggregation() {
    let env = setup_db_test_env().await;
    let pm = PermissionsManager::new(env.db_pool.clone());

    let u1 = pm
        .create_user_with_credentials("member1@vistas.com", Some("password123"), "A", "101", None)
        .await
        .unwrap();
    let _u2 = pm
        .create_user_with_credentials("member2@vistas.com", Some("password123"), "B", "202", None)
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
        .create_user_with_credentials(
            "revoke_me@vistas.com",
            Some("password123"),
            "C",
            "303",
            None,
        )
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
        .create_user_with_credentials("investor@vistas.com", Some("password1"), "D", "404", None)
        .await
        .unwrap();

    // User 1 ALSO gets flat D-405 (same email, new flat)
    let user1_second = pm
        .create_user_with_credentials(
            "investor@vistas.com",
            Some("pass2_ignored"),
            "D",
            "405",
            None,
        )
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
        .create_user_with_credentials("thief@vistas.com", Some("password3"), "D", "404", None)
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
        .create_user_with_credentials("comeback@vistas.com", Some("password123"), "E", "505", None)
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
        .create_user_with_credentials("comeback@vistas.com", None, "F", "606", None)
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

    // super_admin and staff are seeded; every other designation is created through the UI.
    let designations = pm.list_designations().await.unwrap();
    let codes: Vec<&str> = designations.iter().map(|d| d.code.as_str()).collect();
    assert!(codes.contains(&"super_admin"));
    assert!(codes.contains(&"staff"));
    assert_eq!(codes.len(), 2);

    pm.create_designation("custodian", "Custodian", None)
        .await
        .unwrap();
    let designations = pm.list_designations().await.unwrap();
    let codes: Vec<&str> = designations.iter().map(|d| d.code.as_str()).collect();
    assert!(codes.contains(&"super_admin"));
    assert!(codes.contains(&"custodian"));
    assert!(!codes.contains(&"admin"));
    assert!(!codes.contains(&"resident"));
    assert!(!codes.contains(&"tenant"));

    // Only super_admin and staff are seeded with permissions; other designations (e.g.
    // custodian, created above) get none by default.
    let super_admin_perms = pm.get_designation_permissions("super_admin").await.unwrap();
    assert!(super_admin_perms.contains(&"users.manage".to_string()));
    assert!(super_admin_perms.contains(&"documents.all.manage".to_string()));
    assert!(super_admin_perms.contains(&"process.reviewer".to_string()));
    assert_eq!(super_admin_perms.len(), 12);

    let custodian_perms = pm.get_designation_permissions("custodian").await.unwrap();
    assert!(custodian_perms.is_empty());

    let user = pm
        .create_user_with_credentials("staffer@vistas.com", Some("password123"), "A", "101", None)
        .await
        .unwrap();

    assert!(!pm
        .user_has_permission(user.user_id, "users.manage")
        .await
        .unwrap());

    pm.assign_designation(user.user_id, "custodian", None)
        .await
        .unwrap();

    // Custodian has no permissions seeded, so this remains false even though assigned.
    assert!(!pm
        .user_has_permission(user.user_id, "users.manage")
        .await
        .unwrap());
    assert_eq!(
        pm.get_user_designations(user.user_id).await.unwrap(),
        vec!["custodian".to_string()]
    );

    pm.revoke_designation(user.user_id, "custodian", None)
        .await
        .unwrap();

    assert!(pm
        .get_user_designations(user.user_id)
        .await
        .unwrap()
        .is_empty());
}

/// Revoking two permissions from one designation at the same moment must leave every
/// holder with neither — the per-request permission cache cannot keep a revoked grant.
#[tokio::test]
async fn test_concurrent_permission_revokes_leave_no_stale_grant() {
    let env = setup_db_test_env().await;
    let pm = PermissionsManager::new(env.db_pool.clone());

    pm.create_designation("auditor", "Auditor", None)
        .await
        .unwrap();
    let user = pm
        .create_user_with_credentials("auditor@vistas.com", Some("password123"), "A", "101", None)
        .await
        .unwrap();
    pm.assign_designation(user.user_id, "auditor", None)
        .await
        .unwrap();

    for _ in 0..25 {
        for permission in ["process.reviewer", "process.view"] {
            pm.toggle_designation_permission("auditor", permission, true, None)
                .await
                .unwrap();
        }
        let (a, b) = tokio::join!(
            pm.toggle_designation_permission("auditor", "process.reviewer", false, None),
            pm.toggle_designation_permission("auditor", "process.view", false, None),
        );
        a.unwrap();
        b.unwrap();

        let reloaded = pm.get_user_by_id(user.user_id).await.unwrap().unwrap();
        assert!(
            reloaded.permissions.is_empty(),
            "stale grants after concurrent revoke: {:?}",
            reloaded.permissions
        );
    }
}

/// Assigning is refused when the designation carries a permission the assigner lacks,
/// and allowed once they hold all of them.
#[tokio::test]
async fn test_assign_designation_within_assigner_permissions() {
    let env = setup_db_test_env().await;
    let pm = PermissionsManager::new(env.db_pool.clone());

    pm.create_designation("reviewer_role", "Reviewer Role", None)
        .await
        .unwrap();
    pm.toggle_designation_permission("reviewer_role", "process.reviewer", true, None)
        .await
        .unwrap();
    pm.toggle_designation_permission("reviewer_role", "process.view", true, None)
        .await
        .unwrap();
    let target = pm
        .create_user_with_credentials("target@vistas.com", Some("password123"), "A", "101", None)
        .await
        .unwrap();

    let only_view = vec!["process.view".to_string()];
    let err = pm
        .assign_designation_within(target.user_id, "reviewer_role", 1, &only_view)
        .await
        .unwrap_err();
    match err {
        DesignationChangeError::MissingPermissions(missing) => {
            assert_eq!(missing, vec!["process.reviewer".to_string()])
        }
        other => panic!("expected MissingPermissions, got {other:?}"),
    }
    assert!(pm
        .get_user_designations(target.user_id)
        .await
        .unwrap()
        .is_empty());

    let both = vec!["process.view".to_string(), "process.reviewer".to_string()];
    pm.assign_designation_within(target.user_id, "reviewer_role", 1, &both)
        .await
        .unwrap();
    assert_eq!(
        pm.get_user_designations(target.user_id).await.unwrap(),
        vec!["reviewer_role".to_string()]
    );

    let unknown = pm
        .assign_designation_within(target.user_id, "no_such_role", 1, &both)
        .await
        .unwrap_err();
    assert!(matches!(
        unknown,
        DesignationChangeError::UnknownDesignation(_)
    ));
}

/// A delete racing an assign must end one of two ways: the assign lands and the delete is
/// refused, or the delete lands and the assign is refused — never an active holder of a
/// deleted designation.
#[tokio::test]
async fn test_concurrent_assign_and_delete_never_leave_holder_of_deleted_designation() {
    let env = setup_db_test_env().await;
    let pm = PermissionsManager::new(env.db_pool.clone());
    let user = pm
        .create_user_with_credentials("holder@vistas.com", Some("password123"), "A", "101", None)
        .await
        .unwrap();

    for round in 0..25 {
        let code = format!("temp_role_{round}");
        pm.create_designation(&code, "Temp Role", None)
            .await
            .unwrap();

        let (assigned, deleted) = tokio::join!(
            pm.assign_designation(user.user_id, &code, None),
            pm.delete_designation(&code, None),
        );
        assert!(
            assigned.is_ok() != deleted.is_ok(),
            "exactly one must win in round {round}: assign={assigned:?} delete={deleted:?}"
        );

        let holders_of_deleted: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM user_designations ud \
             JOIN designations d ON d.designation_id = ud.designation_id \
             WHERE ud.is_active = TRUE AND d.is_active = FALSE",
        )
        .fetch_one(&env.db_pool)
        .await
        .unwrap();
        assert_eq!(holders_of_deleted, 0, "round {round}");
    }
}

/// Revoking is refused when the designation carries a permission the revoker lacks; a
/// refused revoke leaves the holder in place.
#[tokio::test]
async fn test_revoke_designation_within_revoker_permissions() {
    let env = setup_db_test_env().await;
    let pm = PermissionsManager::new(env.db_pool.clone());

    pm.create_designation("powerful_role", "Powerful Role", None)
        .await
        .unwrap();
    pm.toggle_designation_permission("powerful_role", "process.reviewer", true, None)
        .await
        .unwrap();
    let holder = pm
        .create_user_with_credentials("holder2@vistas.com", Some("password123"), "A", "102", None)
        .await
        .unwrap();
    pm.assign_designation(holder.user_id, "powerful_role", None)
        .await
        .unwrap();

    let none: Vec<String> = Vec::new();
    let err = pm
        .revoke_designation_within(holder.user_id, "powerful_role", 1, &none)
        .await
        .unwrap_err();
    assert!(matches!(err, DesignationChangeError::MissingPermissions(_)));
    assert_eq!(
        pm.get_user_designations(holder.user_id).await.unwrap(),
        vec!["powerful_role".to_string()]
    );

    let held = vec!["process.reviewer".to_string()];
    assert!(pm
        .revoke_designation_within(holder.user_id, "powerful_role", 1, &held)
        .await
        .unwrap());
    assert!(!pm
        .revoke_designation_within(holder.user_id, "powerful_role", 1, &held)
        .await
        .unwrap());
}
