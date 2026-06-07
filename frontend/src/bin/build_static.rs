use std::fs;
use std::path::Path;

#[path = "../views/mod.rs"]
mod views;

fn main() {
    let dist_dir = Path::new("dist");
    if !dist_dir.exists() {
        fs::create_dir(dist_dir).unwrap();
    }

    // index.html redirects to login
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
    fs::write(dist_dir.join("login.html"), views::login::LOGIN_HTML).unwrap();
    fs::write(
        dist_dir.join("forgot-password.html"),
        views::forgot_password::FORGOT_PASSWORD_HTML,
    )
    .unwrap();
    fs::write(
        dist_dir.join("reset-password.html"),
        views::reset_password::RESET_PASSWORD_HTML,
    )
    .unwrap();
    fs::write(
        dist_dir.join("dashboard.html"),
        views::dashboard::DASHBOARD_HTML,
    )
    .unwrap();
    fs::write(dist_dir.join("admin.html"), views::admin::ADMIN_HTML).unwrap();

    println!("Static files generated in dist/ directory.");
}
