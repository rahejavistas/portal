pub const FORGOT_PASSWORD_HTML: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Vistas Portal - Onboard or Reset</title>
    <link href="https://fonts.googleapis.com/css2?family=Outfit:wght@400;500;600;700;800&display=swap" rel="stylesheet">
    <style>
        * {
            box-sizing: border-box;
            font-family: 'Outfit', sans-serif;
        }
        :root {
            --background: #FFFFFF;
            --foreground: #111827;
            --primary: #3B82F6;
            --primary-hover: #2563EB;
            --secondary: #10B981;
            --accent: #F59E0B;
            --muted: #F3F4F6;
            --muted-foreground: #6B7280;
            --border: #E5E7EB;
            --radius-md: 6px;
            --radius-lg: 8px;
        }
        body {
            background-color: var(--muted);
            color: var(--foreground);
            margin: 0;
            display: flex;
            align-items: center;
            justify-content: center;
            min-height: 100vh;
            overflow: hidden;
            position: relative;
        }

        /* Decorative Background Shapes */
        .bg-shape-1 {
            position: absolute;
            width: 400px;
            height: 400px;
            border-radius: 50%;
            background: radial-gradient(circle, rgba(59, 130, 246, 0.08) 0%, rgba(59, 130, 246, 0) 70%);
            top: -100px;
            right: -100px;
            z-index: 1;
        }
        .bg-shape-2 {
            position: absolute;
            width: 500px;
            height: 500px;
            border-radius: 50%;
            background: radial-gradient(circle, rgba(16, 185, 129, 0.06) 0%, rgba(16, 185, 129, 0) 70%);
            bottom: -150px;
            left: -150px;
            z-index: 1;
        }

        .container {
            width: 100%;
            max-width: 480px;
            padding: 24px;
            position: relative;
            z-index: 10;
        }
        .display-header {
            text-align: center;
            margin-bottom: 24px;
        }
        .display-title {
            font-size: 3.5rem;
            font-weight: 800;
            letter-spacing: -0.03em;
            text-transform: uppercase;
            color: var(--primary);
            margin: 0 0 4px 0;
            line-height: 0.9;
        }
        .display-subtitle {
            font-size: 0.75rem;
            font-weight: 600;
            text-transform: uppercase;
            letter-spacing: 0.15em;
            color: var(--muted-foreground);
            margin: 0;
        }
        .card {
            background-color: var(--background);
            border-radius: var(--radius-lg);
            padding: 40px;
            box-shadow: none;
            border: none;
            transition: transform 0.2s ease;
        }
        .card:hover {
            transform: scale(1.01);
        }
        h2 {
            font-size: 1.75rem;
            margin: 0 0 8px 0;
            font-weight: 800;
            letter-spacing: -0.02em;
        }
        .subtitle {
            font-size: 0.875rem;
            color: var(--muted-foreground);
            margin-bottom: 32px;
            line-height: 1.5;
        }
        .form-group {
            margin-bottom: 24px;
        }
        label {
            display: block;
            margin-bottom: 8px;
            color: var(--foreground);
            font-size: 0.75rem;
            font-weight: 600;
            text-transform: uppercase;
            letter-spacing: 0.05em;
        }
        input {
            width: 100%;
            padding: 14px 16px;
            border-radius: var(--radius-md);
            border: 2px solid transparent;
            background-color: var(--muted);
            color: var(--foreground);
            font-size: 15px;
            transition: all 0.2s ease;
        }
        input::placeholder {
            color: var(--muted-foreground);
            opacity: 0.7;
        }
        input:focus {
            outline: none;
            background-color: var(--background);
            border-color: var(--primary);
        }
        button {
            width: 100%;
            padding: 15px;
            border-radius: var(--radius-md);
            border: none;
            background-color: var(--primary);
            color: var(--background);
            font-weight: 700;
            font-size: 0.875rem;
            text-transform: uppercase;
            letter-spacing: 0.1em;
            cursor: pointer;
            transition: all 0.2s ease;
            margin-top: 16px;
        }
        button:hover {
            background-color: var(--primary-hover);
            transform: scale(1.05);
        }
        button:focus-visible {
            outline: 2px solid var(--primary);
            outline-offset: 2px;
        }
        .success-box {
            display: none;
            background-color: #ECFDF5;
            border: 2px dashed var(--secondary);
            padding: 24px;
            border-radius: var(--radius-lg);
            margin-top: 24px;
        }
        .success-title {
            color: #065F46;
            font-weight: 800;
            font-size: 1.15rem;
            margin-bottom: 8px;
            letter-spacing: -0.01em;
        }
        .link-container {
            background-color: var(--background);
            padding: 12px 16px;
            border: 1px solid var(--border);
            border-radius: var(--radius-md);
            word-break: break-all;
            font-family: monospace;
            font-size: 13px;
            color: var(--primary);
            margin: 16px 0;
            font-weight: 600;
        }
        .actions-box {
            display: flex;
            justify-content: space-between;
            margin-top: 16px;
        }
        .actions-box a {
            color: var(--primary);
            text-decoration: none;
            font-weight: 700;
            font-size: 14px;
        }
        .actions-box a:hover {
            color: var(--primary-hover);
            text-decoration: underline;
        }
        .error-msg {
            color: #EF4444;
            font-size: 0.8125rem;
            margin-top: 24px;
            text-align: center;
            display: none;
            background-color: #FEE2E2;
            padding: 12px;
            border-radius: var(--radius-md);
            font-weight: 600;
        }
    </style>
</head>
<body>
    <div class="bg-shape-1"></div>
    <div class="bg-shape-2"></div>

    <div class="container">
        <div class="display-header">
            <h1 class="display-title">Vistas</h1>
            <div class="display-subtitle">Secure Access Portal</div>
        </div>

        <div class="card">
            <h2>Onboard / Reset</h2>
            <div class="subtitle">First-time resident? Enter your email to generate your secure password setup link. Existing users can also trigger a reset.</div>
            
            <form id="forgot-form">
                <div class="form-group">
                    <label for="email">Email Address</label>
                    <input type="email" id="email" required placeholder="resident@vistas.com">
                </div>
                <button type="submit">Generate Secure Link</button>
            </form>

            <div class="error-msg" id="error-msg"></div>

            <div class="success-box" id="success-box">
                <div class="success-title">✓ Link Generated</div>
                <div style="font-size: 14px; color: var(--muted-foreground); margin-bottom: 12px; line-height: 1.5">
                    Since the mail delivery runs in the background, we intercepted and printed the setup/reset link for your ease during development:
                </div>
                <div class="link-container" id="link-url"></div>
                <div class="actions-box">
                    <a id="direct-link" href="">Proceed to Setup Page →</a>
                </div>
            </div>

            <div class="actions-box" style="margin-top: 32px; border-top: 2px solid var(--muted); padding-top: 20px;">
                <a href="/login" style="color: var(--muted-foreground); font-weight: 500;">← Back to Sign In</a>
            </div>
        </div>
    </div>

    <script>
        const API_BASE = window.location.port === "4000" ? `http://${window.location.hostname}:3000` : "";

        const form = document.getElementById('forgot-form');
        const errorMsg = document.getElementById('error-msg');
        const successBox = document.getElementById('success-box');
        const linkUrl = document.getElementById('link-url');
        const directLink = document.getElementById('direct-link');

        form.addEventListener('submit', async (e) => {
            e.preventDefault();
            errorMsg.style.display = 'none';
            successBox.style.display = 'none';

            const email = document.getElementById('email').value.trim();

            try {
                const response = await fetch(`${API_BASE}/api/v1/forgot-password`, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ email })
                });

                const data = await response.json();

                if (response.ok && data.status === 'success') {
                    // Update host dynamically to match frontend port 4000 instead of backend API
                    let finalLink = data.reset_link.replace('3000', '4000').replace('localhost:4000', window.location.host);
                    linkUrl.textContent = finalLink;
                    directLink.href = finalLink;
                    successBox.style.display = 'block';
                } else {
                    errorMsg.textContent = data.message || 'An error occurred';
                    errorMsg.style.display = 'block';
                }
            } catch (err) {
                errorMsg.textContent = 'Failed to connect to backend server';
                errorMsg.style.display = 'block';
            }
        });
    </script>
</body>
</html>
"#;
