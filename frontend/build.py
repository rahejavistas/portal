import os
import re

# Read the production API base URL from the Cloudflare environment
API_BASE = os.environ.get("API_BASE", "")
print(f"Building frontend with API_BASE: '{API_BASE}'")

dist_dir = "dist"
os.makedirs(dist_dir, exist_ok=True)

def process_file(module_name, out_filename, replacements=None):
    filepath = f"src/views/{module_name}.rs"
    if not os.path.exists(filepath):
        print(f"Skipping {filepath}, not found.")
        return

    with open(filepath, "r") as f:
        content = f.read()
        # Extract the raw string between r#" and "#
        match = re.search(r'r#"\s*(.*?)\s*"#', content, re.DOTALL)
        if match:
            html = match.group(1)

            # Replace the local dev API_BASE logic with the hardcoded API_BASE URL
            # This matches `const API_BASE = ...;`
            html = re.sub(
                r'const API_BASE\s*=\s*.*?;',
                f'const API_BASE = "{API_BASE}";',
                html
            )

            # Per-page template substitutions (e.g. process.rs's __PROCESS_MODE__ flag,
            # shared by both /process and /admin-process — see process_html() in that file).
            for token, value in (replacements or {}).items():
                html = html.replace(token, value)

            with open(f"{dist_dir}/{out_filename}", "w") as out:
                out.write(html)
            print(f"Generated {out_filename}")

# Generate all pages
process_file("login", "login.html")
process_file("forgot_password", "forgot-password.html")
process_file("reset_password", "reset-password.html")
process_file("dashboard", "dashboard.html")
process_file("admin", "admin.html")
process_file("process", "process.html", {
    "__PROCESS_MODE__": "false",
})
process_file("process", "admin-process.html", {
    "__PROCESS_MODE__": "true",
    "<title>Vistas Portal - Processes</title>": "<title>Vistas Portal - Process Review</title>",
})

# Generate index.html redirect
with open(f"{dist_dir}/index.html", "w") as f:
    f.write('<!DOCTYPE html>\n<html><head><meta http-equiv="refresh" content="0; url=/login" /></head><body></body></html>')
print("Generated index.html")
