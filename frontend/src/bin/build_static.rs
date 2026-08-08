use std::fs;
use std::path::Path;

#[path = "../views/mod.rs"]
mod views;

fn process_html(html: &str, api_base: &str) -> String {
    let replacement = format!("const API_BASE = \"{}\";", api_base);
    html.replace(
        "const API_BASE = window.location.port === \"4000\" ? `http://${window.location.hostname}:3000` : \"\";",
        &replacement
    )
}

fn main() {
    let dist_dir = Path::new("dist");
    if !dist_dir.exists() {
        fs::create_dir(dist_dir).unwrap();
    }

    let api_base = std::env::var("API_BASE").unwrap_or_else(|_| "".to_string());
    println!("Building static files with API_BASE = '{}'", api_base);

    let index_html = r#"<!DOCTYPE html>
<html>
<head>
    <meta http-equiv="refresh" content="0; url=/login" />
</head>
<body>
    <p>Redirecting to <a href="/login">login</a>...</p>
</body>
</html>"#;

    fs::write(dist_dir.join("index.html"), index_html).unwrap();
    fs::write(
        dist_dir.join("login.html"),
        process_html(views::login::LOGIN_HTML, &api_base),
    )
    .unwrap();
    fs::write(
        dist_dir.join("forgot-password.html"),
        process_html(views::forgot_password::FORGOT_PASSWORD_HTML, &api_base),
    )
    .unwrap();
    fs::write(
        dist_dir.join("reset-password.html"),
        process_html(views::reset_password::RESET_PASSWORD_HTML, &api_base),
    )
    .unwrap();
    fs::write(
        dist_dir.join("dashboard.html"),
        process_html(views::dashboard::DASHBOARD_HTML, &api_base),
    )
    .unwrap();
    fs::write(
        dist_dir.join("admin.html"),
        process_html(views::admin::ADMIN_HTML, &api_base),
    )
    .unwrap();
    fs::write(
        dist_dir.join("process.html"),
        process_html(&views::process::process_html(), &api_base),
    )
    .unwrap();
    fs::write(
        dist_dir.join("admin-process.html"),
        process_html(&views::process::admin_process_html(), &api_base),
    )
    .unwrap();
    fs::write(
        dist_dir.join("admin-accounting.html"),
        process_html(&views::accounting::accounting_html(), &api_base),
    )
    .unwrap();

    println!("Static files generated in dist/ directory.");
}
