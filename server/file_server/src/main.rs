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
        accounting_enabled: config.accounting_enabled,
    };

    macro_rules! make_app {
        ($state:expr) => {{
            // Each admin route group requires a specific permission.
            use authentication::Permission;

            // Also reachable by roles-management/view permissions since the User Designations
            // subtab needs this same listing to know who to assign designations to.
            let users_view_routes = Router::new()
                .route("/users/list", post(handlers::admin::list_users))
                .route("/flats/list", post(handlers::flats::list_flats))
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
                .route("/flats/create", post(handlers::flats::create_flat))
                .route(
                    "/flats/bulk-create",
                    post(handlers::flats::bulk_create_flats),
                )
                .route(
                    "/flats/set-fire-flag",
                    post(handlers::flats::set_flat_fire_flag),
                )
                .route("/flats/delete", post(handlers::flats::delete_flat))
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
                .route("/process/add-note", post(handlers::process::admin_add_note))
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
                        // Needed by the template builder's designation picker.
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
                .route(
                    "/process-templates/deactivate",
                    post(handlers::process_templates::deactivate),
                )
                .route(
                    "/process-templates/holidays/list",
                    post(handlers::process_templates::list_holidays),
                )
                .route(
                    "/process-templates/holidays/add",
                    post(handlers::process_templates::add_holiday),
                )
                .route(
                    "/process-templates/holidays/remove",
                    post(handlers::process_templates::remove_holiday),
                )
                .route(
                    "/process-templates/tat-report",
                    post(handlers::process_templates::tat_report),
                )
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_any_of(&[Permission::ProcessTemplatesManage]),
                ));

            // No permission gate — each handler is self-scoped to the caller's own signer.
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

            // Gated on process.reviewer, same as attaching signers. No delete route —
            // a signer is only ever deactivated, never removed.
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

            let accounting_manage_routes = Router::new()
                .route(
                    "/accounting/billing-plans/create",
                    post(handlers::accounting::create_billing_plan),
                )
                .route(
                    "/accounting/billing-plans/assign",
                    post(handlers::accounting::assign_billing_plan),
                )
                .route(
                    "/accounting/billing-plans/assign-bulk",
                    post(handlers::accounting::bulk_assign_billing_plan),
                )
                .route(
                    "/accounting/invoices/generate",
                    post(handlers::accounting::generate_invoices),
                )
                .route(
                    "/accounting/invoices/:id/void",
                    post(handlers::accounting::void_invoice),
                )
                .route(
                    "/accounting/gst-rules/create",
                    post(handlers::accounting::create_gst_rule),
                )
                .route(
                    "/accounting/interest-rules/create",
                    post(handlers::accounting::create_interest_rule),
                )
                .route(
                    "/accounting/interest/run",
                    post(handlers::accounting::run_interest),
                )
                .route(
                    "/accounting/payments/record",
                    post(handlers::accounting::record_payment),
                )
                .route(
                    "/accounting/payment-instructions/set",
                    post(handlers::accounting::set_payment_instructions),
                )
                .route(
                    "/accounting/claims/list",
                    post(handlers::accounting::list_claims),
                )
                .route(
                    "/accounting/claims/:id/confirm",
                    post(handlers::accounting::confirm_claim),
                )
                .route(
                    "/accounting/claims/:id/reject",
                    post(handlers::accounting::reject_claim),
                )
                .route(
                    "/accounting/invoices/preview",
                    post(handlers::accounting::preview_bill_generation),
                )
                .route(
                    "/accounting/credit/apply",
                    post(handlers::accounting::apply_credit_to_invoice),
                )
                .route(
                    "/accounting/funds/contribution-rules/create",
                    post(handlers::accounting::create_fund_contribution_rule),
                )
                .route(
                    "/accounting/funds/assign-flat",
                    post(handlers::accounting::assign_flat_to_fund),
                )
                .route(
                    "/accounting/accounts/create",
                    post(handlers::accounting::create_account),
                )
                .route(
                    "/accounting/accounts/:id/set-active",
                    post(handlers::accounting::set_account_active),
                )
                .route(
                    "/accounting/vendors/create",
                    post(handlers::accounting::create_vendor),
                )
                .route(
                    "/accounting/purchase-orders/create",
                    post(handlers::accounting::create_purchase_order),
                )
                .route(
                    "/accounting/expenses/submit",
                    post(handlers::accounting::submit_expense),
                )
                .route(
                    "/accounting/expenses/:id/approve",
                    post(handlers::accounting::approve_expense),
                )
                .route(
                    "/accounting/expenses/:id/reject",
                    post(handlers::accounting::reject_expense),
                )
                .route(
                    "/accounting/tds-rules/create",
                    post(handlers::accounting::create_tds_rule),
                )
                .route(
                    "/accounting/bank-reconciliation/import",
                    post(handlers::accounting::import_statement),
                )
                .route(
                    "/accounting/bank-reconciliation/:id/auto-match",
                    post(handlers::accounting::auto_match),
                )
                .route(
                    "/accounting/bank-reconciliation/lines/:id/match-candidates",
                    post(handlers::accounting::match_candidates),
                )
                .route(
                    "/accounting/bank-reconciliation/lines/:id/match",
                    post(handlers::accounting::match_line),
                )
                .route(
                    "/accounting/bank-reconciliation/lines/:id/ignore",
                    post(handlers::accounting::ignore_line),
                )
                .route(
                    "/accounting/budgets/set",
                    post(handlers::accounting::set_budget),
                )
                .route(
                    "/accounting/fiscal-years/:id/close",
                    post(handlers::accounting::close_fiscal_year),
                )
                .route(
                    "/accounting/fiscal-years/:id/reopen",
                    post(handlers::accounting::reopen_fiscal_year),
                )
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_any_of(&[Permission::AccountingManage]),
                ))
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_accounting_enabled,
                ));

            // Reports and generated documents are read-only against the ledger, so
            // AccountingView holds these too, not just AccountingManage.
            let accounting_reports_routes = Router::new()
                .route(
                    "/accounting/documents/pdf",
                    post(handlers::accounting::admin_document_pdf),
                )
                .route(
                    "/accounting/reports/run",
                    post(handlers::accounting::run_report),
                )
                .route(
                    "/accounting/reports/collection-summary",
                    post(handlers::accounting::collection_summary),
                )
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_any_of(&[
                        Permission::AccountingManage,
                        Permission::AccountingView,
                    ]),
                ))
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_accounting_enabled,
                ));

            let accounting_view_routes = Router::new()
                .route(
                    "/accounting/billing-plans/list",
                    post(handlers::accounting::list_billing_plans),
                )
                .route(
                    "/accounting/billing-plans/assignments",
                    post(handlers::accounting::list_billing_assignments),
                )
                .route(
                    "/accounting/invoices/list",
                    post(handlers::accounting::list_invoices),
                )
                .route(
                    "/accounting/gst-rules/list",
                    post(handlers::accounting::list_gst_rules),
                )
                .route(
                    "/accounting/interest-rules/list",
                    post(handlers::accounting::list_interest_rules),
                )
                .route(
                    "/accounting/payments/list",
                    post(handlers::accounting::list_payments),
                )
                .route(
                    "/accounting/payment-instructions",
                    post(handlers::accounting::get_payment_instructions),
                )
                .route(
                    "/accounting/credit/balance",
                    post(handlers::accounting::get_flat_credit_balance),
                )
                .route(
                    "/accounting/funds/list",
                    post(handlers::accounting::list_funds),
                )
                .route(
                    "/accounting/funds/contribution-rules/for-flat",
                    post(handlers::accounting::list_fund_rules_for_flat),
                )
                .route(
                    "/accounting/fiscal-years/list",
                    post(handlers::accounting::list_fiscal_years),
                )
                .route(
                    "/accounting/settings/overview",
                    post(handlers::accounting::overview),
                )
                .route(
                    "/accounting/vendors/list",
                    post(handlers::accounting::list_vendors),
                )
                .route(
                    "/accounting/purchase-orders/list",
                    post(handlers::accounting::list_purchase_orders),
                )
                .route(
                    "/accounting/expenses/list",
                    post(handlers::accounting::list_expenses),
                )
                .route(
                    "/accounting/tds-rules/list",
                    post(handlers::accounting::list_tds_rules),
                )
                .route(
                    "/accounting/tds-summary",
                    post(handlers::accounting::tds_summary),
                )
                .route(
                    "/accounting/bank-reconciliation/lines/list",
                    post(handlers::accounting::list_lines),
                )
                .route(
                    "/accounting/budgets/variance",
                    post(handlers::accounting::variance_report),
                )
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_any_of(&[
                        Permission::AccountingManage,
                        Permission::AccountingView,
                    ]),
                ))
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_accounting_enabled,
                ));

            let accounting_accounts_read_routes = Router::new()
                .route(
                    "/accounting/accounts/list",
                    post(handlers::accounting::list_accounts),
                )
                .route(
                    "/accounting/flats/resolve",
                    post(handlers::accounting::resolve_flat),
                )
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_any_of(&[
                        Permission::AccountingManage,
                        Permission::AccountingView,
                    ]),
                ))
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::require_accounting_enabled,
                ));

            // No permission layer — each handler checks `user.is_admin()` (super_admin only).
            let society_profile_routes = Router::new()
                .route(
                    "/accounting/society-profile",
                    post(handlers::accounting::get_society_profile),
                )
                .route(
                    "/accounting/society-profile/set",
                    post(handlers::accounting::set_society_profile),
                );

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
                .merge(signer_directory_read_routes)
                .merge(accounting_manage_routes)
                .merge(accounting_view_routes)
                .merge(accounting_reports_routes)
                .merge(accounting_accounts_read_routes)
                .merge(society_profile_routes);

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
                    "/api/v1/support/ticket",
                    post(handlers::support::raise_ticket),
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
                    "/api/v1/process/audit-log-pdf",
                    post(handlers::process::audit_log_pdf),
                )
                .route(
                    "/api/v1/process/save-step",
                    post(handlers::process::save_process_step),
                )
                .route(
                    "/api/v1/process/download-url",
                    post(handlers::process::process_download_url),
                )
                .merge({
                    let accounting_resident_routes = Router::new()
                        .route(
                            "/api/v1/accounting/claims/submit",
                            post(handlers::accounting::submit_claim),
                        )
                        .route(
                            "/api/v1/accounting/claims/mine",
                            post(handlers::accounting::list_my_claims),
                        )
                        .route(
                            "/api/v1/accounting/invoices/mine",
                            post(handlers::accounting::list_my_invoices),
                        )
                        .route(
                            "/api/v1/accounting/documents/pdf",
                            post(handlers::accounting::member_document_pdf),
                        )
                        .route(
                            "/api/v1/accounting/payment-instructions",
                            post(handlers::accounting::get_payment_instructions),
                        )
                        .layer(axum_middleware::from_fn_with_state(
                            $state.clone(),
                            middleware::require_accounting_enabled,
                        ));
                    accounting_resident_routes
                })
                .nest("/api/v1/admin", admin_routes)
                // Standard CORS and custom security headers middleware
                .layer(axum_middleware::from_fn_with_state(
                    $state.clone(),
                    middleware::rate_limit_middleware,
                ))
                // Uploads go via presigned S3 PUTs; direct bodies are just small CSVs.
                .layer(DefaultBodyLimit::max(10 * 1024 * 1024))
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
