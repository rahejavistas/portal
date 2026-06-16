use axum::{
    response::{Html, IntoResponse, Redirect},
    routing::get,
    Router,
};
use std::net::SocketAddr;

mod views;

async fn redirect_to_login() -> impl IntoResponse {
    Redirect::to("/login")
}

async fn serve_login() -> impl IntoResponse {
    Html(views::login::LOGIN_HTML)
}

async fn serve_forgot_password() -> impl IntoResponse {
    Html(views::forgot_password::FORGOT_PASSWORD_HTML)
}

async fn serve_reset_password() -> impl IntoResponse {
    Html(views::reset_password::RESET_PASSWORD_HTML)
}

async fn serve_dashboard() -> impl IntoResponse {
    Html(views::dashboard::DASHBOARD_HTML)
}

async fn serve_admin() -> impl IntoResponse {
    Html(views::admin::ADMIN_HTML)
}

async fn serve_process() -> impl IntoResponse {
    Html(views::process::PROCESS_HTML)
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/", get(redirect_to_login))
        .route("/login", get(serve_login))
        .route("/forgot-password", get(serve_forgot_password))
        .route("/reset-password", get(serve_reset_password))
        .route("/dashboard", get(serve_dashboard))
        .route("/admin", get(serve_admin))
        .route("/process", get(serve_process));

    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "4000".to_string())
        .parse::<u16>()
        .unwrap_or(4000);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    println!("Frontend web server running on http://localhost:{}", port);

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}
