pub(crate) const DASHBOARD_HTML: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Vistas Portal - Resident Dashboard</title>
    <link href="https://fonts.googleapis.com/css2?family=Outfit:wght@400;500;600;700;800&display=swap" rel="stylesheet">
    <script>
        document.documentElement.setAttribute(
            'data-tab',
            new URLSearchParams(window.location.search).get('tab') || 'profile'
        );
    </script>
    <style>
        * {
            box-sizing: border-box;
            font-family: 'Outfit', sans-serif;
            margin: 0;
            padding: 0;
        }
        html {
            overflow-x: hidden;
        }
        .truncate {
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
            display: block;
            max-width: 100%;
        }
        .truncate-2-lines {
            display: -webkit-box;
            -webkit-line-clamp: 2;
            -webkit-box-orient: vertical;
            overflow: hidden;
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
            padding: 0;
            min-height: 100vh;
            overflow-x: hidden;
            width: 100%;
        }

        .layout-container {
            display: flex;
            width: 100%;
            min-height: 100vh;
        }

        .bg-shape-top-right {
            position: fixed;
            width: 500px;
            height: 500px;
            border-radius: 50%;
            background: radial-gradient(circle, rgba(59, 130, 246, 0.04) 0%, rgba(59, 130, 246, 0) 70%);
            top: -100px;
            right: -100px;
            z-index: 0;
            pointer-events: none;
        }

        .mobile-topbar {
            display: none;
            position: fixed;
            top: 0;
            left: 0;
            right: 0;
            height: 48px;
            background-color: var(--background);
            border-bottom: 1px solid var(--border);
            align-items: center;
            justify-content: space-between;
            padding: 0 14px;
            z-index: 200;
        }
        .mobile-topbar-brand {
            font-size: 1.5rem;
            font-weight: 800;
            letter-spacing: -0.03em;
            text-transform: uppercase;
            color: var(--primary);
        }
        .hamburger-btn {
            background: none;
            border: none;
            cursor: pointer;
            padding: 8px;
            display: flex;
            flex-direction: column;
            gap: 5px;
            border-radius: var(--radius-md);
            transition: background 0.2s;
        }
        .hamburger-btn:hover { background-color: var(--muted); }
        .hamburger-btn span {
            display: block;
            width: 22px;
            height: 2px;
            background-color: var(--foreground);
            border-radius: 2px;
            transition: all 0.3s ease;
        }

        .sidebar-overlay {
            display: none;
            position: fixed;
            inset: 0;
            width: 100vw;
            height: 100vh;
            background: rgba(0,0,0,0.45);
            z-index: 149;
            opacity: 0;
            transition: opacity 0.3s ease;
        }
        .sidebar-overlay.open {
            display: block;
            opacity: 1;
        }

        .sidebar {
            width: 260px;
            min-width: 260px;
            border-right: 1px solid var(--border);
            display: flex;
            flex-direction: column;
            padding: 40px 24px;
            justify-content: space-between;
            position: fixed;
            height: 100vh;
            left: 0;
            top: 0;
            z-index: 150;
            background-color: var(--background);
            transition: transform 0.3s cubic-bezier(0.4, 0, 0.2, 1);
            will-change: transform;
        }
        .sidebar-brand h1 {
            font-size: 2.25rem;
            font-weight: 800;
            letter-spacing: -0.03em;
            text-transform: uppercase;
            color: var(--primary);
            margin: 0 0 4px 0;
            line-height: 0.9;
        }
        .sidebar-brand p {
            font-size: 0.75rem;
            font-weight: 600;
            text-transform: uppercase;
            letter-spacing: 0.1em;
            color: var(--muted-foreground);
            margin: 0 0 40px 0;
        }
        .nav-menu {
            display: flex;
            flex-direction: column;
            gap: 10px;
            flex-grow: 1;
        }
        .nav-item {
            display: block;
            padding: 12px 16px;
            border: none;
            border-radius: var(--radius-md);
            color: var(--foreground);
            text-decoration: none;
            font-weight: 600;
            font-size: 0.875rem;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            cursor: pointer;
            transition: all 0.2s ease;
            text-align: left;
            background: none;
            width: 100%;
        }
        .nav-item:hover {
            background-color: var(--muted);
            transform: scale(1.03);
        }
        .nav-item.active {
            background-color: var(--primary);
            color: var(--background);
        }
        #panel-documents { display: none; }
        #panel-billing { display: none; }
        html[data-tab="documents"] #panel-profile { display: none; }
        html[data-tab="documents"] #panel-documents { display: block; }
        html[data-tab="documents"] #tab-btn-profile { background-color: transparent; color: var(--foreground); }
        html[data-tab="documents"] #tab-btn-documents { background-color: var(--primary); color: var(--background); }
        html[data-tab="billing"] #panel-profile { display: none; }
        html[data-tab="billing"] #panel-billing { display: block; }
        html[data-tab="billing"] #tab-btn-profile { background-color: transparent; color: var(--foreground); }
        html[data-tab="billing"] #tab-btn-billing { background-color: var(--primary); color: var(--background); }
        .badge { display: inline-block; padding: 3px 10px; border-radius: 9999px; font-size: 0.6875rem; font-weight: 700; text-transform: uppercase; letter-spacing: 0.04em; white-space: nowrap; }
        .badge-green { background-color: #D1FAE5; color: #065F46; }
        .badge-amber { background-color: #FEF3C7; color: #92400E; }
        .badge-red { background-color: #FEE2E2; color: #991B1B; }
        .badge-gray { background-color: var(--muted); color: var(--muted-foreground); }
        .secondary-btn {
            background-color: var(--muted);
            color: var(--foreground);
            border: none;
            padding: 10px 18px;
            border-radius: var(--radius-md);
            font-weight: 700;
            font-size: 0.75rem;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            cursor: pointer;
            transition: all 0.2s ease;
        }
        .secondary-btn:hover { background-color: var(--border); }
        .section-title { font-size: 1.125rem; font-weight: 800; margin: 32px 0 16px 0; }
        .section-title:first-child { margin-top: 0; }
        .empty-note { color: var(--muted-foreground); font-size: 0.875rem; padding: 20px 0; }
        .admin-btn {
            display: none;
            padding: 12px 16px;
            background-color: #FEF3C7;
            color: #D97706;
            border-radius: var(--radius-md);
            text-decoration: none;
            font-weight: 700;
            font-size: 0.8125rem;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            text-align: center;
            margin-bottom: 12px;
            transition: all 0.2s ease;
        }
        .admin-btn:hover {
            background-color: #FDE68A;
            transform: scale(1.05);
        }
        .logout-btn {
            background-color: var(--background);
            border: 2px solid var(--border);
            border-radius: var(--radius-md);
            color: var(--foreground);
            padding: 12px;
            cursor: pointer;
            font-weight: 700;
            font-size: 0.8125rem;
            text-transform: uppercase;
            width: 100%;
            text-align: center;
            transition: all 0.2s ease;
        }
        .logout-btn:hover {
            background-color: #FEE2E2;
            border-color: #FCA5A5;
            color: #EF4444;
            transform: scale(1.03);
        }

        .category-filter-bar {
            display: flex;
            gap: 10px;
            margin-bottom: 24px;
            overflow-x: auto;
            padding-bottom: 8px;
            -webkit-overflow-scrolling: touch;
        }
        .category-filter-bar::-webkit-scrollbar {
            display: none;
        }
        .filter-chip {
            flex-shrink: 0;
            background-color: var(--background);
            border: 2px solid var(--border);
            border-radius: var(--radius-md);
            color: var(--foreground);
            padding: 10px 20px;
            font-weight: 700;
            font-size: 0.8125rem;
            text-transform: uppercase;
            letter-spacing: 0.03em;
            cursor: pointer;
            transition: all 0.2s ease;
        }
        .filter-chip:hover {
            background-color: var(--muted);
            transform: translateY(-1px);
        }
        .filter-chip.active {
            background-color: var(--primary);
            color: var(--background);
            border-color: var(--primary);
        }
        .sub-filter-chip {
            flex-shrink: 0;
            background-color: var(--background);
            border: 1px solid var(--border);
            border-radius: var(--radius-md);
            color: var(--muted-foreground);
            padding: 6px 14px;
            font-weight: 600;
            font-size: 0.75rem;
            text-transform: uppercase;
            letter-spacing: 0.02em;
            cursor: pointer;
            transition: all 0.2s ease;
        }
        .sub-filter-chip:hover {
            background-color: var(--muted);
            color: var(--foreground);
        }
        .sub-filter-chip.active {
            background-color: var(--muted);
            color: var(--primary);
            border-color: var(--primary);
        }

        .main-content {
            flex-grow: 1;
            margin-left: 260px;
            padding: 60px 80px;
            max-width: 1200px;
            position: relative;
            z-index: 10;
        }

        @media (max-width: 768px) {
            .mobile-topbar { display: flex; }
            .sidebar {
                transform: translateX(-100%);
                width: min(280px, 85vw);
                min-width: unset;
                padding: 64px 20px 24px 20px;
                top: 0;
                box-shadow: 4px 0 24px rgba(0,0,0,0.12);
            }
            .sidebar.open { transform: translateX(0); }
            .sidebar-brand p { display: none; }
            .sidebar-brand h1 { display: none; }
            .main-content {
                margin-left: 0 !important;
                padding: 72px 16px 32px 16px;
                width: 100%;
                min-width: 0;
                max-width: 100vw;
                box-sizing: border-box;
            }
            .doc-table-desktop { display: none; }
            .doc-cards-mobile { display: flex !important; }
            h2 { font-size: 1.5rem; }
            .section-desc { font-size: 0.875rem; }
        }
        @media (min-width: 769px) {
            .doc-table-desktop { display: block; }
            .doc-cards-mobile { display: none !important; }
        }

        .doc-cards-mobile {
            display: flex;
            flex-direction: column;
            gap: 12px;
        }
        .doc-card-mobile {
            background: var(--background);
            border-radius: var(--radius-lg);
            padding: 16px;
            border: 1px solid var(--border);
            display: flex;
            flex-direction: column;
            gap: 10px;
        }
        .doc-card-mobile-header {
            display: flex;
            justify-content: space-between;
            align-items: flex-start;
            gap: 12px;
        }
        .doc-card-mobile-actions {
            display: flex;
            justify-content: flex-end;
        }

        h2 {
            font-size: 2.75rem;
            font-weight: 800;
            letter-spacing: -0.03em;
            color: var(--foreground);
            margin-top: 0;
            margin-bottom: 8px;
        }
        .section-desc {
            font-size: 1.125rem;
            color: var(--muted-foreground);
            margin: 0 0 40px 0;
        }

        .card {
            background-color: var(--background);
            border-radius: var(--radius-lg);
            padding: 40px;
            margin-bottom: 40px;
            box-shadow: none;
            border: none;
        }

        .form-group {
            margin-bottom: 20px;
            display: flex;
            flex-direction: column;
            gap: 6px;
        }
        .form-group label {
            font-size: 0.75rem;
            font-weight: 700;
            color: var(--muted-foreground);
            text-transform: uppercase;
        }
        .form-group input {
            padding: 10px;
            border: 1px solid var(--border);
            border-radius: var(--radius-md);
            font-size: 0.875rem;
        }
        .form-group input:disabled {
            background-color: var(--muted);
            color: var(--muted-foreground);
        }

        .feedback-box {
            display: none;
            padding: 12px;
            border-radius: var(--radius-md);
            margin-top: 20px;
            font-size: 13px;
            font-weight: 600;
        }
        .feedback-box.error {
            background-color: #FEE2E2;
            color: #EF4444;
        }
        .feedback-box.success {
            background-color: #ECFDF5;
            color: #059669;
        }

        .profile-table {
            width: 100%;
        }
        .profile-row {
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 20px 0;
            border-bottom: 1px solid var(--border);
        }
        .profile-row:last-child {
            border-bottom: none;
        }
        .profile-label {
            color: var(--muted-foreground);
            font-size: 0.75rem;
            text-transform: uppercase;
            font-weight: 700;
            letter-spacing: 0.05em;
        }
        .profile-value {
            font-weight: 700;
            font-size: 1.125rem;
        }

        .role-pill {
            display: inline-block;
            padding: 4px 10px;
            border-radius: 9999px;
            font-size: 0.6875rem;
            font-weight: 700;
            text-transform: uppercase;
            background-color: var(--muted);
            color: var(--muted-foreground);
            margin-right: 6px;
        }
        .role-pill.admin {
            background-color: #FEF3C7;
            color: #D97706;
        }
        .role-pill.committee {
            background-color: #E0F2FE;
            color: #0369A1;
        }

        .table-container {
            overflow-x: auto;
            border-radius: var(--radius-lg);
            border: none;
            background-color: var(--background);
        }
        table {
            width: 100%;
            border-collapse: collapse;
            text-align: left;
        }
        thead th {
            position: sticky;
            top: 0;
            z-index: 2;
        }
        th {
            background-color: #F8FAFC;
            border-bottom: 1px solid var(--border);
            padding: 16px 24px;
            color: var(--muted-foreground);
            font-size: 0.75rem;
            text-transform: uppercase;
            font-weight: 700;
            letter-spacing: 0.05em;
        }
        td {
            border-bottom: 1px solid var(--border);
            padding: 20px 24px;
            font-size: 0.9375rem;
            vertical-align: middle;
        }
        tr:last-child td {
            border-bottom: none;
        }
        tr:hover td {
            background-color: #FAFAFA;
        }
        .doc-title {
            font-weight: 700;
            font-size: 1.125rem;
            color: var(--primary);
            cursor: pointer;
            text-decoration: none;
            transition: color 0.15s ease;
        }
        .doc-title:hover {
            color: var(--primary-hover);
            text-decoration: underline;
        }
        .doc-desc {
            font-size: 0.875rem;
            color: var(--muted-foreground);
            margin-top: 4px;
            line-height: 1.45;
        }
        .doc-meta {
            font-size: 0.75rem;
            color: var(--muted-foreground);
            margin-top: 8px;
            font-weight: 500;
        }
        .doc-category-badge {
            font-size: 0.6875rem;
            text-transform: uppercase;
            padding: 4px 8px;
            border-radius: var(--radius-md);
            font-weight: 700;
            background-color: var(--muted);
            color: var(--muted-foreground);
            display: inline-block;
        }
        
        .cat-resident {
            background-color: #EFF6FF;
            color: #1D4ED8;
        }
        .cat-generic {
            background-color: #FEF3C7;
            color: #B45309;
        }
        .cat-minutes {
            background-color: #ECFDF5;
            color: #047857;
        }
        .cat-operational {
            background-color: #F5F3FF;
            color: #6D28D9;
        }

        .download-btn {
            background-color: var(--primary);
            color: var(--background);
            border: none;
            padding: 10px 18px;
            border-radius: var(--radius-md);
            font-weight: 700;
            font-size: 0.75rem;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            cursor: pointer;
            transition: all 0.2s ease;
        }
        .download-btn:hover {
            background-color: var(--primary-hover);
            transform: scale(1.05);
        }
    </style>
</head>
<body>
    <div class="bg-shape-top-right"></div>

    <div class="mobile-topbar" id="mobile-topbar">
        <span class="mobile-topbar-brand">Vistas</span>
        <button class="hamburger-btn" id="hamburger-btn" onclick="toggleSidebar()" aria-label="Open menu">
            <span></span><span></span><span></span>
        </button>
    </div>
    <div class="sidebar-overlay" id="sidebar-overlay" onclick="closeSidebar()"></div>

    <div class="layout-container">
        <aside class="sidebar" id="sidebar">
            <div class="sidebar-brand">
                <h1>Vistas</h1>
                <p>Resident Portal</p>
                
                <nav class="nav-menu">
                    <button class="nav-item active" id="tab-btn-profile" onclick="switchTab('profile'); closeSidebar()">Profile</button>
                    <button class="nav-item" id="tab-btn-documents" onclick="switchTab('documents'); closeSidebar()">Documents</button>
                    <button class="nav-item" id="tab-btn-billing" style="display:none;" onclick="switchTab('billing'); closeSidebar()">Billing</button>
                    <a href="/process" class="nav-item" id="tab-link-workflows" style="text-decoration: none;" onclick="closeSidebar()">Processes</a>
                    <a href="/admin" class="admin-btn" id="admin-link" style="display:none;">
                        Admin Dashboard &rarr;
                    </a>
                    <button class="logout-btn" id="logout-btn" style="margin-top: 8px;">Log Out</button>
                </nav>
            </div>
        </aside>



        <main class="main-content" id="main-content">
            <section id="panel-profile">
                <h2>Profile</h2>

                <div class="card">
                    <div class="profile-table">
                        <div class="profile-row">
                            <span class="profile-label">Email Address</span>
                            <span class="profile-value truncate" id="user-email" style="max-width:260px;">Loading...</span>
                        </div>
                        <div class="profile-row">
                            <span class="profile-label">Flat Number</span>
                            <span class="profile-value" id="user-flat">Loading...</span>
                        </div>
                        <div class="profile-row">
                            <span class="profile-label">Assigned Roles</span>
                            <span class="profile-value" id="user-roles">Loading...</span>
                        </div>
                    </div>
                </div>

                <div class="card" id="account-settings-card">
                    <h3 style="margin-top:0; font-size:18px; font-weight:800; margin-bottom:8px;">Security &amp; Notifications</h3>
                    <div style="display:flex; align-items:flex-start; gap:12px; padding:14px 0; border-bottom:1px solid var(--border);">
                        <input type="checkbox" id="mfa-toggle" onchange="toggleMfa()" style="margin-top:3px; accent-color:var(--primary); cursor:pointer; width:auto;">
                        <label for="mfa-toggle" style="cursor:pointer;">
                            <div style="font-weight:700; font-size:14px;">Sign-in OTP</div>
                            <div style="color:var(--muted-foreground); font-size:13px; margin-top:2px;">Require a one-time code by email to sign in.</div>
                        </label>
                    </div>
                    <div style="display:flex; align-items:flex-start; gap:12px; padding:14px 0; border-bottom:1px solid var(--border);">
                        <input type="checkbox" id="notify-process-toggle" onchange="saveNotificationSettings()" style="margin-top:3px; accent-color:var(--primary); cursor:pointer; width:auto;">
                        <label for="notify-process-toggle" style="cursor:pointer;">
                            <div style="font-weight:700; font-size:14px;">Process updates</div>
                            <div style="color:var(--muted-foreground); font-size:13px; margin-top:2px;">Email me about process updates.</div>
                        </label>
                    </div>
                    <div id="notify-signature-row" style="display:none; align-items:flex-start; gap:12px; padding:14px 0;">
                        <input type="checkbox" id="notify-signature-toggle" onchange="saveNotificationSettings()" style="margin-top:3px; accent-color:var(--primary); cursor:pointer; width:auto;">
                        <label for="notify-signature-toggle" style="cursor:pointer;">
                            <div style="font-weight:700; font-size:14px;">Signature requests</div>
                            <div style="color:var(--muted-foreground); font-size:13px; margin-top:2px;">Email me when it's my turn to sign, or a document I'm involved in is fully signed.</div>
                        </label>
                    </div>
                    <div class="feedback-box" id="account-settings-feedback" style="margin-top:12px; display:none;"></div>
                </div>

                <div class="card" id="signer-info-card" style="display:none;">
                    <h3 style="margin-top:0; font-size:18px; font-weight:800; margin-bottom:8px;">Signer Information</h3>
                    <p style="color:var(--muted-foreground); font-size:13px; margin-bottom:20px;">You've been granted signing access. Fill in your contact details so a process reviewer can attach you as a signer on documents.</p>
                    <div class="form-group">
                        <label for="signer-name">Full Name</label>
                        <input type="text" id="signer-name" placeholder="Your full name">
                    </div>
                    <div class="form-group">
                        <label for="signer-phone">Phone (digits only)</label>
                        <input type="text" id="signer-phone" placeholder="9876543210" inputmode="numeric">
                    </div>
                    <div class="form-group">
                        <label for="signer-email">Email</label>
                        <input type="email" id="signer-email" placeholder="you@example.com">
                    </div>
                    <button type="button" class="download-btn" onclick="saveSignerInfo()">Save</button>
                    <div class="feedback-box" id="signer-info-feedback" style="margin-top:12px; display:none;"></div>
                </div>
            </section>

            <section id="panel-documents">
                <div style="display: flex; justify-content: space-between; align-items: flex-start; flex-wrap: wrap; gap: 16px; margin-bottom: 16px;">
                    <div>
                        <h2 style="margin-top: 0;">Documents</h2>
                        <p class="section-desc" style="margin-bottom: 0;">Search, filter, and download stored building records.</p>
                    </div>
                    <button class="download-btn" onclick="openUploadModal()">Upload Document</button>
                </div>
                <div class="category-filter-bar" id="user-filter-bar">
                    <button class="filter-chip active" id="user-filter-all" onclick="filterDocs('ALL')">All</button>
                </div>
                <div class="category-filter-bar" id="user-subcategory-filter-bar" style="display:none;"></div>

                <div class="table-container doc-table-desktop">
                    <table>
                        <thead>
                            <tr>
                                <th>File Details</th>
                                <th>Document Category</th>
                                <th></th>
                            </tr>
                        </thead>
                        <tbody id="doc-rows-target">
                            <tr>
                                <td colspan="3" style="text-align: center; color: var(--muted-foreground); font-weight: 600;">Loading documents...</td>
                            </tr>
                        </tbody>
                    </table>
                </div>
                <div class="doc-cards-mobile" id="doc-cards-mobile-target" style="flex-direction: column; gap: 12px; display: none;">
                    <p style="text-align: center; color: var(--muted-foreground); font-weight: 600;">Loading documents...</p>
                </div>
            </section>

            <section id="panel-billing">
                <h2 style="margin-top: 0;">Billing</h2>
                <p class="section-desc" style="margin-bottom: 0;">Your maintenance bills and payment history for this flat.</p>

                <div class="section-title">Pending Bills</div>
                <div id="billing-pending-list"><p class="empty-note">Loading…</p></div>

                <div class="section-title">Paid Bills</div>
                <div id="billing-paid-list"><p class="empty-note">Loading…</p></div>

                <div class="section-title">Payments You've Reported</div>
                <div id="billing-claims-list"><p class="empty-note">Loading…</p></div>
            </section>
        </main>
        
        <div id="upload-modal" style="display:none; position:fixed; top:0; left:0; width:100vw; height:100vh; background:rgba(0,0,0,0.5); z-index:9999; justify-content:center; align-items:center; padding: 20px; box-sizing: border-box;">
            <div class="card" style="width: 100%; max-width: 500px; padding: 24px; position: relative;">
                <button onclick="closeUploadModal()" style="position: absolute; top: 16px; right: 16px; background: none; border: none; font-size: 24px; cursor: pointer; color: var(--muted-foreground);">&times;</button>
                <h3 style="margin-top: 0;">Upload Document</h3>
                <p style="font-size: 0.875rem; color: var(--muted-foreground); margin-bottom: 24px;">
                    Upload personal documents (max 1GB). They will be automatically linked to your flat and kept private.
                </p>
                <form id="user-upload-form" onsubmit="handleUserUpload(event)" style="display: flex; flex-direction: column; gap: 16px;">
                    <div>
                        <label style="display: block; font-size: 0.75rem; font-weight: 700; color: var(--muted-foreground); margin-bottom: 4px; text-transform: uppercase;">Title</label>
                        <input type="text" id="upload-title" required style="width: 100%; padding: 10px; border: 1px solid var(--border); border-radius: var(--radius-md);" placeholder="Document Title">
                    </div>
                    <div>
                        <label style="display: block; font-size: 0.75rem; font-weight: 700; color: var(--muted-foreground); margin-bottom: 4px; text-transform: uppercase;">Description (Optional)</label>
                        <input type="text" id="upload-desc" style="width: 100%; padding: 10px; border: 1px solid var(--border); border-radius: var(--radius-md);" placeholder="Brief description">
                    </div>
                    <div>
                        <label style="display: block; font-size: 0.75rem; font-weight: 700; color: var(--muted-foreground); margin-bottom: 4px; text-transform: uppercase;">File</label>
                        <input type="file" id="upload-file" required accept=".pdf,.doc,.docx,.xls,.xlsx,.txt,.jpg,.jpeg,.png" style="width: 100%; padding: 7px; border: 1px solid var(--border); border-radius: var(--radius-md); background: #fff;">
                    </div>
                    <div style="display: flex; flex-wrap: wrap; justify-content: flex-end; gap: 12px; margin-top: 8px;">
                        <button type="button" onclick="closeUploadModal()" class="filter-chip">Cancel</button>
                        <button type="submit" id="upload-submit-btn" class="download-btn" style="padding: 10px 24px;">Upload</button>
                    </div>
                </form>
                <div id="upload-status" style="margin-top: 12px; font-size: 0.875rem; font-weight: 600;"></div>
            </div>
        </div>

        <div id="alert-modal" style="display:none; position:fixed; top:0; left:0; width:100vw; height:100vh; background:rgba(0,0,0,0.5); z-index:10011; justify-content:center; align-items:center; padding: 20px; box-sizing: border-box;" onclick="if (event.target === this) closeAlertModal()">
            <div class="card" style="width: 100%; max-width: 400px; padding: 24px;">
                <p id="alert-modal-message" style="margin:0; color:var(--foreground); font-size:14px; line-height:1.5; white-space:pre-line;"></p>
                <div style="display:flex; justify-content:flex-end; margin-top:20px;">
                    <button type="button" id="alert-modal-ok-btn" class="download-btn" style="padding: 10px 24px;" onclick="closeAlertModal()">OK</button>
                </div>
            </div>
        </div>

    </div>

    <script>
        let _alertResolve = null;
        function showAlert(message) {
            document.getElementById('alert-modal-message').textContent = message;
            document.getElementById('alert-modal').style.display = 'flex';
            return new Promise(resolve => { _alertResolve = resolve; });
        }
        function closeAlertModal() {
            document.getElementById('alert-modal').style.display = 'none';
            if (_alertResolve) {
                const resolve = _alertResolve;
                _alertResolve = null;
                resolve();
            }
        }

        function toggleSidebar() {
            const sidebar = document.getElementById('sidebar');
            const overlay = document.getElementById('sidebar-overlay');
            if (sidebar.classList.contains('open')) {
                closeSidebar();
            } else {
                sidebar.classList.add('open');
                overlay.classList.add('open');
                document.body.style.overflow = 'hidden';
            }
        }
        function closeSidebar() {
            const sidebar = document.getElementById('sidebar');
            const overlay = document.getElementById('sidebar-overlay');
            sidebar.classList.remove('open');
            overlay.classList.remove('open');
            document.body.style.overflow = '';
        }

        function escapeHtml(unsafe) {
            if (!unsafe) return '';
            return unsafe
                 .toString()
                 .replace(/&/g, "&amp;")
                 .replace(/</g, "&lt;")
                 .replace(/>/g, "&gt;")
                 .replace(/"/g, "&quot;")
                 .replace(/'/g, "&#039;");
        }

        function truncText(text, maxLen) {
            const str = (text === null || text === undefined) ? '' : text.toString();
            return str.length > maxLen ? str.slice(0, maxLen - 1) + '…' : str;
        }
        function truncTitle(text) {
            return escapeHtml((text === null || text === undefined) ? '' : text.toString());
        }

        async function readErrorMessage(response) {
            const raw = await response.text();
            try {
                const parsed = JSON.parse(raw);
                if (typeof parsed === 'string') return parsed;
                if (parsed && typeof parsed.message === 'string') return parsed.message;
            } catch (e) {}
            return raw;
        }

        function isAdminUser(user) {
            return !!(user && (
                user.can_view_users || user.can_manage_users ||
                user.can_manage_documents || user.can_manage_roles ||
                user.can_manage_designations || user.can_manage_categories ||
                user.can_review_process || user.can_view_roles || user.can_view_all_processes
            ));
        }

        const API_BASE = window.location.port === "4000" ? `http://${window.location.hostname}:3000` : "";
        const token = localStorage.getItem('vfs_token');
        const userStr = localStorage.getItem('vfs_user');
        let currentUser = null;

        function renderProfile(user) {
            document.getElementById('user-email').textContent = user.email;
            document.getElementById('user-email').title = user.email;

            const rolesContainer = document.getElementById('user-roles');
            rolesContainer.innerHTML = '';

            const isAdmin = isAdminUser(user);

            if (user.designations && user.designations.length > 0) {
                user.designations.forEach(designation => {
                    const pill = document.createElement('span');
                    pill.className = 'role-pill ' + designation.toLowerCase();
                    pill.textContent = designation;
                    rolesContainer.appendChild(pill);
                });
            } else {
                rolesContainer.innerHTML = '<span style="color: var(--muted-foreground); font-size: 13px; font-weight: 600;">Standard Resident</span>';
            }

            const hasFlat = user.wing !== 'TBD' && user.flat_number !== 'TBD';

            if (!hasFlat && isAdmin) {
                window.location.replace('/admin');
                return;
            }

            if (!hasFlat) {
                document.getElementById('user-flat').innerHTML = '<span style="color: var(--foreground); font-style: italic;">Administrative Account</span>';
            } else {
                document.getElementById('user-flat').textContent = `${user.wing}-${user.flat_number}`;
            }

            document.getElementById('admin-link').style.display = isAdmin ? 'block' : 'none';
            document.getElementById('tab-btn-billing').style.display = (user.accounting_enabled && hasFlat) ? 'block' : 'none';

            loadAccountSettings();

            const isSignEligible = !!(user.designations && user.designations.some(d => d !== 'super_admin'));

            const signerCard = document.getElementById('signer-info-card');
            if (isSignEligible) {
                signerCard.style.display = 'block';
                loadSignerInfo();
            } else {
                signerCard.style.display = 'none';
            }

            document.getElementById('notify-signature-row').style.display = isSignEligible ? 'flex' : 'none';
        }

        async function loadAccountSettings() {
            try {
                const mfaResp = await fetch(`${API_BASE}/api/v1/profile/mfa`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}` }
                });
                if (mfaResp.ok) {
                    const mfa = await mfaResp.json();
                    document.getElementById('mfa-toggle').checked = !!mfa.mfa_enabled;
                }
            } catch (err) {}

            try {
                const notifResp = await fetch(`${API_BASE}/api/v1/profile/notifications`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}` }
                });
                if (notifResp.ok) {
                    const prefs = await notifResp.json();
                    document.getElementById('notify-process-toggle').checked = !!prefs.notify_process_updates;
                    document.getElementById('notify-signature-toggle').checked = !!prefs.notify_signature_events;
                }
            } catch (err) {}
        }

        async function toggleMfa() {
            const feedback = document.getElementById('account-settings-feedback');
            const enabled = document.getElementById('mfa-toggle').checked;
            feedback.style.display = 'none';
            try {
                const resp = await fetch(`${API_BASE}/api/v1/profile/mfa/toggle`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}`, 'Content-Type': 'application/json' },
                    body: JSON.stringify({ enabled })
                });
                feedback.style.display = 'block';
                if (resp.ok) {
                    feedback.className = 'feedback-box success';
                    feedback.textContent = enabled
                        ? 'Email login code enabled. You\'ll be asked for a code the next time you log in.'
                        : 'Email login code disabled.';
                } else {
                    document.getElementById('mfa-toggle').checked = !enabled;
                    feedback.className = 'feedback-box error';
                    feedback.textContent = 'Failed to update setting. Please try again.';
                }
            } catch (err) {
                document.getElementById('mfa-toggle').checked = !enabled;
                feedback.style.display = 'block';
                feedback.className = 'feedback-box error';
                feedback.textContent = 'Connection error. Please try again.';
            }
        }

        async function saveNotificationSettings() {
            const feedback = document.getElementById('account-settings-feedback');
            const notify_process_updates = document.getElementById('notify-process-toggle').checked;
            const notify_signature_events = document.getElementById('notify-signature-toggle').checked;
            feedback.style.display = 'none';
            try {
                const resp = await fetch(`${API_BASE}/api/v1/profile/notifications/toggle`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}`, 'Content-Type': 'application/json' },
                    body: JSON.stringify({ notify_process_updates, notify_signature_events })
                });
                feedback.style.display = 'block';
                if (resp.ok) {
                    feedback.className = 'feedback-box success';
                    feedback.textContent = 'Notification preferences saved.';
                } else {
                    feedback.className = 'feedback-box error';
                    feedback.textContent = 'Failed to update preferences. Please try again.';
                }
            } catch (err) {
                feedback.style.display = 'block';
                feedback.className = 'feedback-box error';
                feedback.textContent = 'Connection error. Please try again.';
            }
        }

        async function loadSignerInfo() {
            try {
                const resp = await fetch(`${API_BASE}/api/v1/profile/signer-info`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}` }
                });
                if (!resp.ok) return;
                const info = await resp.json();
                if (info) {
                    const nameInput = document.getElementById('signer-name');
                    nameInput.value = info.name || '';
                    nameInput.disabled = true;
                    nameInput.title = 'Contact support to change your registered name.';
                    document.getElementById('signer-phone').value = info.phone || '';
                    document.getElementById('signer-email').value = info.email || '';
                }
            } catch (err) {}
        }

        async function saveSignerInfo() {
            const feedback = document.getElementById('signer-info-feedback');
            const name = document.getElementById('signer-name').value.trim();
            const phone = document.getElementById('signer-phone').value.trim();
            const email = document.getElementById('signer-email').value.trim();

            feedback.style.display = 'none';
            try {
                const resp = await fetch(`${API_BASE}/api/v1/profile/signer-info/save`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}`, 'Content-Type': 'application/json' },
                    body: JSON.stringify({ name, phone, email })
                });
                feedback.style.display = 'block';
                if (resp.ok) {
                    feedback.className = 'feedback-box success';
                    feedback.textContent = 'Signer information saved.';
                } else {
                    feedback.className = 'feedback-box error';
                    const raw = await resp.text();
                    let msg = raw;
                    try {
                        const parsed = JSON.parse(raw);
                        msg = typeof parsed === 'string' ? parsed : (parsed.message || raw);
                    } catch (e) {}
                    feedback.textContent = msg;
                }
            } catch (err) {
                feedback.style.display = 'block';
                feedback.className = 'feedback-box error';
                feedback.textContent = 'Network error communicating with server.';
            }
        }

        if (!token || !userStr) {
            localStorage.clear();
            window.location.href = '/login?redirect=' + encodeURIComponent(window.location.pathname + window.location.search);
        } else {
            currentUser = JSON.parse(userStr);
            renderProfile(currentUser);

            async function refreshProfile() {
                try {
                    const response = await fetch(`${API_BASE}/api/v1/profile`, {
                        method: 'POST',
                        headers: {
                            'Authorization': `Bearer ${token}`
                        }
                    });
                    if (response.ok) {
                        const freshUser = await response.json();
                        localStorage.setItem('vfs_user', JSON.stringify(freshUser));
                        currentUser = freshUser;
                        renderProfile(freshUser);
                    } else if (response.status === 401) {
                        localStorage.clear();
                        window.location.href = '/login';
                    }
                } catch (err) {
                    console.error("Failed to refresh user profile:", err);
                }
            }
            refreshProfile();
        }

        document.getElementById('logout-btn').addEventListener('click', () => {
            localStorage.clear();
            window.location.href = '/login';
        });

        const urlParams = new URLSearchParams(window.location.search);
        const initialTab = urlParams.get('tab') || 'profile';

        function switchTab(tabName) {
            if (tabName === 'billing' && !(currentUser && currentUser.accounting_enabled)) {
                tabName = 'profile';
            }
            document.documentElement.setAttribute('data-tab', tabName);

            document.querySelectorAll('.nav-item').forEach(el => el.classList.remove('active'));
            document.getElementById(`tab-btn-${tabName}`).classList.add('active');
            
            document.getElementById('panel-profile').style.display = 'none';
            document.getElementById('panel-documents').style.display = 'none';
            document.getElementById('panel-billing').style.display = 'none';

            if (tabName === 'documents') {
                document.getElementById('panel-documents').style.display = 'block';
                loadResidentDocuments();
            } else if (tabName === 'billing') {
                document.getElementById('panel-billing').style.display = 'block';
                loadBillingTab();
            } else {
                document.getElementById('panel-profile').style.display = 'block';
            }
        }

        let allResidentDocs = [];
        let currentCategoryFilter = 'ALL';
        let currentSubcategoryFilter = 'ALL';

        async function loadResidentDocuments() {
            try {
                const response = await fetch(`${API_BASE}/api/v1/documents`, {
                    method: 'POST',
                    headers: {
                        'Authorization': `Bearer ${token}`
                    }
                });
                if (response.ok) {
                    allResidentDocs = await response.json();
                    renderCategoryFilterChips();
                    applyFiltersAndRender();
                } else {
                    document.getElementById('doc-rows-target').innerHTML = '<tr><td colspan="3" style="text-align: center; color: var(--foreground); font-weight: 600;">Failed to fetch documents.</td></tr>';
                }
            } catch (err) {
                document.getElementById('doc-rows-target').innerHTML = '<tr><td colspan="3" style="text-align: center; color: var(--foreground); font-weight: 600;">Connection error loading documents.</td></tr>';
            }
        }

        switchTab(initialTab);

        function money(v) {
            if (v === null || v === undefined) return '—';
            const n = typeof v === 'string' ? parseFloat(v) : v;
            if (Number.isNaN(n)) return '—';
            return '₹' + n.toLocaleString('en-IN', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
        }
        function fmtDate(v) {
            if (!v) return '—';
            const d = new Date(v);
            if (Number.isNaN(d.getTime())) return v;
            return d.toLocaleDateString('en-IN', { day: '2-digit', month: 'short', year: 'numeric' });
        }
        function paymentMethodLabelResident(m) {
            const map = { bank_transfer: 'Bank Transfer', upi: 'UPI', cheque: 'Cheque', cash: 'Cash' };
            return map[m] || m;
        }

        function invoiceCard(inv) {
            const outstanding = parseFloat(inv.total_amount) - parseFloat(inv.amount_paid);
            const statusMap = { PAID: 'badge-green', PARTIALLY_PAID: 'badge-amber', UNPAID: 'badge-red', VOID: 'badge-gray' };
            const cls = statusMap[inv.status] || 'badge-gray';
            const isPending = inv.status === 'UNPAID' || inv.status === 'PARTIALLY_PAID';
            return `<div class="card" style="padding:24px; margin-bottom:16px;">
                <div style="display:flex; justify-content:space-between; align-items:flex-start; flex-wrap:wrap; gap:12px;">
                    <div>
                        <div style="font-weight:800; font-size:1.0625rem;">${escapeHtml(inv.invoice_number)}</div>
                        <div style="color:var(--muted-foreground); font-size:0.8125rem; margin-top:4px;">Billing period ${fmtDate(inv.billing_period_start)} to ${fmtDate(inv.billing_period_end)} — due ${fmtDate(inv.due_date)}</div>
                    </div>
                    <span class="badge ${cls}">${(inv.status || '').replace(/_/g, ' ')}</span>
                </div>
                <div style="display:flex; justify-content:space-between; align-items:center; margin-top:16px; flex-wrap:wrap; gap:12px;">
                    <div>
                        <div style="font-size:1.375rem; font-weight:800;">${money(inv.total_amount)}</div>
                        ${isPending ? `<div style="color:var(--muted-foreground); font-size:0.8125rem;">${money(outstanding)} still due</div>` : ''}
                    </div>
                    <div style="display:flex; gap:10px; flex-wrap:wrap;">
                        <button class="secondary-btn" onclick="downloadInvoicePdf('${inv.invoice_id}', '${escapeHtml(inv.invoice_number)}')">Download PDF</button>
                        ${isPending ? `<button class="download-btn" onclick="toggleReportPayment('${inv.invoice_id}')">Report a Payment</button>` : ''}
                    </div>
                </div>
                ${isPending ? `<div id="report-payment-${inv.invoice_id}" style="display:none; margin-top:20px; padding-top:20px; border-top:1px solid var(--border);">
                    <div class="form-group"><label>Amount Paid</label><input type="number" step="0.01" id="claim-amount-${inv.invoice_id}" value="${outstanding.toFixed(2)}"></div>
                    <div class="form-group"><label>Payment Method</label><select id="claim-method-${inv.invoice_id}"><option value="upi">UPI</option><option value="bank_transfer">Bank Transfer</option><option value="cheque">Cheque</option><option value="cash">Cash</option></select></div>
                    <div class="form-group"><label>Reference Number</label><input id="claim-ref-${inv.invoice_id}" placeholder="UPI ref, cheque no., or bank UTR"></div>
                    <div class="form-group"><label>Note (optional)</label><input id="claim-note-${inv.invoice_id}" placeholder="Anything the admin should know"></div>
                    <div class="feedback-box" id="claim-feedback-${inv.invoice_id}"></div>
                    <button class="download-btn" onclick="submitPaymentReport('${inv.invoice_id}')">Submit</button>
                </div>` : ''}
            </div>`;
        }

        async function loadBillingTab() {
            document.getElementById('billing-pending-list').innerHTML = '<p class="empty-note">Loading…</p>';
            document.getElementById('billing-paid-list').innerHTML = '<p class="empty-note">Loading…</p>';
            try {
                const response = await fetch(`${API_BASE}/api/v1/accounting/invoices/mine`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}` }
                });
                if (!response.ok) throw new Error('Failed to load your bills');
                const invoices = await response.json();
                const pending = invoices.filter(i => i.status === 'UNPAID' || i.status === 'PARTIALLY_PAID');
                const paid = invoices.filter(i => i.status === 'PAID');
                document.getElementById('billing-pending-list').innerHTML = pending.length
                    ? pending.map(invoiceCard).join('')
                    : '<p class="empty-note">No pending bills — you\'re all caught up.</p>';
                document.getElementById('billing-paid-list').innerHTML = paid.length
                    ? paid.map(invoiceCard).join('')
                    : '<p class="empty-note">No paid bills yet.</p>';
            } catch (err) {
                document.getElementById('billing-pending-list').innerHTML = '<p class="empty-note">Could not load your bills.</p>';
                document.getElementById('billing-paid-list').innerHTML = '';
            }
            loadMyPaymentReports();
        }

        async function loadMyPaymentReports() {
            const el = document.getElementById('billing-claims-list');
            try {
                const response = await fetch(`${API_BASE}/api/v1/accounting/claims/mine`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}`, 'Content-Type': 'application/json' },
                    body: JSON.stringify({})
                });
                if (!response.ok) throw new Error('Failed to load your payment reports');
                const claims = await response.json();
                if (!claims.length) { el.innerHTML = '<p class="empty-note">You haven\'t reported any payments yet.</p>'; return; }
                const statusMap = { CONFIRMED: 'badge-green', PENDING: 'badge-amber', REJECTED: 'badge-red' };
                el.innerHTML = claims.map(c => `<div class="card" style="padding:20px; margin-bottom:12px;">
                    <div style="display:flex; justify-content:space-between; align-items:center; flex-wrap:wrap; gap:10px;">
                        <div>
                            <div style="font-weight:700;">${escapeHtml(c.invoice_number)} — ${money(c.amount)}</div>
                            <div style="color:var(--muted-foreground); font-size:0.8125rem; margin-top:4px;">${paymentMethodLabelResident(c.payment_method)} — reference ${escapeHtml(c.reference_number)} — reported ${fmtDate(c.submitted_at)}</div>
                            ${c.status === 'REJECTED' && c.review_note ? `<div style="color:#991B1B; font-size:0.8125rem; margin-top:4px;">Reason: ${escapeHtml(c.review_note)}</div>` : ''}
                        </div>
                        <span class="badge ${statusMap[c.status] || 'badge-gray'}">${c.status}</span>
                    </div>
                </div>`).join('');
            } catch (err) {
                el.innerHTML = '<p class="empty-note">Could not load your payment reports.</p>';
            }
        }

        function toggleReportPayment(invoiceId) {
            const el = document.getElementById(`report-payment-${invoiceId}`);
            el.style.display = el.style.display === 'none' ? 'block' : 'none';
        }

        async function submitPaymentReport(invoiceId) {
            const feedback = document.getElementById(`claim-feedback-${invoiceId}`);
            feedback.style.display = 'none';
            const amount = parseFloat(document.getElementById(`claim-amount-${invoiceId}`).value);
            const paymentMethod = document.getElementById(`claim-method-${invoiceId}`).value;
            const referenceNumber = document.getElementById(`claim-ref-${invoiceId}`).value.trim();
            const note = document.getElementById(`claim-note-${invoiceId}`).value.trim();
            if (!amount || amount <= 0) {
                feedback.style.display = 'block'; feedback.className = 'feedback-box error'; feedback.textContent = 'Enter the amount you paid.';
                return;
            }
            if (!referenceNumber) {
                feedback.style.display = 'block'; feedback.className = 'feedback-box error'; feedback.textContent = 'Enter your payment reference number.';
                return;
            }
            try {
                const response = await fetch(`${API_BASE}/api/v1/accounting/claims/submit`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}`, 'Content-Type': 'application/json' },
                    body: JSON.stringify({
                        invoice_id: invoiceId, amount, payment_method: paymentMethod,
                        reference_number: referenceNumber, note: note || null
                    })
                });
                if (!response.ok) {
                    const err = await response.json().catch(() => ({}));
                    throw new Error(err.message || 'Could not submit your payment report.');
                }
                feedback.style.display = 'block'; feedback.className = 'feedback-box success';
                feedback.textContent = 'Reported. An admin will review and confirm it shortly.';
                loadMyPaymentReports();
            } catch (err) {
                feedback.style.display = 'block'; feedback.className = 'feedback-box error'; feedback.textContent = err.message;
            }
        }

        async function downloadInvoicePdf(invoiceId, invoiceNumber) {
            try {
                const response = await fetch(`${API_BASE}/api/v1/accounting/documents/pdf`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}`, 'Content-Type': 'application/json' },
                    body: JSON.stringify({ kind: 'invoice', invoice_id: invoiceId })
                });
                if (!response.ok) throw new Error('Could not download this invoice.');
                const blob = await response.blob();
                const url = window.URL.createObjectURL(blob);
                const a = document.createElement('a');
                a.href = url; a.download = `${invoiceNumber}.pdf`;
                document.body.appendChild(a); a.click(); a.remove();
                window.URL.revokeObjectURL(url);
            } catch (err) {
                showAlert(err.message);
            }
        }

        function renderCategoryFilterChips() {
            const bar = document.getElementById('user-filter-bar');
            const categories = [...new Set(allResidentDocs.map(d => d.document.category))];

            bar.innerHTML = '<button class="filter-chip active" id="user-filter-all" onclick="filterDocs(\'ALL\')">All</button>' +
                categories.map(cat => {
                    const chipId = 'user-filter-' + cat.toLowerCase().replace(/_/g, '-');
                    return `<button class="filter-chip" id="${chipId}" onclick="filterDocs('${cat}')">${escapeHtml(formatCategory(cat))}</button>`;
                }).join('');

            if (currentCategoryFilter !== 'ALL' && !categories.includes(currentCategoryFilter)) {
                currentCategoryFilter = 'ALL';
            }
            renderSubcategoryFilterChips();
        }

        function renderSubcategoryFilterChips() {
            const bar = document.getElementById('user-subcategory-filter-bar');
            if (currentCategoryFilter === 'ALL') {
                bar.style.display = 'none';
                bar.innerHTML = '';
                return;
            }

            const docsInCategory = allResidentDocs.filter(d => d.document.category === currentCategoryFilter);
            const subcats = [...new Map(
                docsInCategory
                    .filter(d => d.metadata && d.metadata.subcategory_code)
                    .map(d => [d.metadata.subcategory_code, d.metadata.subcategory_label])
            ).entries()];

            if (subcats.length === 0) {
                bar.style.display = 'none';
                bar.innerHTML = '';
                currentSubcategoryFilter = 'ALL';
                return;
            }

            bar.style.display = 'flex';
            bar.innerHTML = '<button class="sub-filter-chip active" id="user-subfilter-all" onclick="filterBySubcategory(\'ALL\')">All</button>' +
                subcats.map(([code, label]) => {
                    const chipId = 'user-subfilter-' + code.toLowerCase().replace(/_/g, '-');
                    return `<button class="sub-filter-chip" id="${chipId}" onclick="filterBySubcategory('${code}')">${escapeHtml(label)}</button>`;
                }).join('');

            if (currentSubcategoryFilter !== 'ALL' && !subcats.some(([code]) => code === currentSubcategoryFilter)) {
                currentSubcategoryFilter = 'ALL';
            }
        }

        function filterDocs(category) {
            currentCategoryFilter = category;
            currentSubcategoryFilter = 'ALL';

            document.querySelectorAll('#user-filter-bar .filter-chip').forEach(el => el.classList.remove('active'));
            const chipId = 'user-filter-' + category.toLowerCase().replace(/_/g, '-');
            const target = document.getElementById(chipId);
            if (target) target.classList.add('active');

            renderSubcategoryFilterChips();
            applyFiltersAndRender();
        }

        function filterBySubcategory(code) {
            currentSubcategoryFilter = code;

            document.querySelectorAll('#user-subcategory-filter-bar .sub-filter-chip').forEach(el => el.classList.remove('active'));
            const chipId = 'user-subfilter-' + code.toLowerCase().replace(/_/g, '-');
            const target = document.getElementById(chipId);
            if (target) target.classList.add('active');

            applyFiltersAndRender();
        }

        function applyFiltersAndRender() {
            let filtered = allResidentDocs;
            if (currentCategoryFilter !== 'ALL') {
                filtered = filtered.filter(item => item.document.category === currentCategoryFilter);
            }
            if (currentSubcategoryFilter !== 'ALL') {
                filtered = filtered.filter(item => item.metadata && item.metadata.subcategory_code === currentSubcategoryFilter);
            }
            renderResidentDocsTable(filtered);
        }

        function formatCategory(cat) {
            switch(cat) {
                case 'RESIDENT_SPECIFIC': return 'My Flat Documents';
                case 'PUBLIC': return 'Public';
                default: return cat.toLowerCase().split('_').map(w => w.charAt(0).toUpperCase() + w.slice(1)).join(' ');
            }
        }

        function getCategoryBadgeClass(cat) {
            switch(cat) {
                case 'RESIDENT_SPECIFIC': return 'cat-resident';
                case 'PUBLIC': return 'cat-generic';
                default: return 'cat-operational';
            }
        }

        function renderResidentDocsTable(docsList) {
            const tbody = document.getElementById('doc-rows-target');
            const cardsContainer = document.getElementById('doc-cards-mobile-target');
            tbody.innerHTML = '';
            cardsContainer.innerHTML = '';
            
            if (docsList.length === 0) {
                tbody.innerHTML = '<tr><td colspan="3" style="text-align: center; color: var(--muted-foreground); padding: 40px; font-weight: 500;">No documents found matching the filter criteria.</td></tr>';
                cardsContainer.innerHTML = '<p style="text-align: center; color: var(--muted-foreground); padding: 24px; font-weight: 500;">No documents found.</p>';
                return;
            }

            docsList.forEach(docWithMeta => {
                const doc = docWithMeta.document;
                const meta = docWithMeta.metadata;

                const tr = document.createElement('tr');
                let metaHtml = '';
                if (meta) {
                    metaHtml = `
                        <div class="doc-meta">
                            ${meta.subcategory_label ? `Subcategory: <strong>${escapeHtml(meta.subcategory_label)}</strong>` : ''}
                            ${meta.tags && meta.tags.length > 0 ? ` &nbsp;|&nbsp; Tags: ${meta.tags.map(t => `#${escapeHtml(t)}`).join(', ')}` : ''}
                             &nbsp;|&nbsp; Date: ${escapeHtml(meta.document_date)}
                        </div>
                    `;
                }
                tr.innerHTML = `
                    <td style="max-width:420px;">
                        <div class="doc-title truncate" title="${truncTitle(doc.title)}" onclick="downloadDocument('${escapeHtml(doc.document_id)}')">${escapeHtml(truncText(doc.title, 70))}</div>
                        <div class="doc-desc truncate-2-lines" title="${truncTitle(doc.description)}">${escapeHtml(doc.description) || 'No description provided.'}</div>
                        ${metaHtml}
                    </td>
                    <td><span class="doc-category-badge ${getCategoryBadgeClass(doc.category)}">${formatCategory(doc.category)}</span></td>
                    <td></td>
                `;
                tbody.appendChild(tr);

                const card = document.createElement('div');
                card.className = 'doc-card-mobile';
                card.innerHTML = `
                    <div class="doc-card-mobile-header">
                        <div style="flex:1; min-width:0;">
                            <div class="doc-title truncate" style="font-size:1rem;" title="${truncTitle(doc.title)}" onclick="downloadDocument('${escapeHtml(doc.document_id)}')">${escapeHtml(truncText(doc.title, 50))}</div>
                            <div class="doc-desc truncate-2-lines" style="margin-top:4px;" title="${truncTitle(doc.description)}">${escapeHtml(doc.description) || 'No description.'}</div>
                        </div>
                        <span class="doc-category-badge ${getCategoryBadgeClass(doc.category)}" style="flex-shrink:0;">${formatCategory(doc.category)}</span>
                    </div>
                    ${meta ? `<div class="doc-meta" style="font-size:0.75rem;">${meta.document_date || ''}</div>` : ''}
                `;
                cardsContainer.appendChild(card);
            });
        }

        async function downloadDocument(docId) {
            const url = `${API_BASE}/api/v1/documents/${docId}`;
            try {
                const response = await fetch(url, {
                    method: 'POST',
                    headers: {
                        'Authorization': `Bearer ${token}`
                    }
                });
                if (response.ok) {
                    const data = await response.json();
                    if (data.download_url) {
                        window.open(data.download_url, '_blank');
                    } else {
                        showAlert('No download link returned by server.');
                    }
                } else {
                    const errText = await readErrorMessage(response);
                    showAlert(`Failed to get download link: ${errText}`);
                }
            } catch (err) {
                console.error(err);
                showAlert('Connection error while fetching download link.');
            }
        }

        async function handleUserUpload(e) {
            e.preventDefault();
            const title = document.getElementById('upload-title').value.trim();
            const fileInput = document.getElementById('upload-file');
            const statusEl = document.getElementById('upload-status');
            const submitBtn = document.getElementById('upload-submit-btn');

            if (fileInput.files.length === 0) return;
            const file = fileInput.files[0];

            if (file.size > 1024 * 1024 * 1024) {
                statusEl.style.color = '#EF4444';
                statusEl.textContent = 'Error: File size must be under 1GB.';
                return;
            }

            const allowedExts = ['pdf', 'doc', 'docx', 'xls', 'xlsx', 'txt', 'jpg', 'jpeg', 'png'];
            const ext = file.name.split('.').pop().toLowerCase();
            if (!allowedExts.includes(ext)) {
                statusEl.style.color = '#EF4444';
                statusEl.textContent = 'Error: Only PDF, Word, Excel, Text, and image (JPG/PNG) files are allowed.';
                return;
            }

            submitBtn.disabled = true;
            submitBtn.textContent = 'Requesting URL...';
            statusEl.textContent = '';

            try {
                const urlRes = await fetch(`${API_BASE}/api/v1/documents/upload-url`, {
                    method: 'POST',
                    headers: {
                        'Authorization': `Bearer ${token}`,
                        'Content-Type': 'application/json'
                    },
                    body: JSON.stringify({
                        title: title,
                        filename: file.name,
                        content_type: file.type || 'application/octet-stream'
                    })
                });

                if (!urlRes.ok) {
                    const errText = await readErrorMessage(urlRes);
                    statusEl.style.color = '#EF4444';
                    statusEl.textContent = `Failed: ${errText}`;
                    return;
                }

                const { document_id, upload_url } = await urlRes.json();

                submitBtn.textContent = 'Uploading...';
                const putRes = await fetch(upload_url, {
                    method: 'PUT',
                    headers: { 'Content-Type': file.type || 'application/octet-stream' },
                    body: file
                });

                if (!putRes.ok) {
                    statusEl.style.color = '#EF4444';
                    statusEl.textContent = 'Upload to storage failed. Please try again.';
                    return;
                }

                submitBtn.textContent = 'Confirming...';
                const confirmRes = await fetch(`${API_BASE}/api/v1/documents/confirm-upload`, {
                    method: 'POST',
                    headers: {
                        'Authorization': `Bearer ${token}`,
                        'Content-Type': 'application/json'
                    },
                    body: JSON.stringify({ document_id })
                });

                if (!confirmRes.ok) {
                    const errText = await readErrorMessage(confirmRes);
                    statusEl.style.color = '#EF4444';
                    statusEl.textContent = `Confirmation failed: ${errText}`;
                    return;
                }

                statusEl.style.color = '#10B981';
                statusEl.textContent = 'Document uploaded successfully!';
                
                loadResidentDocuments();
                
                setTimeout(() => {
                    closeUploadModal();
                }, 1000);
            } catch (err) {
                statusEl.style.color = '#EF4444';
                statusEl.textContent = 'Connection error during upload.';
            } finally {
                submitBtn.disabled = false;
                submitBtn.textContent = 'Upload';
            }
        }

        function openUploadModal() {
            document.getElementById('upload-modal').style.display = 'flex';
        }
        function closeUploadModal() {
            document.getElementById('upload-modal').style.display = 'none';
            document.getElementById('user-upload-form').reset();
            document.getElementById('upload-status').textContent = '';
        }
    </script>
</body>
</html>
"#;
