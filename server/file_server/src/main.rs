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

    // Run database migrations automatically on startup (SQLx embeds them in the binary)
    if let Err(e) = sqlx::migrate!("../../infra/migrations").run(&pg_pool).await {
        eprintln!("WARNING: Database migrations failed: {}", e);
    }

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
                    middleware::require_permission(&[
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
                    middleware::require_permission(&[Permission::UsersManage]),
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
                    "/process/revoke",
                    post(handlers::process::admin_revoke_step),
                )
                .route(
                    "/process/add-note",
                    post(handlers::process::admin_add_note),
                )
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_permission(&[Permission::ProcessReviewer]),
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
                    middleware::require_permission(&[Permission::UsersRolesManage]),
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
                    middleware::require_permission(&[Permission::DesignationsManage]),
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
                    middleware::require_permission(&[Permission::DocumentsCategoriesManage]),
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
                    middleware::require_permission(&[
                        Permission::UsersRolesManage,
                        Permission::DesignationsManage,
                        Permission::DocumentsCategoriesManage,
                    ]),
                ));

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
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_permission(&[Permission::DocumentsSignaturesSign]),
                ));

            let document_signatures_manage_routes = Router::new()
                .route(
                    "/documents/signatures/cancel",
                    post(handlers::signatures::cancel),
                )
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_permission(&[Permission::DocumentsSignaturesManage]),
                ));

            let document_signatures_read_routes = Router::new()
                .route(
                    "/documents/signatures/for-document",
                    post(handlers::signatures::for_document),
                )
                .route("/documents/signatures/all", post(handlers::signatures::all))
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_permission(&[
                        Permission::DocumentsSignaturesSign,
                        Permission::DocumentsSignaturesManage,
                        Permission::ProcessReviewer,
                        Permission::ProcessView,
                        Permission::DocumentsAllManage,
                    ]),
                ));

            let admin_routes = users_view_routes
                .merge(users_manage_routes)
                .merge(documents_manage_routes)
                .merge(process_reviewer_routes)
                .merge(roles_user_designation_routes)
                .merge(roles_designation_definition_routes)
                .merge(document_categories_manage_routes)
                .merge(roles_shared_read_routes)
                .merge(document_signatures_sign_routes)
                .merge(document_signatures_manage_routes)
                .merge(document_signatures_read_routes);

            Router::new()
                .route("/health", get(handlers::health::health_check))
                .route("/api/v1/login", post(handlers::auth::login))
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
