use authentication::PermissionsManager;
use axum::{
    extract::DefaultBodyLimit,
    middleware as axum_middleware,
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;

use file_server::config::AppConfig;
use file_server::state::AppState;
use file_server::{document, handlers, middleware};

#[tokio::main]
async fn main() {
    let config = AppConfig::load_config();

    if let Err(e) = telemetry::init_telemetry(
        config.telemetry.otlp_endpoint.clone(),
        config.telemetry.otlp_username.clone(),
        config.telemetry.otlp_password.clone(),
    ) {
        eprintln!("Warning: Failed to initialize telemetry: {}", e);
    }

    // Initialize Postgres connection pool
    let pg_pool = base::postgres::create_pool(&config.get_postgres_config())
        .await
        .expect("Failed to create Postgres pool");

    let s3_accessor = base::s3::S3Accessor::new(&config.s3).await;

    // Initialize PermissionsManager
    let permissions_manager = Arc::new(PermissionsManager::new(pg_pool.clone()));

    let document_manager = Arc::new(document::DocumentManager::new(
        pg_pool.clone(),
        s3_accessor.clone(),
    ));

    seed_admin_user_or_die(
        &permissions_manager,
        &config.super_admin_email,
        "super_admin",
    )
    .await;

    let rate_limiters = Arc::new(middleware::RateLimiters::new(
        config.rate_limit.enabled,
        config.rate_limit.ip_per_min,
        config.rate_limit.user_per_min,
        config.rate_limit.admin_per_min,
    ));

    let signyu_client = config.signyu.api_key.clone().map(|api_key| {
        std::sync::Arc::new(file_server::signature_providers::signyu::SignYuClient::new(
            config.signyu.base_url.clone(),
            api_key,
            pg_pool.clone(),
        ))
    });

    // Initialize shared application state (completely stateless!)
    let state = AppState {
        permissions_manager,
        document_manager,
        pg_pool: pg_pool.clone(),
        jwt_secret: config.jwt_secret.clone(),
        jwt_expiry: config.jwt_expiry,
        forgot_password_expiry: config.forgot_password_expiry,
        rate_limiters,
        super_admin_email: config.super_admin_email.clone(),
        signyu_client,
        signyu_webhook_secret: config.signyu.webhook_secret.clone(),
    };

    macro_rules! make_app {
        ($state:expr) => {{
            // Each admin route group requires a specific permission.
            use authentication::Permission;

            // Also reachable by roles-management/view permissions since the User Designations
            // subtab needs this same listing to know who to assign designations to.
            let users_view_routes = Router::new()
                .route("/users/list", post(handlers::admin::list_users))
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_any_of(&[
                        Permission::UsersView,
                        Permission::UsersManage,
                        Permission::UsersRolesManage,
                        Permission::UsersRolesView,
                    ]),
                ));

            let users_manage_routes = Router::new()
                .route("/users/create", post(handlers::admin::create_user))
                .route("/users/create-staff", post(handlers::admin::create_staff))
                .route(
                    "/users/revoke-flat",
                    post(handlers::admin::admin_revoke_flat),
                )
                .route(
                    "/users/upload",
                    post(handlers::bulk_users_upload::upload_users),
                )
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_any_of(&[Permission::UsersManage]),
                ));

            // No blanket permission layer — each handler checks per-category access.
            let documents_manage_routes = Router::new()
                .route(
                    "/documents/list",
                    post(handlers::documents::admin_list_documents),
                )
                .route(
                    "/documents/categories",
                    post(handlers::documents::admin_list_manageable_categories),
                )
                .route(
                    "/documents/flat-owners",
                    post(handlers::admin::lookup_flat_owners),
                )
                .route(
                    "/documents/upload-url",
                    post(handlers::documents::admin_upload_url),
                )
                .route(
                    "/documents/confirm-upload",
                    post(handlers::documents::admin_confirm_upload),
                )
                .route(
                    "/documents/:id/get",
                    post(handlers::documents::admin_get_document),
                )
                .route(
                    "/documents/:id/update",
                    post(handlers::documents::admin_update_document),
                )
                .route(
                    "/documents/:id/delete",
                    post(handlers::documents::admin_delete_document),
                );

            let process_reviewer_routes = Router::new()
                .route(
                    "/process/review",
                    post(handlers::process::admin_review_step),
                )
                .route(
                    "/process/upload-url",
                    post(handlers::process::process_admin_upload_url),
                )
                .route(
                    "/process/confirm-upload",
                    post(handlers::process::process_admin_confirm_upload),
                )
                .route(
                    "/process/revoke",
                    post(handlers::process::admin_revoke_step),
                )
                .route(
                    "/process/add-note",
                    post(handlers::process::admin_add_note),
                )
                .route(
                    "/process/documents/add-signers",
                    post(handlers::process::admin_add_document_signers),
                )
                .route(
                    "/documents/signatures/provider-status",
                    post(handlers::signatures::provider_status),
                )
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_any_of(&[Permission::ProcessReviewer]),
                ));

            // Assigning/revoking an existing designation to/from a user.
            let roles_user_designation_routes = Router::new()
                .route(
                    "/roles/designations/assign",
                    post(handlers::roles::assign_designation),
                )
                .route(
                    "/roles/designations/revoke",
                    post(handlers::roles::revoke_designation),
                )
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_any_of(&[Permission::UsersRolesManage]),
                ));

            // Creating designations and editing their permissions/category access.
            let roles_designation_definition_routes = Router::new()
                .route(
                    "/roles/designations/create",
                    post(handlers::roles::create_designation),
                )
                .route(
                    "/roles/designations/delete",
                    post(handlers::roles::delete_designation),
                )
                .route(
                    "/roles/designations/rename",
                    post(handlers::roles::rename_designation),
                )
                .route(
                    "/roles/designations/permissions/toggle",
                    post(handlers::roles::toggle_designation_permission),
                )
                .route(
                    "/roles/document-category-designations/toggle",
                    post(handlers::roles::toggle_document_category_designation),
                )
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_any_of(&[Permission::DesignationsManage]),
                ));

            let document_categories_manage_routes = Router::new()
                .route(
                    "/roles/document-categories/create",
                    post(handlers::roles::create_document_category),
                )
                .route(
                    "/roles/document-categories/delete",
                    post(handlers::roles::delete_document_category),
                )
                .route(
                    "/roles/document-categories/rename",
                    post(handlers::roles::rename_document_category),
                )
                .route(
                    "/roles/document-subcategories/create",
                    post(handlers::roles::create_document_subcategory),
                )
                .route(
                    "/roles/document-subcategories/rename",
                    post(handlers::roles::rename_document_subcategory),
                )
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_any_of(&[Permission::DocumentsCategoriesManage]),
                ));

            // Read-only listings shared by the roles/category-management capabilities above.
            let roles_shared_read_routes = Router::new()
                .route(
                    "/roles/designations",
                    post(handlers::roles::list_designations),
                )
                .route(
                    "/roles/permissions",
                    post(handlers::roles::list_permissions),
                )
                .route(
                    "/roles/document-category-designations",
                    post(handlers::roles::list_document_category_designations),
                )
                .route(
                    "/roles/document-categories",
                    post(handlers::roles::list_document_categories),
                )
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_any_of(&[
                        Permission::UsersRolesManage,
                        Permission::DesignationsManage,
                        Permission::DocumentsCategoriesManage,
                        // The template builder's "Eligible Signer Designations" picker
                        // needs a plain read-only list of designation codes/labels —
                        // without this, anyone who can manage templates but not
                        // roles/designations gets a silent 403 here and sees an empty
                        // picker with nothing to choose from.
                        Permission::ProcessTemplatesManage,
                    ]),
                ));

            let process_templates_manage_routes = Router::new()
                .route(
                    "/process-templates/list",
                    post(handlers::process_templates::list),
                )
                .route(
                    "/process-templates/get-version",
                    post(handlers::process_templates::get_version),
                )
                .route(
                    "/process-templates/save",
                    post(handlers::process_templates::save),
                )
                .route(
                    "/process-templates/activate",
                    post(handlers::process_templates::activate),
                )
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_any_of(&[Permission::ProcessTemplatesManage]),
                ));

            // No signatures-specific permission gates these — signing eligibility is
            // "holds any designation" (see `User::has_any_designation`), and each
            // handler is self-scoped anyway: `queue` only ever returns the caller's own
            // requirements, and `replace-upload-url`/`replace-confirm-upload` require the
            // caller to actually BE the requirement's signer. Authentication alone (the
            // `SessionUser` extractor) is enough at the router level.
            let document_signatures_sign_routes = Router::new()
                .route(
                    "/documents/signatures/queue",
                    post(handlers::signatures::queue),
                )
                .route(
                    "/documents/signatures/replace-upload-url",
                    post(handlers::signatures::replace_upload_url),
                )
                .route(
                    "/documents/signatures/replace-confirm-upload",
                    post(handlers::signatures::replace_confirm_upload),
                )
                .route(
                    "/documents/signatures/document-download-url",
                    post(handlers::signatures::document_download_url),
                );

            let document_signatures_read_routes = Router::new()
                .route("/documents/signatures/all", post(handlers::signatures::all))
                .route(
                    "/documents/signatures/refresh-status",
                    post(handlers::signatures::refresh_status),
                )
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_any_of(&[
                        Permission::ProcessReviewer,
                        Permission::ProcessView,
                        Permission::DocumentsAllManage,
                    ]),
                ));

            // Signer directory oversight (set-active/full list) belongs to whoever
            // attaches signers in the first place — process.reviewer — there is no
            // separate signatures-management permission. Deliberately no delete route:
            // a signer is never hard-removed, only deactivated (reversible).
            let signer_directory_manage_routes = Router::new()
                .route("/signers/set-active", post(handlers::signers::set_active))
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_any_of(&[Permission::ProcessReviewer]),
                ));

            // /signers/list itself scopes its response by an `include_inactive` request
            // flag (not by permission) — see the handler.
            let signer_directory_read_routes = Router::new()
                .route("/signers/list", post(handlers::signers::list))
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_any_of(&[Permission::ProcessReviewer]),
                ));

            let admin_routes = users_view_routes
                .merge(users_manage_routes)
                .merge(documents_manage_routes)
                .merge(process_reviewer_routes)
                .merge(roles_user_designation_routes)
                .merge(roles_designation_definition_routes)
                .merge(document_categories_manage_routes)
                .merge(roles_shared_read_routes)
                .merge(process_templates_manage_routes)
                .merge(document_signatures_sign_routes)
                .merge(document_signatures_read_routes)
                .merge(signer_directory_manage_routes)
                .merge(signer_directory_read_routes);

            Router::new()
                .route("/health", get(handlers::health::health_check))
                .route(
                    "/webhooks/signyu",
                    post(handlers::signatures::signyu_webhook),
                )
                .route("/api/v1/login", post(handlers::auth::login))
                .route("/api/v1/mfa/verify", post(handlers::auth::mfa_verify))
                .route("/api/v1/mfa/resend", post(handlers::auth::mfa_resend))
                .route(
                    "/api/v1/forgot-password",
                    post(handlers::auth::forgot_password),
                )
                .route(
                    "/api/v1/reset-password",
                    post(handlers::auth::reset_password),
                )
                .route("/api/v1/profile", post(handlers::auth::profile))
                .route(
                    "/api/v1/profile/mfa",
                    post(handlers::auth::get_mfa_settings),
                )
                .route(
                    "/api/v1/profile/mfa/toggle",
                    post(handlers::auth::set_mfa_settings),
                )
                .route(
                    "/api/v1/profile/notifications",
                    post(handlers::auth::get_notification_settings),
                )
                .route(
                    "/api/v1/profile/notifications/toggle",
                    post(handlers::auth::set_notification_settings),
                )
                .route(
                    "/api/v1/profile/signer-info",
                    post(handlers::auth::get_signer_info),
                )
                .route(
                    "/api/v1/profile/signer-info/save",
                    post(handlers::auth::save_signer_info),
                )
                .route(
                    "/api/v1/documents",
                    post(handlers::documents::list_documents),
                )
                .route(
                    "/api/v1/documents/:id",
                    post(handlers::documents::get_document),
                )
                .route(
                    "/api/v1/documents/upload-url",
                    post(handlers::documents::user_upload_url),
                )
                .route(
                    "/api/v1/documents/confirm-upload",
                    post(handlers::documents::user_confirm_upload),
                )
                .route(
                    "/api/v1/process-templates/available",
                    post(handlers::process_templates::available),
                )
                .route(
                    "/api/v1/process/list",
                    post(handlers::process::list_user_processes),
                )
                .route(
                    "/api/v1/process/state",
                    post(handlers::process::get_process_state),
                )
                .route(
                    "/api/v1/process/save-step",
                    post(handlers::process::save_process_step),
                )

                .route(
                    "/api/v1/process/download-url",
                    post(handlers::process::process_download_url),
                )
                .nest("/api/v1/admin", admin_routes)
                // Standard CORS and custom security headers middleware
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::rate_limit_middleware,
                ))
                .layer(DefaultBodyLimit::max(1024 * 1024 * 1024 + 10 * 1024 * 1024)) // 1GB + 10MB overhead
                .layer(axum_middleware::from_fn(middleware::add_security_headers))
                .layer(axum_middleware::from_fn(middleware::log_request))
                .with_state($state)
        }};
    }

    let is_lambda = std::env::var("AWS_LAMBDA_RUNTIME_API").is_ok();

    if is_lambda {
        let app = make_app!(state);
        // Do NOT apply the Rust CORS layer in Lambda. AWS Function URL handles CORS!
        lambda_http::run(app).await.unwrap();
    } else {
        let app = make_app!(state).layer(middleware::get_cors_layer()); // Only apply CORS locally
        let addr = SocketAddr::from(([0, 0, 0, 0], config.port));

        axum::Server::bind(&addr)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .unwrap();
    }
}

async fn seed_admin_user_or_die(
    permissions_manager: &PermissionsManager,
    email: &str,
    designation_code: &str,
) {
    match permissions_manager.email_exists(email).await {
        Ok(false) => {
            if let Err(e) = permissions_manager
                .create_seeded_user(email, designation_code)
                .await
            {
                eprintln!("Failed to seed admin user: {}", e);
            }
        }
        Ok(true) => {}
        Err(e) => {
            eprintln!("Error checking admin user existence: {}", e);
        }
    }
}
