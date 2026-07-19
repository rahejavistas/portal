pub(crate) const ADMIN_HTML: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Vistas Portal - Admin Suite</title>
    <link href="https://fonts.googleapis.com/css2?family=Outfit:wght@400;500;600;700;800&display=swap" rel="stylesheet">
    <script>
        // Set the initial suite before the page paints, so the correct panel
        // is visible from the first frame instead of flashing Residents first.
        document.documentElement.setAttribute(
            'data-tab',
            new URLSearchParams(window.location.search).get('tab') || 'residents'
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
        /* Single-line truncation utility — pair with a title="" attribute (see truncTitle()
           in <script>) so the full text is still available on hover. */
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

        /* Layout Container */
        .layout-container {
            display: flex;
            width: 100%;
            min-height: 100vh;
        }

        /* Decorative Background Shapes — clipped to never overflow viewport */
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

        /* Left Main Sidebar */
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

        /* Mobile Top Bar */
        .mobile-topbar {
            display: none;
            position: fixed;
            top: 0; left: 0; right: 0;
            height: 56px;
            background-color: var(--background);
            border-bottom: 1px solid var(--border);
            align-items: center;
            justify-content: space-between;
            padding: 0 16px;
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
        .back-btn {
            display: block;
            padding: 12px 16px;
            border: 2px solid var(--border);
            border-radius: var(--radius-md);
            color: var(--foreground);
            text-decoration: none;
            font-weight: 700;
            font-size: 0.8125rem;
            text-transform: uppercase;
            text-align: center;
            transition: all 0.2s ease;
        }
        .back-btn:hover {
            background-color: var(--muted);
            transform: scale(1.03);
        }
        .admin-btn {
            display: block;
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

        /* Category filter bar and chips */
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

        /* Main Workspace Contents */
        .workspace {
            flex-grow: 1;
            margin-left: 260px;
            padding: 60px 80px;
            max-width: 1200px;
            position: relative;
            z-index: 10;
        }

        /* Typography */
        h2 {
            font-size: 2.75rem;
            font-weight: 800;
            letter-spacing: -0.03em;
            color: var(--foreground);
            margin-top: 0;
            margin-bottom: 8px;
        }
        .header-thick-rule {
            display: none; /* Replaced by color blocks and spaces */
        }

        /* Tabs Panels */
        .suite-panel {
            display: none;
        }
        .suite-panel.active {
            display: block;
        }
        /* Initial-tab state, set synchronously in <head> before first paint
           (see the inline script above) — avoids a flash to the Residents suite
           on a fresh page load with ?tab=documents. */
        html[data-tab="documents"] #suite-residents { display: none; }
        html[data-tab="documents"] #suite-documents { display: block; }
        html[data-tab="documents"] #suite-btn-residents { background-color: transparent; color: var(--foreground); }
        html[data-tab="documents"] #suite-btn-documents { background-color: var(--primary); color: var(--background); }

        /* Subtabs */
        .subtab-bar {
            display: flex;
            gap: 12px;
            border-bottom: 1px solid var(--border);
            padding-bottom: 12px;
            margin-bottom: 32px;
        }
        .subtab-btn {
            background-color: var(--background);
            border: 2px solid var(--border);
            border-radius: var(--radius-md);
            color: var(--foreground);
            padding: 8px 16px;
            font-weight: 700;
            font-size: 0.75rem;
            text-transform: uppercase;
            cursor: pointer;
            transition: all 0.2s ease;
        }
        .subtab-btn:hover {
            background-color: var(--muted);
            transform: scale(1.03);
        }
        .subtab-btn.active {
            background-color: var(--primary);
            color: var(--background);
            border-color: var(--primary);
        }
        .subtab-panel {
            display: none;
        }
        .subtab-panel.active {
            display: block;
        }

        /* Cards and Splits */
        .card {
            background-color: var(--background);
            border-radius: var(--radius-lg);
            padding: 40px;
            margin-bottom: 32px;
            box-shadow: none;
            border: none;
            height: fit-content;
            min-width: 0;
            overflow: hidden;
        }
        .split-grid {
            display: grid;
            grid-template-columns: 1fr 2fr;
            gap: 32px;
        }
        @media (max-width: 950px) {
            .split-grid {
                grid-template-columns: 1fr;
            }
        }

        /* Forms */
        .form-group {
            margin-bottom: 24px;
        }
        .form-row-2 {
            display: grid;
            grid-template-columns: 1fr 1fr;
            gap: 20px;
        }
        @media (max-width: 600px) {
            .form-row-2 {
                grid-template-columns: 1fr;
            }
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
        input, select, textarea {
            width: 100%;
            padding: 12px 14px;
            border-radius: var(--radius-md);
            border: 2px solid transparent;
            background-color: var(--muted);
            color: var(--foreground);
            font-size: 14px;
            transition: all 0.2s ease;
        }
        textarea {
            resize: vertical;
            height: 100px;
        }
        input:focus, select:focus, textarea:focus {
            outline: none;
            background-color: var(--background);
            border-color: var(--primary);
        }
        button.submit-btn {
            width: 100%;
            padding: 14px;
            border-radius: var(--radius-md);
            border: none;
            background-color: var(--primary);
            color: var(--background);
            font-weight: 700;
            font-size: 0.8125rem;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            cursor: pointer;
            transition: all 0.2s ease;
            margin-top: 12px;
        }
        button.submit-btn:hover {
            background-color: var(--primary-hover);
            transform: scale(1.05);
        }

        /* Feedback box */
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

        /* Permission info icon (click to open a description popup — works on touch, unlike hover tooltips) */
        .perm-info-btn {
            display: inline-flex;
            align-items: center;
            justify-content: center;
            width: 16px;
            height: 16px;
            margin-left: 6px;
            border-radius: 50%;
            border: 1px solid var(--muted-foreground);
            background: transparent;
            color: var(--muted-foreground);
            font-size: 10px;
            font-style: italic;
            font-weight: 700;
            line-height: 1;
            cursor: pointer;
            padding: 0;
        }
        .perm-info-btn:hover, .perm-info-btn:focus {
            border-color: var(--primary);
            color: var(--primary);
        }

        /* Table */
        .table-container {
            overflow-x: auto;
            border-radius: var(--radius-lg);
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
            padding: 14px;
            color: var(--muted-foreground);
            font-size: 0.75rem;
            text-transform: uppercase;
            font-weight: 700;
            letter-spacing: 0.05em;
        }
        td {
            border-bottom: 1px solid var(--border);
            padding: 14px;
            font-size: 13px;
            vertical-align: middle;
        }
        tr:last-child td {
            border-bottom: none;
        }
        tr:hover td {
            background-color: #FAFAFA;
        }

        /* Badges */
        .badge {
            display: inline-block;
            padding: 2px 8px;
            border-radius: var(--radius-md);
            font-size: 0.6875rem;
            font-weight: 700;
            text-transform: uppercase;
            margin-right: 4px;
            background-color: var(--muted);
            color: var(--muted-foreground);
        }
        .badge.admin {
            background-color: #FEF3C7;
            color: #D97706;
        }
        .badge.committee {
            background-color: #E0F2FE;
            color: #0369A1;
        }
        .badge.deleted {
            background-color: #FEE2E2;
            color: #EF4444;
        }

        /* Category tag (matches the resident document list's badge styling) */
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

        /* Action elements */
        .action-select {
            width: 120px;
            padding: 6px 8px;
            font-size: 12px;
            background-color: var(--muted);
            border: 1px solid transparent;
            color: var(--foreground);
            cursor: pointer;
            border-radius: var(--radius-md);
        }
        .action-select:focus {
            outline: none;
            background-color: var(--background);
            border-color: var(--primary);
        }
        .del-action-btn {
            background-color: #FEE2E2;
            border: none;
            color: #EF4444;
            padding: 6px 12px;
            border-radius: var(--radius-md);
            font-size: 12px;
            font-weight: 700;
            text-transform: uppercase;
            cursor: pointer;
            transition: all 0.2s ease;
        }
        .del-action-btn:hover {
            background-color: #EF4444;
            color: var(--background);
            transform: scale(1.05);
        }

        @media (max-width: 768px) {
            .mobile-topbar { display: flex; }
            .sidebar {
                transform: translateX(-100%);
                width: min(280px, 85vw);
                min-width: unset;
                padding: 64px 20px 24px 20px;
                box-shadow: 4px 0 24px rgba(0,0,0,0.12);
            }
            .sidebar.open { transform: translateX(0); }
            .sidebar-brand p { display: none; }
            .workspace {
                margin-left: 0 !important;
                padding: 72px 16px 32px 16px;
                width: 100%;
                min-width: 0;
                max-width: 100vw;
                overflow-x: hidden;
            }
            /* Reduce card padding so inner content fits */
            .card { padding: 20px; margin-bottom: 20px; }
            /* Collapse split grid immediately */
            .split-grid { grid-template-columns: 1fr; gap: 0; }
            /* Subtabs wrap instead of overflowing */
            .subtab-bar { flex-wrap: wrap; gap: 8px; }
            /* Category filter bar must not widen the page */
            .category-filter-bar {
                max-width: 100%;
                overflow-x: auto;
                -webkit-overflow-scrolling: touch;
            }
            /* Table: horizontal scroll only, vertical handled by page */
            table {
                min-width: 480px;
            }
            /* Hide least critical table columns on very small screens */
            .admin-col-hide-mobile { display: none; }
            h2 { font-size: 1.75rem; }
        }
    </style>
</head>
<body>
    <div class="bg-shape-top-right"></div>

    <!-- Mobile Top Bar -->
    <div class="mobile-topbar" id="mobile-topbar">
        <span class="mobile-topbar-brand">Admin</span>
        <button class="hamburger-btn" id="hamburger-btn" onclick="toggleSidebar()" aria-label="Open menu">
            <span></span><span></span><span></span>
        </button>
    </div>
    <!-- Sidebar Overlay -->
    <div class="sidebar-overlay" id="sidebar-overlay" onclick="closeSidebar()"></div>

    <div class="layout-container">
        <!-- Sidebar Navigation -->
        <aside class="sidebar" id="sidebar">
            <div class="sidebar-brand">
                <h1>Admin</h1>
                <p>Raheja Vistas Admin Room</p>
                
                <nav class="nav-menu">
                    <button class="nav-item" id="suite-btn-residents" style="display:none;" onclick="switchSuite('residents'); closeSidebar()">
                        Residents
                    </button>
                    <button class="nav-item" id="suite-btn-documents" style="display:none;" onclick="switchSuite('documents'); closeSidebar()">
                        Documents
                    </button>
                    <button class="nav-item" id="suite-btn-roles" style="display:none;" onclick="switchSuite('roles'); closeSidebar()">
                        Roles
                    </button>
                    <a href="/process" class="nav-item" id="suite-btn-workflows" style="text-decoration: none;" onclick="closeSidebar()">
                        Processes
                    </a>
                    <a href="/dashboard" class="admin-btn" id="my-flat-dashboard-link" style="display:none;">
                        My Flat Dashboard &rarr;
                    </a>
                    <button class="logout-btn" id="logout-btn">Log Out</button>
                </nav>
            </div>
        </aside>



        <!-- Main Workspace Contents -->
        <main class="workspace" id="main-workspace">
            
            <!-- RESIDENTS SUITE PANEL -->
            <section class="suite-panel active" id="suite-residents">
                <h2>Residents</h2>
                
                <div class="split-grid" id="residents-split-grid">
                    <!-- Left: Add Form & Upload Form (requires users.manage) -->
                    <div id="residents-left-column" style="display: flex; flex-direction: column; gap: 32px;">
                        <div class="card">
                            <h3 style="margin-top:0; font-size:20px; font-weight:800; margin-bottom: 24px;">Add Resident</h3>
                            <form id="onboard-form">
                                <div class="form-group">
                                    <label for="email">Email</label>
                                    <input type="email" id="email" required placeholder="e.g. resident@vistas.com">
                                </div>
                                <div class="form-group">
                                    <label for="wing">Wing</label>
                                    <input type="text" id="wing" required placeholder="e.g. A" maxlength="2">
                                </div>
                                <div class="form-group">
                                    <label for="flat">Flat</label>
                                    <input type="text" id="flat" required placeholder="e.g. 101">
                                </div>
                                <button type="submit" class="submit-btn">Add Resident</button>
                            </form>

                            <div class="feedback-box" id="feedback"></div>
                        </div>

                        <div class="card">
                            <h3 style="margin-top:0; font-size:20px; font-weight:800; margin-bottom: 24px;">Bulk Upload CSV</h3>
                            <form id="upload-csv-form">
                                <div class="form-group">
                                    <label for="csv-file">CSV File (Email, Wing, Flat)</label>
                                    <input type="file" id="csv-file" accept=".csv" required style="border: 2px dashed var(--border); padding: 16px; border-radius: 8px; cursor: pointer;">
                                </div>
                                <button type="submit" class="submit-btn">Upload CSV</button>
                            </form>
                            <div class="feedback-box" id="upload-feedback"></div>
                            <div id="upload-results" style="display: none; margin-top: 16px;">
                                <div style="display: flex; justify-content: space-between; margin-bottom: 8px; font-weight: 600;">
                                    <span style="color: var(--success);">Success: <span id="upload-success-count">0</span></span>
                                    <span style="color: var(--destructive);">Failed: <span id="upload-error-count">0</span></span>
                                </div>
                                <table style="font-size: 14px;">
                                    <thead>
                                        <tr>
                                            <th style="padding: 8px;">Email</th>
                                            <th style="padding: 8px;">Error</th>
                                        </tr>
                                    </thead>
                                    <tbody id="upload-errors-target">
                                    </tbody>
                                </table>
                            </div>
                        </div>
                    </div>

                    <!-- Right: Resident list -->
                    <div class="card">
                        <h3 style="margin-top:0; font-size:20px; font-weight:800; margin-bottom: 24px;">Registry</h3>
                        <div style="margin-bottom: 24px;">
                            <input type="text" id="search-input" placeholder="Search User...">
                        </div>
                        <div class="table-container">
                            <table>
                                <tbody id="user-rows-target">
                                    <tr>
                                        <td colspan="2" style="text-align: center; color: var(--muted-foreground); font-weight: 600;">Loading registry...</td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>
                    </div>
                </div>
            </section>

            <!-- DOCUMENTS SUITE PANEL -->
            <section class="suite-panel" id="suite-documents">
                <h2>Documents</h2>
                
                <!-- Subtabs Menu -->
                <div class="subtab-bar">
                    <button class="subtab-btn active" id="subtab-btn-db" onclick="switchSubtab('db')">Explorer</button>
                    <button class="subtab-btn" id="subtab-btn-register" onclick="switchSubtab('register')">Add Document</button>
                    <button class="subtab-btn" id="subtab-btn-categories" style="display:none;" onclick="switchSubtab('categories')">Categories</button>
                </div>

                <!-- Subtab 1: Document Database Explorer -->
                <div class="subtab-panel active" id="subtab-panel-db">
                    
                    <!-- Category Filter Bar (Desktop & Mobile) -->
                    <div class="category-filter-bar" id="admin-filter-bar">
                        <button class="filter-chip active" id="admin-filter-all" onclick="filterAdminDocsBySubbar('ALL')">All</button>
                        <button class="filter-chip" id="admin-filter-deleted" onclick="filterAdminDocsBySubbar('DELETED')">Deleted Files</button>
                    </div>
                    <div class="category-filter-bar" id="admin-subcategory-filter-bar" style="display:none;"></div>

                    <div class="card">
                        <h3 style="margin-top:0; font-size:20px; font-weight:800; margin-bottom: 24px;">Explorer</h3>

                        <div class="table-container">
                            <table>
                                <tbody id="admin-doc-rows-target">
                                    <tr>
                                        <td colspan="3" style="text-align: center; color: var(--muted-foreground); font-weight: 600;">Loading documents...</td>
                                    </tr>
                                </tbody>
                            </table>
                        </div>
                    </div>
                </div>

                <!-- Subtab 2: Register Document Form -->
                <div class="subtab-panel" id="subtab-panel-register">
                    <div class="card" style="width: 100%; max-width: 700px;">
                        <h3 style="margin-top:0; font-size:20px; font-weight:800; margin-bottom: 24px;">Add Document</h3>
                        <form id="register-doc-form">
                            <div class="form-group">
                                <label for="doc-title">Title</label>
                                <input type="text" id="doc-title" required placeholder="e.g. General Body Meeting Minutes May 2026">
                            </div>
                            <div class="form-group">
                                <label for="doc-desc">Description</label>
                                <textarea id="doc-desc" placeholder="e.g. Summary of structural paint vendors..."></textarea>
                            </div>
                            <div class="form-group">
                                <label for="doc-file">Document File</label>
                                <input type="file" id="doc-file" accept=".pdf,.doc,.docx,.xls,.xlsx,.txt,.jpg,.jpeg,.png" required style="padding: 8px; width: 100%; background-color: var(--muted);">
                            </div>
                            <div class="form-group">
                                <label for="doc-category">Document Category</label>
                                <select id="doc-category" required onchange="handleCategoryChange()">
                                    <option value="">Loading categories...</option>
                                </select>
                            </div>

                            <!-- Resident Flat (shown only for RESIDENT_SPECIFIC) -->
                            <div id="flat-mapping-section" style="display: none;">
                                <h4 style="margin: 25px 0 15px 0; color: var(--foreground); font-size:14px; text-transform: uppercase; padding-bottom: 8px; font-weight: 800; border-bottom: 1px solid var(--border);">Flat Visibility Mapping</h4>
                                <div class="form-row-2">
                                    <div class="form-group">
                                        <label for="doc-flat-wing">Wing</label>
                                        <input type="text" id="doc-flat-wing" placeholder="e.g. A" maxlength="2" oninput="scheduleFlatOwnerLookup()">
                                    </div>
                                    <div class="form-group">
                                        <label for="doc-flat-number">Flat Number</label>
                                        <input type="text" id="doc-flat-number" placeholder="e.g. 301" oninput="scheduleFlatOwnerLookup()">
                                    </div>
                                </div>
                                <div id="flat-owner-preview" style="font-size:12.5px; color:var(--muted-foreground); margin-top:-8px; margin-bottom:15px; min-height:16px;"></div>
                            </div>

                            <!-- Optional Details fields -->
                            <h4 style="margin: 25px 0 15px 0; color: var(--foreground); font-size:14px; text-transform: uppercase; padding-bottom: 8px; font-weight: 800; border-bottom: 1px solid var(--border);">Other Info (Optional)</h4>
                            
                            <div class="form-row-2">
                                <div class="form-group">
                                    <label for="doc-subcategory">Subcategory</label>
                                    <select id="doc-subcategory">
                                        <option value="">None</option>
                                    </select>
                                </div>
                                <div class="form-group">
                                    <label for="doc-tags">Tags (comma-separated)</label>
                                    <input type="text" id="doc-tags" placeholder="e.g. flat_301b, maintenance, paint, rules">
                                </div>
                            </div>

                            <div class="form-group">
                                <label for="doc-date">Date</label>
                                <input type="date" id="doc-date">
                            </div>

                            <div class="form-row-2">
                                <div class="form-group">
                                    <label for="doc-valid-from">Valid From</label>
                                    <input type="date" id="doc-valid-from">
                                </div>
                                <div class="form-group">
                                    <label for="doc-valid-until">Valid Until</label>
                                    <input type="date" id="doc-valid-until">
                                </div>
                            </div>

                            <button type="submit" class="submit-btn">Add Document</button>
                        </form>
                        <div class="feedback-box" id="doc-feedback"></div>
                    </div>
                </div>

                <!-- Subtab 3: Document Categories (create + designation access matrix — requires documents.categories.manage) -->
                <div class="subtab-panel" id="subtab-panel-categories">
                    <div class="card" id="category-access-card" style="display:none; margin-bottom: 24px;">
                        <h3 style="margin-top:0; font-size:20px; font-weight:800; margin-bottom: 24px;">Document Category Access</h3>
                        <p style="color:var(--muted-foreground); font-size:14px; margin-bottom:20px;">Choose which designations can view or manage each document category.</p>
                        <div class="table-container">
                            <table>
                                <tbody id="category-access-designations-rows-target">
                                    <tr><td colspan="2" style="text-align: center; color: var(--muted-foreground); font-weight: 600;">Loading...</td></tr>
                                </tbody>
                            </table>
                        </div>
                    </div>

                    <div class="card" style="margin-bottom: 24px;">
                        <h3 style="margin-top:0; font-size:20px; font-weight:800; margin-bottom: 24px;">All Categories</h3>
                        <div class="table-container">
                            <table>
                                <tbody id="categories-list-rows-target">
                                    <tr><td colspan="2" style="text-align: center; color: var(--muted-foreground); font-weight: 600;">Loading...</td></tr>
                                </tbody>
                                <tfoot>
                                    <tr>
                                        <td><input type="text" id="new-category-label" placeholder="e.g. Maintenance" style="margin:0;"></td>
                                        <td><button type="button" id="create-category-submit-btn" class="submit-btn" style="width:auto; padding:8px 16px; margin:0;" onclick="submitCreateCategory()">Add New</button></td>
                                    </tr>
                                </tfoot>
                            </table>
                        </div>
                        <div class="feedback-box" id="create-category-feedback" style="margin-top:15px;"></div>
                    </div>

                    <div class="card" style="margin-bottom: 24px;">
                        <h3 style="margin-top:0; font-size:20px; font-weight:800; margin-bottom: 24px;">Subcategories</h3>
                        <p style="color:var(--muted-foreground); font-size:14px; margin-bottom:20px;">Add labels under a category to help residents filter documents faster.</p>
                        <div class="form-group" style="max-width:320px;">
                            <label for="subcategory-parent-select">Category</label>
                            <select id="subcategory-parent-select" onchange="renderSubcategoriesList()">
                                <option value="">Loading categories...</option>
                            </select>
                        </div>
                        <div class="table-container">
                            <table>
                                <tbody id="subcategories-list-rows-target">
                                    <tr><td colspan="2" style="text-align: center; color: var(--muted-foreground); font-weight: 600;">Loading...</td></tr>
                                </tbody>
                                <tfoot>
                                    <tr>
                                        <td><input type="text" id="new-subcategory-label" placeholder="e.g. Audit" style="margin:0;"></td>
                                        <td><button type="button" id="create-subcategory-submit-btn" class="submit-btn" style="width:auto; padding:8px 16px; margin:0;" onclick="submitCreateSubcategory()">Add New</button></td>
                                    </tr>
                                </tfoot>
                            </table>
                        </div>
                        <div class="feedback-box" id="create-subcategory-feedback" style="margin-top:15px;"></div>
                    </div>
                </div>
            </section>

            <!-- ROLES SUITE PANEL (visible only if the caller can manage roles and/or designations) -->
            <section class="suite-panel" id="suite-roles">
                <h2>Roles</h2>

                <!-- Subtabs Menu -->
                <div class="subtab-bar">
                    <button class="subtab-btn active" id="subtab-btn-user-designations" style="display:none;" onclick="switchSubtab('user-designations')">User Designations</button>
                    <button class="subtab-btn" id="subtab-btn-manage-designations" style="display:none;" onclick="switchSubtab('manage-designations')">Manage Designations</button>
                </div>

                <!-- Subtab 1: User Designations (assign/revoke — requires users.roles.manage) -->
                <div class="subtab-panel active" id="subtab-panel-user-designations">
                    <div class="card">
                        <h3 style="margin-top:0; font-size:20px; font-weight:800; margin-bottom: 24px;">User Designations</h3>
                        <p style="color:var(--muted-foreground); font-size:14px; margin-bottom:20px;">Assign or revoke designations for any resident.</p>
                        <div style="margin-bottom: 24px; display:flex; gap:12px; align-items:center;">
                            <input type="text" id="roles-search-input" placeholder="Search User..." style="flex:1;">
                            <button type="button" id="add-staff-btn" style="display:none; width:auto; flex-shrink:0; padding:10px 18px; margin:0;" class="submit-btn" onclick="openAddStaffModal()">Add Staff</button>
                        </div>
                        <div class="table-container">
                            <table>
                                <tbody id="roles-user-rows-target">
                                    <tr><td colspan="3" style="text-align: center; color: var(--muted-foreground); font-weight: 600;">Loading...</td></tr>
                                </tbody>
                            </table>
                        </div>
                    </div>
                </div>

                <!-- Subtab 2: Manage Designations (create + their permissions/document access — requires designations.manage) -->
                <div class="subtab-panel" id="subtab-panel-manage-designations">
                    <div class="card" style="margin-bottom: 24px;">
                        <h3 style="margin-top:0; font-size:20px; font-weight:800; margin-bottom: 24px;">All Designations</h3>
                        <div class="table-container">
                            <table>
                                <tbody id="designations-list-rows-target">
                                    <tr><td colspan="2" style="text-align: center; color: var(--muted-foreground); font-weight: 600;">Loading...</td></tr>
                                </tbody>
                                <tfoot>
                                    <tr>
                                        <td><input type="text" id="new-designation-label" placeholder="e.g. Maintenance Lead" style="margin:0;"></td>
                                        <td><button type="button" class="submit-btn" style="width:auto; padding:8px 16px; margin:0;" onclick="submitCreateDesignation()">Add New</button></td>
                                    </tr>
                                </tfoot>
                            </table>
                        </div>
                        <div class="feedback-box" id="create-designation-feedback" style="margin-top:15px;"></div>
                    </div>

                    <div class="card" style="margin-bottom: 24px;">
                        <h3 style="margin-top:0; font-size:20px; font-weight:800; margin-bottom: 24px;">Designation Permissions</h3>
                        <p style="color:var(--muted-foreground); font-size:14px; margin-bottom:20px;">Controls what each designation can do.</p>
                        <div class="table-container">
                            <table id="designation-permissions-table">
                                <thead>
                                    <tr id="designation-permissions-header">
                                        <th>Designation</th>
                                    </tr>
                                </thead>
                                <tbody id="designation-permissions-rows-target">
                                    <tr><td style="text-align: center; color: var(--muted-foreground); font-weight: 600;">Loading...</td></tr>
                                </tbody>
                            </table>
                        </div>
                        <div style="display:flex; justify-content:flex-end; margin-top:16px;">
                            <button id="save-designation-permissions-btn" class="submit-btn" style="width:auto; padding:10px 20px;" onclick="saveDesignationPermissions()">Save Changes</button>
                        </div>
                        <div class="feedback-box" id="designation-permissions-feedback" style="margin-top: 15px; display:none;"></div>
                    </div>
                </div>
            </section>
        </main>
    </div>

    <!-- Revoke Flat Modal -->
    <div id="revoke-flat-modal" style="display:none; position:fixed; top:0; left:0; width:100vw; height:100vh; background:rgba(0,0,0,0.5); z-index:9999; justify-content:center; align-items:center;">
        <div style="background:var(--background); padding:24px; border-radius:12px; width:min(400px, 92vw); box-sizing:border-box; border:1px solid var(--border); box-shadow: 0 10px 30px rgba(0,0,0,0.5);">
            <h3 style="margin-top:0; color:var(--foreground); font-size:18px;">Revoke Resident Flat</h3>
            <p style="color:var(--muted-foreground); font-size:14px; margin-bottom:20px; line-height:1.5;">Select the flat mapping you want to revoke for this user.</p>
            
            <div class="form-group">
                <label for="revoke-flat-select">Select Flat</label>
                <select id="revoke-flat-select" style="width:100%; padding:10px; background:var(--input-bg); border:1px solid var(--border); color:var(--foreground); border-radius:6px; margin-bottom:24px;"></select>
            </div>
            
            <input type="hidden" id="revoke-flat-user-id">
            <div class="feedback-box" id="revoke-feedback" style="margin-bottom: 15px;"></div>
            
            <div style="display:flex; flex-wrap:wrap; justify-content:flex-end; gap:12px;">
                <button onclick="closeRevokeFlatModal()" style="padding:10px 16px; background:transparent; border:1px solid var(--border); color:var(--foreground); border-radius:6px; cursor:pointer;">Cancel</button>
                <button onclick="submitRevokeFlat()" class="del-action-btn" style="padding:10px 16px;">Revoke Flat</button>
            </div>
        </div>
    </div>

    <!-- Manage Designations Modal -->
    <div id="manage-designations-modal" style="display:none; position:fixed; top:0; left:0; width:100vw; height:100vh; background:rgba(0,0,0,0.5); z-index:9999; justify-content:center; align-items:center;">
        <div style="background:var(--background); padding:28px; border-radius:12px; width:min(440px, 92vw); box-sizing:border-box; border:1px solid var(--border); box-shadow: 0 10px 30px rgba(0,0,0,0.5);">
            <h3 id="manage-designations-title" style="margin-top:0; margin-bottom:20px; color:var(--foreground); font-size:18px; word-break:break-word;">Manage Designations</h3>

            <div class="form-group">
                <div id="manage-designations-checklist" style="display:flex; flex-direction:column; gap:4px; max-height:280px; overflow-y:auto; padding:8px; border:1px solid var(--border); border-radius:8px; box-sizing:border-box;"></div>
            </div>

            <input type="hidden" id="manage-designations-user-id">
            <div class="feedback-box" id="manage-designations-feedback" style="margin-top: 15px;"></div>

            <div style="display:flex; flex-wrap:wrap; justify-content:flex-end; gap:8px; margin-top:20px;">
                <button onclick="closeManageDesignationsModal()" style="box-sizing:border-box; height:40px; padding:0 16px; font-size:14px; line-height:1; background:transparent; border:1px solid var(--border); color:var(--foreground); border-radius:6px; cursor:pointer;">Close</button>
                <button id="manage-designations-save-btn" onclick="saveManageDesignations()" style="box-sizing:border-box; height:40px; padding:0 16px; font-size:14px; line-height:1; background:var(--primary); border:1px solid var(--primary); color:var(--background); border-radius:6px; cursor:pointer; font-weight:700;">Save Changes</button>
            </div>
        </div>
    </div>

    <!-- Manage Category Access Modal -->
    <div id="manage-category-access-modal" style="display:none; position:fixed; top:0; left:0; width:100vw; height:100vh; background:rgba(0,0,0,0.5); z-index:9999; justify-content:center; align-items:center;">
        <div style="background:var(--background); padding:28px; border-radius:12px; width:min(480px, 92vw); box-sizing:border-box; border:1px solid var(--border); box-shadow: 0 10px 30px rgba(0,0,0,0.5);">
            <h3 id="manage-category-access-title" style="margin-top:0; margin-bottom:8px; color:var(--foreground); font-size:18px; word-break:break-word;">Category Access</h3>
            <p style="color:var(--muted-foreground); font-size:13px; margin-bottom:16px;">Read lets this designation view documents in a category. Manage adds upload, sign, and delete.</p>

            <div id="manage-category-access-warning" style="display:none; background:#FFFBEB; color:#B45309; border:1px solid #FDE68A; border-radius:8px; padding:10px 12px; font-size:13px; line-height:1.5; margin-bottom:16px;"></div>

            <div style="display:flex; font-size:11px; font-weight:700; text-transform:uppercase; color:var(--muted-foreground); padding:0 8px; margin-bottom:6px;">
                <div style="flex:1;">Document Category</div>
                <div style="width:60px; text-align:center;">Read</div>
                <div style="width:70px; text-align:center;">Manage</div>
            </div>
            <div id="manage-category-access-checklist" style="display:flex; flex-direction:column; gap:4px; max-height:320px; overflow-y:auto; padding:8px; border:1px solid var(--border); border-radius:8px; box-sizing:border-box;"></div>

            <input type="hidden" id="manage-category-access-designation-code">
            <div class="feedback-box" id="manage-category-access-feedback" style="margin-top: 15px;"></div>

            <div style="display:flex; flex-wrap:wrap; justify-content:flex-end; gap:8px; margin-top:20px;">
                <button onclick="closeManageCategoryAccessModal()" style="box-sizing:border-box; height:40px; padding:0 16px; font-size:14px; line-height:1; background:transparent; border:1px solid var(--border); color:var(--foreground); border-radius:6px; cursor:pointer;">Close</button>
                <button id="manage-category-access-save-btn" onclick="saveManageCategoryAccess()" style="box-sizing:border-box; height:40px; padding:0 16px; font-size:14px; line-height:1; background:var(--primary); border:1px solid var(--primary); color:var(--background); border-radius:6px; cursor:pointer; font-weight:700;">Save Changes</button>
            </div>
        </div>
    </div>

    <!-- Permission Info Popup -->
    <div id="perm-info-modal" style="display:none; position:fixed; top:0; left:0; width:100vw; height:100vh; background:rgba(0,0,0,0.5); z-index:9999; justify-content:center; align-items:center;" onclick="if (event.target === this) closePermInfoModal()">
        <div style="background:var(--background); padding:24px; border-radius:12px; width:min(360px, 92vw); box-sizing:border-box; border:1px solid var(--border); box-shadow: 0 10px 30px rgba(0,0,0,0.5);">
            <h3 id="perm-info-title" style="margin-top:0; margin-bottom:10px; color:var(--foreground); font-size:16px;"></h3>
            <p id="perm-info-desc" style="color:var(--muted-foreground); font-size:14px; line-height:1.5; margin-bottom:20px;"></p>
            <div style="display:flex; justify-content:flex-end;">
                <button onclick="closePermInfoModal()" style="box-sizing:border-box; height:38px; padding:0 16px; font-size:14px; line-height:1; background:var(--primary); border:1px solid var(--primary); color:var(--background); border-radius:6px; cursor:pointer; font-weight:700;">Got it</button>
            </div>
        </div>
    </div>

    <!-- Edit Modal (shared by designations + document categories/subcategories: rename + delete) -->
    <div id="rename-modal" style="display:none; position:fixed; top:0; left:0; width:100vw; height:100vh; background:rgba(0,0,0,0.5); z-index:9999; justify-content:center; align-items:center;" onclick="if (event.target === this) closeRenameModal()">
        <div style="background:var(--background); padding:24px; border-radius:12px; width:min(400px, 92vw); box-sizing:border-box; border:1px solid var(--border); box-shadow: 0 10px 30px rgba(0,0,0,0.5);">
            <h3 id="rename-modal-title" style="margin-top:0; margin-bottom:16px; color:var(--foreground); font-size:18px;">Edit</h3>
            <input type="hidden" id="rename-modal-kind">
            <input type="hidden" id="rename-modal-code">
            <div class="form-group">
                <label for="rename-modal-input">Label</label>
                <input type="text" id="rename-modal-input" onkeydown="if (event.key === 'Enter') submitRenameModal()">
            </div>
            <div class="feedback-box" id="rename-modal-feedback" style="margin-top: 15px;"></div>
            <div style="display:flex; flex-wrap:wrap; justify-content:space-between; align-items:center; gap:8px; margin-top:20px;">
                <button id="rename-modal-delete-btn" onclick="deleteFromRenameModal()" class="del-action-btn" style="display:none;">Delete</button>
                <div style="display:flex; flex-wrap:wrap; gap:8px; margin-left:auto;">
                    <button onclick="closeRenameModal()" style="box-sizing:border-box; height:40px; padding:0 16px; font-size:14px; line-height:1; background:transparent; border:1px solid var(--border); color:var(--foreground); border-radius:6px; cursor:pointer;">Cancel</button>
                    <button id="rename-modal-save-btn" onclick="submitRenameModal()" style="box-sizing:border-box; height:40px; padding:0 16px; font-size:14px; line-height:1; background:var(--primary); border:1px solid var(--primary); color:var(--background); border-radius:6px; cursor:pointer; font-weight:700;">Save</button>
                </div>
            </div>
        </div>
    </div>

    <!-- Confirm Modal (replaces native confirm() — Chrome silently swallows those unless
         DevTools is open, which reads as "the button just doesn't work") -->
    <div id="confirm-modal" style="display:none; position:fixed; top:0; left:0; width:100vw; height:100vh; background:rgba(0,0,0,0.5); z-index:10000; justify-content:center; align-items:center;" onclick="if (event.target === this) closeConfirmModal(false)">
        <div style="background:var(--background); padding:24px; border-radius:12px; width:min(400px, 92vw); box-sizing:border-box; border:1px solid var(--border); box-shadow: 0 10px 30px rgba(0,0,0,0.5);">
            <p id="confirm-modal-message" style="margin:0; color:var(--foreground); font-size:14px; line-height:1.5;"></p>
            <div style="display:flex; flex-wrap:wrap; justify-content:flex-end; gap:8px; margin-top:20px;">
                <button onclick="closeConfirmModal(false)" style="box-sizing:border-box; height:40px; padding:0 16px; font-size:14px; line-height:1; background:transparent; border:1px solid var(--border); color:var(--foreground); border-radius:6px; cursor:pointer;">Cancel</button>
                <button id="confirm-modal-ok-btn" onclick="closeConfirmModal(true)" class="del-action-btn">Confirm</button>
            </div>
        </div>
    </div>

    <!-- Add Staff Modal -->
    <div id="add-staff-modal" style="display:none; position:fixed; top:0; left:0; width:100vw; height:100vh; background:rgba(0,0,0,0.5); z-index:10000; justify-content:center; align-items:center;" onclick="if (event.target === this) closeAddStaffModal()">
        <div style="background:var(--background); padding:24px; border-radius:12px; width:min(400px, 92vw); box-sizing:border-box; border:1px solid var(--border); box-shadow: 0 10px 30px rgba(0,0,0,0.5);">
            <h3 style="margin-top:0; margin-bottom:16px; color:var(--foreground); font-size:18px;">Add Staff</h3>
            <p style="color:var(--muted-foreground); font-size:13px; margin-top:0; margin-bottom:16px;">Creates an account with no flat, assigned the Staff designation.</p>
            <div class="form-group">
                <label for="add-staff-email">Email</label>
                <input type="email" id="add-staff-email" placeholder="e.g. staff@vistas.com">
            </div>
            <div class="feedback-box" id="add-staff-feedback" style="margin-top:12px;"></div>
            <div style="display:flex; flex-wrap:wrap; justify-content:flex-end; gap:8px; margin-top:20px;">
                <button onclick="closeAddStaffModal()" style="box-sizing:border-box; height:40px; padding:0 16px; font-size:14px; line-height:1; background:transparent; border:1px solid var(--border); color:var(--foreground); border-radius:6px; cursor:pointer;">Cancel</button>
                <button type="button" id="add-staff-submit-btn" class="submit-btn" style="width:auto; margin:0;" onclick="submitAddStaff()">Create</button>
            </div>
        </div>
    </div>

    <!-- Edit Document Modal -->
    <div id="edit-document-modal" style="display:none; position:fixed; top:0; left:0; width:100vw; height:100vh; background:rgba(0,0,0,0.5); z-index:9999; justify-content:center; align-items:center; overflow-y:auto;" onclick="if (event.target === this) closeEditDocumentModal()">
        <div style="background:var(--background); padding:24px; border-radius:12px; width:min(480px, 92vw); box-sizing:border-box; border:1px solid var(--border); box-shadow: 0 10px 30px rgba(0,0,0,0.5); margin:32px 0; max-height:calc(100vh - 64px); overflow-y:auto;">
            <h3 style="margin-top:0; margin-bottom:16px; color:var(--foreground); font-size:18px;">Edit Document</h3>
            <input type="hidden" id="edit-doc-id">
            <input type="hidden" id="edit-doc-original-category">

            <div class="form-group">
                <label for="edit-doc-title">Title</label>
                <input type="text" id="edit-doc-title">
            </div>
            <div class="form-group">
                <label for="edit-doc-desc">Description</label>
                <textarea id="edit-doc-desc"></textarea>
            </div>
            <div class="form-group">
                <label for="edit-doc-category">Category</label>
                <select id="edit-doc-category" onchange="handleEditCategoryChange()"></select>
            </div>
            <div id="edit-doc-category-warning" style="display:none; margin:0 0 15px 0; padding:10px 12px; background:rgba(234,179,8,0.12); border:1px solid rgba(234,179,8,0.4); border-radius:6px; font-size:12.5px; color:var(--foreground); line-height:1.5;">
                Changing the category may change who can view and access this document.
            </div>
            <div class="form-group">
                <label for="edit-doc-subcategory">Subcategory</label>
                <select id="edit-doc-subcategory">
                    <option value="">None</option>
                </select>
            </div>
            <div class="form-row-2">
                <div class="form-group">
                    <label for="edit-doc-tags">Tags (comma-separated)</label>
                    <input type="text" id="edit-doc-tags">
                </div>
                <div class="form-group">
                    <label for="edit-doc-date">Date</label>
                    <input type="date" id="edit-doc-date">
                </div>
            </div>
            <div class="form-row-2">
                <div class="form-group">
                    <label for="edit-doc-valid-from">Valid From</label>
                    <input type="date" id="edit-doc-valid-from">
                </div>
                <div class="form-group">
                    <label for="edit-doc-valid-until">Valid Until</label>
                    <input type="date" id="edit-doc-valid-until">
                </div>
            </div>

            <div class="feedback-box" id="edit-doc-feedback" style="margin-top: 15px;"></div>
            <div style="display:flex; flex-wrap:wrap; justify-content:space-between; align-items:center; gap:8px; margin-top:20px;">
                <button class="del-action-btn" onclick="deleteDocumentFromEditModal()">Delete Document</button>
                <div style="display:flex; flex-wrap:wrap; gap:8px;">
                    <button onclick="closeEditDocumentModal()" style="box-sizing:border-box; height:40px; padding:0 16px; font-size:14px; line-height:1; background:transparent; border:1px solid var(--border); color:var(--foreground); border-radius:6px; cursor:pointer;">Cancel</button>
                    <button id="edit-doc-save-btn" onclick="submitEditDocument()" style="box-sizing:border-box; height:40px; padding:0 16px; font-size:14px; line-height:1; background:var(--primary); border:1px solid var(--primary); color:var(--background); border-radius:6px; cursor:pointer; font-weight:700;">Save</button>
                </div>
            </div>
        </div>
    </div>

    <script>
        function isAdminUser(user) {
            return !!(user && (
                user.can_view_users || user.can_manage_users ||
                user.can_manage_documents || user.can_manage_roles ||
                user.can_manage_designations || user.can_manage_categories ||
                user.can_review_process || user.can_view_roles || user.can_view_all_processes
            ));
        }

        // ---- Sidebar toggle (mobile) ----
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

        // Truncates long, unbounded text (emails, titles, labels) to a fixed length with
        // "…" so it can't blow out table/flex layouts. Pair with truncTitle() on the same
        // element so the full value is still readable on hover.
        function truncText(text, maxLen) {
            const str = (text === null || text === undefined) ? '' : text.toString();
            return str.length > maxLen ? str.slice(0, maxLen - 1) + '…' : str;
        }
        function truncTitle(text) {
            return escapeHtml((text === null || text === undefined) ? '' : text.toString());
        }

        // Replaces native confirm() — resolves true/false once the user picks.
        let _confirmResolve = null;
        function showConfirm(message, okLabel) {
            document.getElementById('confirm-modal-message').textContent = message;
            document.getElementById('confirm-modal-ok-btn').textContent = okLabel || 'Confirm';
            document.getElementById('confirm-modal').style.display = 'flex';
            return new Promise(resolve => { _confirmResolve = resolve; });
        }
        function closeConfirmModal(result) {
            document.getElementById('confirm-modal').style.display = 'none';
            if (_confirmResolve) {
                const resolve = _confirmResolve;
                _confirmResolve = null;
                resolve(result);
            }
        }

        // API error bodies are either a bare JSON string or a {message: "..."} object —
        // reading the raw response text directly would show the JSON quoting/escaping
        // to the user. This unwraps either shape down to a plain message.
        async function readErrorMessage(response) {
            const raw = await response.text();
            try {
                const parsed = JSON.parse(raw);
                if (typeof parsed === 'string') return parsed;
                if (parsed && typeof parsed.message === 'string') return parsed.message;
            } catch (e) {}
            return raw;
        }

        const API_BASE = window.location.port === "4000" ? `http://${window.location.hostname}:3000` : "";
        const token = localStorage.getItem('vfs_token');
        const userStr = localStorage.getItem('vfs_user');
        const user = userStr ? JSON.parse(userStr) : null;
        const canManageRoles = !!(user && user.can_manage_roles);
        const canManageDesignations = !!(user && user.can_manage_designations);
        const canManageCategories = !!(user && user.can_manage_categories);
        const canViewUsers = !!(user && user.can_view_users);
        const canManageUsers = !!(user && user.can_manage_users);
        const canManageDocuments = !!(user && user.can_manage_documents);
        const canViewRoles = !!(user && user.can_view_roles);
        let defaultRolesSubtab = 'user-designations';
        if (canManageRoles || canManageDesignations || canViewRoles) {
            const rolesBtn = document.getElementById('suite-btn-roles');
            if (rolesBtn) rolesBtn.style.display = 'block';
            const userDesigBtn = document.getElementById('subtab-btn-user-designations');
            const manageDesigBtn = document.getElementById('subtab-btn-manage-designations');
            if ((canManageRoles || canViewRoles) && userDesigBtn) userDesigBtn.style.display = 'inline-block';
            if (canManageDesignations && manageDesigBtn) manageDesigBtn.style.display = 'inline-block';
            defaultRolesSubtab = (canManageRoles || canViewRoles) ? 'user-designations' : 'manage-designations';
        }
        // Add Staff hits /users/create-staff, which the backend gates on users.manage —
        // not users.roles.manage — so its visibility must track that permission
        // independently of the surrounding Roles suite's own gate.
        if (canManageUsers) {
            const addStaffBtn = document.getElementById('add-staff-btn');
            if (addStaffBtn) addStaffBtn.style.display = 'inline-block';
        }
        if (canManageCategories || canManageDesignations) {
            const categoriesBtn = document.getElementById('subtab-btn-categories');
            if (categoriesBtn) categoriesBtn.style.display = 'inline-block';
        }
        // Explorer/Add Document deal with actual document content, gated on
        // canManageDocuments — a categories-only manager has no business there and
        // should land straight on (and only ever see) the Categories subtab.
        if (!canManageDocuments) {
            const dbBtn = document.getElementById('subtab-btn-db');
            if (dbBtn) dbBtn.style.display = 'none';
            const registerBtn = document.getElementById('subtab-btn-register');
            if (registerBtn) registerBtn.style.display = 'none';
        }
        if (canManageDesignations) {
            const categoryAccessCard = document.getElementById('category-access-card');
            if (categoryAccessCard) categoryAccessCard.style.display = 'block';
        }
        if (canViewUsers) {
            const residentsBtn = document.getElementById('suite-btn-residents');
            if (residentsBtn) residentsBtn.style.display = 'block';
        }
        if (!canManageUsers) {
            const leftColumn = document.getElementById('residents-left-column');
            if (leftColumn) leftColumn.style.display = 'none';
            const splitGrid = document.getElementById('residents-split-grid');
            if (splitGrid) splitGrid.style.gridTemplateColumns = '1fr';
        }
        if (canManageDocuments || canManageCategories) {
            const documentsBtn = document.getElementById('suite-btn-documents');
            if (documentsBtn) documentsBtn.style.display = 'block';
        }
        const hasFlat = !!(user && user.wing !== 'TBD' && user.flat_number !== 'TBD');
        const canViewAllProcesses = !!(user && user.can_view_all_processes);
        const myFlatDashboardLink = document.getElementById('my-flat-dashboard-link');
        if (myFlatDashboardLink) myFlatDashboardLink.style.display = hasFlat ? 'block' : 'none';
        // Only show a Processes link here if this account actually holds process oversight
        // permission. Personal-flat processes are NOT admin business — they're already
        // reachable via "My Flat Dashboard →" -> /dashboard -> Processes, so duplicating
        // that here off of hasFlat alone just clutters the admin nav with a dead-end link
        // for accounts that have a flat but no process-related permission at all.
        const workflowsBtn = document.getElementById('suite-btn-workflows');
        if (workflowsBtn) {
            workflowsBtn.style.display = canViewAllProcesses ? 'block' : 'none';
            workflowsBtn.href = '/admin-process';
            workflowsBtn.textContent = 'Process Review';
        }
        const defaultSuite = canViewUsers ? 'residents' : ((canManageDocuments || canManageCategories) ? 'documents' : (canManageRoles || canManageDesignations || canViewRoles ? 'roles' : null));

        if (!token || !user) {
            localStorage.clear();
            window.location.href = '/login';
        } else {
            if (!isAdminUser(user)) {
                window.location.href = '/dashboard';
            }

            // Sync user profile from server in background
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
                        if (!isAdminUser(freshUser)) {
                            window.location.href = '/dashboard';
                            return;
                        }
                        // All capability flags above (canManageRoles, canViewRoles, etc.) were
                        // computed once, synchronously, from the localStorage snapshot at page
                        // load. If a designation/permission was granted or revoked after this
                        // tab's last login, that snapshot is stale — reload so every gate on
                        // this page recomputes from the fresh grant instead of silently sitting
                        // on outdated access until the user manually logs out and back in.
                        if (JSON.stringify(freshUser) !== JSON.stringify(user)) {
                            window.location.reload();
                        }
                    } else if (response.status === 401) {
                        localStorage.clear();
                        window.location.href = '/login';
                    }
                } catch (err) {
                    console.error("Failed to refresh user profile:", err);
                }
            }
            refreshProfile();

            document.getElementById('logout-btn').addEventListener('click', () => {
                localStorage.clear();
                window.location.href = '/login';
            });
        }

        let uploadFormCategories = [];

        function handleCategoryChange() {
            const select = document.getElementById('doc-category');
            const cat = select.value;

            // Show/hide flat mapping section for RESIDENT_SPECIFIC category
            const flatSection = document.getElementById('flat-mapping-section');
            flatSection.style.display = (cat === 'RESIDENT_SPECIFIC') ? 'block' : 'none';

            const subcatSelect = document.getElementById('doc-subcategory');
            const selectedCat = uploadFormCategories.find(c => c.code === cat);
            const matching = selectedCat ? selectedCat.subcategories : [];
            subcatSelect.innerHTML = '<option value="">None</option>' +
                matching.map(s => `<option value="${escapeHtml(s.code)}">${escapeHtml(s.label)}</option>`).join('');

            // Toggling that section shifts page layout right below the still-focused
            // select — browsers then auto-scroll to keep it in view, which reads as an
            // unwanted jump right as the dropdown closes. Blurring avoids that.
            select.blur();
        }

        let flatOwnerLookupTimer = null;

        function scheduleFlatOwnerLookup() {
            clearTimeout(flatOwnerLookupTimer);
            flatOwnerLookupTimer = setTimeout(runFlatOwnerLookup, 400);
        }

        async function runFlatOwnerLookup() {
            const preview = document.getElementById('flat-owner-preview');
            const wing = document.getElementById('doc-flat-wing').value.trim();
            const flat_number = document.getElementById('doc-flat-number').value.trim();

            if (!wing || !flat_number) {
                preview.textContent = '';
                return;
            }

            preview.textContent = 'Checking...';
            try {
                const resp = await fetch(`${API_BASE}/api/v1/admin/documents/flat-owners`, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json', 'Authorization': `Bearer ${token}` },
                    body: JSON.stringify({ wing, flat_number })
                });
                if (!resp.ok) {
                    preview.textContent = '';
                    return;
                }
                const data = await resp.json();
                const emails = data.emails || [];
                if (emails.length === 0) {
                    preview.innerHTML = '<span style="color:var(--muted-foreground);">No active owner found for this flat.</span>';
                } else {
                    preview.innerHTML = `Will be visible to: <strong>${emails.map(escapeHtml).join(', ')}</strong>`;
                }
            } catch (err) {
                preview.textContent = '';
            }
        }

        async function loadUploadCategories() {
            const select = document.getElementById('doc-category');
            try {
                const resp = await fetch(`${API_BASE}/api/v1/admin/documents/categories`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}` }
                });
                if (resp.ok) {
                    uploadFormCategories = await resp.json();
                    select.innerHTML = uploadFormCategories.map(c => `<option value="${escapeHtml(c.code)}">${escapeHtml(formatCategory(c.code))}</option>`).join('');
                } else {
                    select.innerHTML = '<option value="">No categories available</option>';
                }
            } catch (err) {
                select.innerHTML = '<option value="">No categories available</option>';
            }

            handleCategoryChange();
        }

        // Initialize category dynamic selection
        loadUploadCategories();

        // Suite initialization from URL parameter
        const urlParams = new URLSearchParams(window.location.search);
        const initialTab = urlParams.get('tab');
        if (initialTab === 'documents' && (canManageDocuments || canManageCategories)) {
            switchSuite('documents');
        } else if (initialTab === 'roles' && (canManageRoles || canManageDesignations || canViewRoles)) {
            switchSuite('roles');
        } else if (initialTab === 'residents' && canViewUsers) {
            switchSuite('residents');
        } else if (defaultSuite) {
            switchSuite(defaultSuite);
        }

        // High Level Suite Toggle
        function switchSuite(suiteName) {
            // Keep the FOUC-prevention attribute (set once from the URL at page load,
            // see the inline <head> script) in sync so its CSS overrides don't fight
            // the .active class toggled below on later clicks.
            document.documentElement.setAttribute('data-tab', suiteName);

            document.querySelectorAll('.nav-item').forEach(item => item.classList.remove('active'));

            document.getElementById(`suite-btn-${suiteName}`).classList.add('active');
            
            // Switch panels
            document.getElementById('suite-residents').style.display = suiteName === 'residents' ? 'block' : 'none';
            document.getElementById('suite-documents').style.display = suiteName === 'documents' ? 'block' : 'none';
            const rolesPanel = document.getElementById('suite-roles');
            if (rolesPanel) rolesPanel.style.display = suiteName === 'roles' ? 'block' : 'none';

            if (suiteName === 'documents') {
                // Always explicitly (re-)select the right subtab on entry — otherwise
                // whichever subtab was left active from a previous visit stays shown
                // without its data ever being (re-)loaded, and the suite renders blank.
                switchSubtab((!canManageDocuments && canManageCategories) ? 'categories' : 'db');
            } else if (suiteName === 'roles') {
                switchSubtab(defaultRolesSubtab);
                loadRolesData();
            }
        }

        // Document Suite Subtab Toggle
        function switchSubtab(subName) {
            document.querySelectorAll('.subtab-btn').forEach(btn => btn.classList.remove('active'));
            document.querySelectorAll('.subtab-panel').forEach(p => p.classList.remove('active'));

            document.getElementById(`subtab-btn-${subName}`).classList.add('active');
            document.getElementById(`subtab-panel-${subName}`).classList.add('active');
            
            if (subName === 'db') {
                loadAdminDocuments();
            } else if (subName === 'categories') {
                loadCategoriesTabData();
            }
        }

        // Resident Onboarding Handling
        const onboardForm = document.getElementById('onboard-form');
        const feedback = document.getElementById('feedback');

        onboardForm.addEventListener('submit', async (e) => {
            e.preventDefault();
            feedback.style.display = 'none';

            const email = document.getElementById('email').value.trim();
            const wing = document.getElementById('wing').value.trim();
            const flat_number = document.getElementById('flat').value.trim();

            try {
                const response = await fetch(`${API_BASE}/api/v1/admin/users/create`, {
                    method: 'POST',
                    headers: { 
                        'Content-Type': 'application/json',
                        'Authorization': `Bearer ${token}`
                    },
                    body: JSON.stringify({ email, wing, flat_number })
                });

                const data = await response.json();

                if (response.ok) {
                    feedback.textContent = '✓ Resident added!';
                    feedback.className = 'feedback-box success';
                    feedback.style.display = 'block';
                    onboardForm.reset();

                    loadUsers();
                } else {
                    feedback.textContent = data.message || 'Failed to onboard resident';
                    feedback.className = 'feedback-box error';
                    feedback.style.display = 'block';
                }
            } catch (err) {
                feedback.textContent = 'Failed to connect to backend server';
                feedback.className = 'feedback-box error';
                feedback.style.display = 'block';
            }
        });

        // Bulk Upload Form
        const uploadCsvForm = document.getElementById('upload-csv-form');
        const uploadFeedback = document.getElementById('upload-feedback');
        const uploadResultsSection = document.getElementById('upload-results');
        const uploadSuccessCount = document.getElementById('upload-success-count');
        const uploadErrorCount = document.getElementById('upload-error-count');
        const uploadErrorsTarget = document.getElementById('upload-errors-target');

        uploadCsvForm.addEventListener('submit', async (e) => {
            e.preventDefault();
            uploadFeedback.style.display = 'none';
            uploadResultsSection.style.display = 'none';

            const fileInput = document.getElementById('csv-file');
            if (fileInput.files.length === 0) return;

            const formData = new FormData();
            formData.append('file', fileInput.files[0]);

            try {
                const response = await fetch(`${API_BASE}/api/v1/admin/users/upload`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}` },
                    body: formData
                });

                const data = await response.json();

                if (response.ok) {
                    uploadFeedback.textContent = `Processed ${data.total_rows} rows.`;
                    uploadFeedback.className = 'feedback-box success';
                    uploadFeedback.style.display = 'block';

                    uploadResultsSection.style.display = 'block';
                    uploadSuccessCount.textContent = data.successfully_onboarded;
                    uploadErrorCount.textContent = data.failed_users.length;

                    uploadErrorsTarget.innerHTML = '';
                    for (const err of data.failed_users) {
                        const tr = document.createElement('tr');
                        tr.innerHTML = `
                            <td style="padding: 8px;">${err.email}</td>
                            <td style="padding: 8px; color: var(--destructive);">${err.reason}</td>
                        `;
                        uploadErrorsTarget.appendChild(tr);
                    }

                    fileInput.value = '';
                    loadUsers();
                } else {
                    uploadFeedback.textContent = data.message || 'Failed to process CSV file';
                    uploadFeedback.className = 'feedback-box error';
                    uploadFeedback.style.display = 'block';
                }
            } catch (err) {
                uploadFeedback.textContent = 'Failed to connect to backend server';
                uploadFeedback.className = 'feedback-box error';
                uploadFeedback.style.display = 'block';
            }
        });

        // Resident Directory Handling
        let allUsers = [];
        async function loadUsers() {
            const tbody = document.getElementById('user-rows-target');
            try {
                const response = await fetch(`${API_BASE}/api/v1/admin/users/list`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}` }
                });

                if (response.ok) {
                    allUsers = await response.json();
                    renderUsers(allUsers);
                } else {
                    tbody.innerHTML = `<tr><td colspan="3" style="text-align: center; color: var(--foreground); font-weight: 600;">Failed to load residents.</td></tr>`;
                }
            } catch (err) {
                tbody.innerHTML = '<tr><td colspan="3" style="text-align: center; color: var(--foreground); font-weight: 600;">Error connecting to API server</td></tr>';
            }
        }

        function openRevokeFlatModal(userId, wingsStr, flatsStr) {
            const wings = wingsStr.split(', ');
            const flats = flatsStr.split(', ');
            const select = document.getElementById('revoke-flat-select');
            select.innerHTML = '';
            
            for (let i = 0; i < wings.length; i++) {
                if (wings[i] && flats[i] && wings[i] !== 'TBD' && flats[i] !== 'TBD') {
                    const opt = document.createElement('option');
                    opt.value = JSON.stringify({wing: wings[i], flat: flats[i]});
                    opt.textContent = `Wing ${wings[i]} - Flat ${flats[i]}`;
                    select.appendChild(opt);
                }
            }
            
            if (select.options.length === 0) {
                alert("No active flat mappings to revoke.");
                return;
            }
            
            document.getElementById('revoke-flat-user-id').value = userId;
            document.getElementById('revoke-feedback').innerHTML = '';
            document.getElementById('revoke-feedback').className = 'feedback-box';
            document.getElementById('revoke-flat-modal').style.display = 'flex';
        }
        
        function closeRevokeFlatModal() {
            document.getElementById('revoke-flat-modal').style.display = 'none';
        }
        
        async function submitRevokeFlat() {
            const userId = parseInt(document.getElementById('revoke-flat-user-id').value);
            const select = document.getElementById('revoke-flat-select');
            const feedback = document.getElementById('revoke-feedback');
            
            if (!select.value) return;
            const target = JSON.parse(select.value);
            
            try {
                const response = await fetch(`${API_BASE}/api/v1/admin/users/revoke-flat`, {
                    method: 'POST',
                    headers: {
                        'Content-Type': 'application/json',
                        'Authorization': `Bearer ${token}`
                    },
                    body: JSON.stringify({
                        user_id: userId,
                        wing: target.wing,
                        flat_number: target.flat
                    })
                });
                
                const data = await response.json();
                
                if (response.ok) {
                    feedback.className = 'feedback-box success';
                    feedback.innerHTML = `<span class="icon">✓</span> ${escapeHtml(data.message)}`;
                    setTimeout(() => {
                        closeRevokeFlatModal();
                        loadUsers();
                    }, 1000);
                } else {
                    feedback.className = 'feedback-box error';
                    feedback.innerHTML = `<span class="icon">!</span> ${escapeHtml(data.message || 'Failed to revoke flat.')}`;
                }
            } catch (err) {
                feedback.className = 'feedback-box error';
                feedback.innerHTML = `<span class="icon">!</span> Network error communicating with server.`;
            }
        }
        function renderUsers(usersList) {
            const tbody = document.getElementById('user-rows-target');
            tbody.innerHTML = '';

            if (usersList.length === 0) {
                tbody.innerHTML = '<tr><td colspan="2" style="text-align: center; color: var(--muted-foreground); font-weight: 600;">No residents found.</td></tr>';
                return;
            }

            usersList.forEach(u => {
                const tr = document.createElement('tr');
                const hasAdmin = u.designations.includes('super_admin');

                let mappingText = '';
                if (hasAdmin) {
                    mappingText = `<span style="color:var(--foreground); font-style:italic;">Admin Account</span>`;
                } else if (u.wing === 'TBD' || u.flat_number === 'TBD') {
                    // No active flat mapping (e.g. revoked) — not necessarily an admin;
                    // they may still hold a non-admin designation, which is why they're
                    // listed at all (see get_all_users' WHERE clause).
                    mappingText = `<span style="color:var(--muted-foreground); font-style:italic;">No Flat Assigned</span>`;
                } else {
                    const wings = u.wing.split(', ');
                    const flats = u.flat_number.split(', ');
                    const mappings = wings.map((w, i) => `${escapeHtml(w)}-${escapeHtml(flats[i])}`).join(', ');
                    mappingText = `Flats: <strong>${mappings}</strong>`;
                }

                // Only show Revoke Flat if they actually have a flat assigned, and the caller can manage users
                const hasRealFlat = u.wing !== 'TBD' && u.flat_number !== 'TBD';
                const revokeFlatHtml = (hasRealFlat && canManageUsers)
                    ? `<button class="del-action-btn" style="font-size: 11px; padding: 6px 10px; white-space: nowrap;" onclick="openRevokeFlatModal(${u.user_id}, '${escapeHtml(u.wing)}', '${escapeHtml(u.flat_number)}')">Revoke Flat</button>`
                    : '';

                tr.innerHTML = `
                    <td>
                        <div class="truncate" style="font-weight: 700; color: var(--foreground); max-width:260px;" title="${truncTitle(u.email)}">${escapeHtml(truncText(u.email, 40))}</div>
                        <div style="font-size: 12px; color: var(--muted-foreground); margin-top: 3px;">${mappingText}</div>
                    </td>
                    <td>
                        <div style="display: flex; flex-direction: row; align-items: center; justify-content: center; gap: 10px;">
                            ${revokeFlatHtml}
                        </div>
                    </td>
                `;

                tbody.appendChild(tr);
            });
        }

        document.getElementById('search-input').addEventListener('input', (e) => {
            const query = e.target.value.toLowerCase().trim();
            const filtered = allUsers.filter(u => {
                const mapStr = `${u.wing}-${u.flat_number}`.toLowerCase();
                return u.email.toLowerCase().includes(query) || 
                       mapStr.includes(query);
            });
            renderUsers(filtered);
        });

        // ==========================================
        // ROLES TAB — user designations + document category access matrix
        // ==========================================

        let allNonSystemDesignations = [];
        let allPermissions = [];
        let allNonSystemCategories = [];

        // Fixed display order + friendly copy for the permission matrix. Any permission
        // the backend returns that isn't listed here still renders (falls back to its raw
        // code/label), appended after these — keeps the UI forward-compatible with new
        // permissions the server adds before this map is updated.
        const PERMISSION_ORDER = [
            'users.view', 'users.manage', 'users.roles.view', 'users.roles.manage',
            'designations.manage', 'documents.all.manage', 'documents.categories.manage',
            'process.view', 'process.reviewer',
        ];
        const PERMISSION_INFO = {
            'users.view': { title: 'View Residents', desc: 'View all residents.' },
            'users.manage': { title: 'Manage Residents', desc: 'View and manage all residents.' },
            'users.roles.view': { title: 'View Designations', desc: 'View the designations held by users. This naturally also gives view access to the full resident list.' },
            'users.roles.manage': { title: 'Manage Users Designations', desc: 'Change which designation(s) a user holds. This naturally also gives view access to the full resident list.' },
            'designations.manage': { title: 'Manage Designation Setup', desc: 'Add new designations, modify the permissions a designation grants, and change which document categories it can view or manage.' },
            'documents.all.manage': { title: 'Manage All Documents', desc: 'View and manage documents in every category, regardless of per-category access.' },
            'documents.categories.manage': { title: 'Manage Document Categories', desc: 'Add new document categories.' },
            'process.view': { title: 'View Processes', desc: 'View the processes of all residents, including the files attached to them. Cannot approve or deny steps.' },
            'process.reviewer': { title: 'Review Processes', desc: 'Review resident processes: approve or deny submitted steps, and view the files attached to them.' },
        };

        function sortPermissionsForDisplay(permissions) {
            const rank = code => {
                const i = PERMISSION_ORDER.indexOf(code);
                return i === -1 ? PERMISSION_ORDER.length : i;
            };
            return [...permissions].sort((a, b) => rank(a.code) - rank(b.code));
        }

        function showPermInfo(code) {
            const info = PERMISSION_INFO[code];
            const perm = allPermissions.find(p => p.code === code);
            document.getElementById('perm-info-title').textContent = (info && info.title) || (perm && perm.label) || code;
            document.getElementById('perm-info-desc').textContent = (info && info.desc) || (perm && perm.label) || 'No description available.';
            document.getElementById('perm-info-modal').style.display = 'flex';
        }

        function closePermInfoModal() {
            document.getElementById('perm-info-modal').style.display = 'none';
        }

        async function loadRolesData() {
            await loadNonSystemDesignations();

            if (canManageRoles || canViewRoles) {
                await loadUsers();
                renderRolesUserTable(allUsers);
            }

            if (canManageDesignations) {
                renderDesignationsList();
                await loadPermissionsCatalog();
                renderDesignationPermissionsMatrix();
            }
        }

        async function loadCategoriesTabData() {
            await loadNonSystemDesignations();
            await loadNonSystemCategories();
            renderCategoriesList();
            await loadCategoryMatrix();
            populateSubcategoryParentSelect();
            renderSubcategoriesList();
        }

        let allCategories = [];
        // Categories a designation can be granted read/manage access to. This is
        // deliberately broader than allNonSystemCategories (which drives the All
        // Categories CRUD list, where is_system rows correctly can't be renamed/deleted):
        // PUBLIC is is_system but designations still need to be grantable manage access
        // to it (so e.g. a non-admin role can be allowed to post public documents).
        // RESIDENT_SPECIFIC stays excluded — it's inherently flat-scoped/personal, not
        // something granted to a designation.
        let allAccessGrantableCategories = [];

        async function loadNonSystemCategories() {
            try {
                const resp = await fetch(`${API_BASE}/api/v1/admin/roles/document-categories`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}` }
                });
                if (resp.ok) {
                    allCategories = await resp.json();
                    allNonSystemCategories = allCategories.filter(c => !c.is_system);
                    allAccessGrantableCategories = allCategories.filter(c => c.code !== 'RESIDENT_SPECIFIC');
                }
            } catch (err) {
                console.error('Failed to load document categories:', err);
            }
        }

        function renderCategoriesList() {
            const tbody = document.getElementById('categories-list-rows-target');
            if (allCategories.length === 0) {
                tbody.innerHTML = '<tr><td colspan="2" style="text-align: center; color: var(--muted-foreground); font-weight: 600;">No categories yet.</td></tr>';
                return;
            }
            tbody.innerHTML = allCategories.map(c => `
                <tr>
                    <td style="font-weight:700;"><span class="truncate" style="max-width:280px;" title="${truncTitle(c.label)}">${escapeHtml(truncText(c.label, 50))}</span></td>
                    <td style="text-align:center;">
                        ${canManageCategories ? `<button class="action-select" style="cursor:pointer;" onclick="editDocumentCategory('${escapeHtml(c.code)}', '${escapeHtml(c.label)}', ${c.is_system})">Edit</button>` : ''}
                    </td>
                </tr>
            `).join('');
        }

        function editDocumentCategory(code, currentLabel, isSystem) {
            openRenameModal('category', code, currentLabel, isSystem);
        }

        async function submitCreateCategory() {
            const feedback = document.getElementById('create-category-feedback');
            const labelInput = document.getElementById('new-category-label');
            const label = labelInput.value.trim();
            const code = label.toUpperCase().replace(/\s+/g, '_');

            feedback.style.display = 'none';
            if (!label) return;

            try {
                const resp = await fetch(`${API_BASE}/api/v1/admin/roles/document-categories/create`, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json', 'Authorization': `Bearer ${token}` },
                    body: JSON.stringify({ code, label })
                });
                const data = await resp.json();
                feedback.style.display = 'block';
                if (resp.ok) {
                    feedback.className = 'feedback-box success';
                    feedback.innerHTML = `<span class="icon">✓</span> Category "${escapeHtml(label)}" created.`;
                    labelInput.value = '';
                    await loadNonSystemCategories();
                    renderCategoriesList();
                    await loadCategoryMatrix();
                } else {
                    feedback.className = 'feedback-box error';
                    feedback.innerHTML = `<span class="icon">!</span> ${escapeHtml(data.message || 'Failed to create category.')}`;
                }
            } catch (err) {
                feedback.style.display = 'block';
                feedback.className = 'feedback-box error';
                feedback.innerHTML = `<span class="icon">!</span> Network error communicating with server.`;
            }
        }

        function populateSubcategoryParentSelect() {
            const select = document.getElementById('subcategory-parent-select');
            const prev = select.value;
            select.innerHTML = allCategories.map(c => `<option value="${escapeHtml(c.code)}">${escapeHtml(c.label)}</option>`).join('');
            select.value = allCategories.some(c => c.code === prev) ? prev : (allCategories[0] ? allCategories[0].code : '');
        }

        function renderSubcategoriesList() {
            const tbody = document.getElementById('subcategories-list-rows-target');
            const category = document.getElementById('subcategory-parent-select').value;
            const selectedCat = allCategories.find(c => c.code === category);
            const rows = selectedCat ? selectedCat.subcategories : [];
            if (rows.length === 0) {
                tbody.innerHTML = '<tr><td colspan="2" style="text-align: center; color: var(--muted-foreground); font-weight: 600;">No subcategories yet.</td></tr>';
                return;
            }
            tbody.innerHTML = rows.map(s => `
                <tr>
                    <td style="font-weight:700;"><span class="truncate" style="max-width:280px;" title="${truncTitle(s.label)}">${escapeHtml(truncText(s.label, 50))}</span></td>
                    <td>
                        ${canManageCategories ? `<button class="action-select" style="cursor:pointer;" onclick="renameDocumentSubcategory(${s.subcategory_id}, '${escapeHtml(s.label)}')">Rename</button>` : ''}
                    </td>
                </tr>
            `).join('');
        }

        function renameDocumentSubcategory(subcategoryId, currentLabel) {
            openRenameModal('subcategory', String(subcategoryId), currentLabel);
        }

        async function submitCreateSubcategory() {
            const feedback = document.getElementById('create-subcategory-feedback');
            const labelInput = document.getElementById('new-subcategory-label');
            const label = labelInput.value.trim();
            const code = label.toUpperCase().replace(/\s+/g, '_');
            const category = document.getElementById('subcategory-parent-select').value;

            feedback.style.display = 'none';
            if (!label || !category) return;

            try {
                const resp = await fetch(`${API_BASE}/api/v1/admin/roles/document-subcategories/create`, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json', 'Authorization': `Bearer ${token}` },
                    body: JSON.stringify({ category, code, label })
                });
                const data = await resp.json();
                feedback.style.display = 'block';
                if (resp.ok) {
                    feedback.className = 'feedback-box success';
                    feedback.innerHTML = `<span class="icon">✓</span> Subcategory "${escapeHtml(label)}" created.`;
                    labelInput.value = '';
                    await loadNonSystemCategories();
                    renderSubcategoriesList();
                } else {
                    feedback.className = 'feedback-box error';
                    feedback.innerHTML = `<span class="icon">!</span> ${escapeHtml(data.message || 'Failed to create subcategory.')}`;
                }
            } catch (err) {
                feedback.style.display = 'block';
                feedback.className = 'feedback-box error';
                feedback.innerHTML = `<span class="icon">!</span> Network error communicating with server.`;
            }
        }

        let allDesignations = [];

        async function loadNonSystemDesignations() {
            try {
                const resp = await fetch(`${API_BASE}/api/v1/admin/roles/designations`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}` }
                });
                if (resp.ok) {
                    allDesignations = await resp.json();
                    allNonSystemDesignations = allDesignations.filter(d => !d.is_system);
                }
            } catch (err) {
                console.error('Failed to load designations:', err);
            }
        }

        function renderDesignationsList() {
            const tbody = document.getElementById('designations-list-rows-target');
            if (allDesignations.length === 0) {
                tbody.innerHTML = '<tr><td colspan="2" style="text-align: center; color: var(--muted-foreground); font-weight: 600;">No designations yet.</td></tr>';
                return;
            }
            tbody.innerHTML = allDesignations.map(d => `
                <tr>
                    <td style="font-weight:700;"><span class="truncate" style="max-width:280px;" title="${truncTitle(d.label)}">${escapeHtml(truncText(d.label, 50))}</span></td>
                    <td style="text-align:center;">
                        <button class="action-select" style="cursor:pointer;" onclick="editDesignation('${escapeHtml(d.code)}', '${escapeHtml(d.label)}', ${d.is_system})">Edit</button>
                    </td>
                </tr>
            `).join('');
        }

        function editDesignation(code, currentLabel, isSystem) {
            openRenameModal('designation', code, currentLabel, isSystem);
        }

        const RENAME_MODAL_TITLES = { category: 'Edit Category', subcategory: 'Edit Subcategory', designation: 'Edit Designation' };

        // Subcategories have no delete action at all (see deleteFromRenameModal); category/
        // designation just can't be deleted while system-protected or (for designations)
        // still assigned to someone — that check happens server-side, this only hides the
        // button for the one case we know client-side (is_system).
        function openRenameModal(kind, code, currentLabel, isSystem) {
            document.getElementById('rename-modal-kind').value = kind;
            document.getElementById('rename-modal-code').value = code;
            document.getElementById('rename-modal-title').textContent = RENAME_MODAL_TITLES[kind] || 'Edit';
            const input = document.getElementById('rename-modal-input');
            input.value = currentLabel;
            document.getElementById('rename-modal-feedback').innerHTML = '';
            document.getElementById('rename-modal-feedback').style.display = 'none';

            const deleteBtn = document.getElementById('rename-modal-delete-btn');
            deleteBtn.style.display = (kind !== 'subcategory' && !isSystem) ? 'inline-block' : 'none';

            document.getElementById('rename-modal').style.display = 'flex';
            input.focus();
            input.select();
        }

        function closeRenameModal() {
            document.getElementById('rename-modal').style.display = 'none';
        }

        async function submitRenameModal() {
            const kind = document.getElementById('rename-modal-kind').value;
            const code = document.getElementById('rename-modal-code').value;
            const newLabel = document.getElementById('rename-modal-input').value.trim();
            const feedback = document.getElementById('rename-modal-feedback');
            const saveBtn = document.getElementById('rename-modal-save-btn');

            if (!newLabel) {
                feedback.style.display = 'block';
                feedback.className = 'feedback-box error';
                feedback.innerHTML = 'Label cannot be empty.';
                return;
            }

            const RENAME_URLS = {
                category: `${API_BASE}/api/v1/admin/roles/document-categories/rename`,
                subcategory: `${API_BASE}/api/v1/admin/roles/document-subcategories/rename`,
                designation: `${API_BASE}/api/v1/admin/roles/designations/rename`,
            };
            const body = kind === 'subcategory'
                ? { subcategory_id: parseInt(code, 10), label: newLabel }
                : { code, label: newLabel };

            saveBtn.disabled = true;
            saveBtn.textContent = 'Saving...';
            try {
                const resp = await fetch(RENAME_URLS[kind], {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json', 'Authorization': `Bearer ${token}` },
                    body: JSON.stringify(body)
                });
                const data = await resp.json();
                if (resp.ok) {
                    closeRenameModal();
                    if (kind === 'category') {
                        await loadNonSystemCategories();
                        renderCategoriesList();
                        await loadCategoryMatrix();
                    } else if (kind === 'subcategory') {
                        await loadNonSystemCategories();
                        renderSubcategoriesList();
                    } else {
                        await loadNonSystemDesignations();
                        renderDesignationsList();
                        renderDesignationPermissionsMatrix();
                    }
                } else {
                    feedback.style.display = 'block';
                    feedback.className = 'feedback-box error';
                    feedback.innerHTML = data.message || 'Failed to rename.';
                }
            } catch (err) {
                feedback.style.display = 'block';
                feedback.className = 'feedback-box error';
                feedback.innerHTML = 'Network error communicating with server.';
            } finally {
                saveBtn.disabled = false;
                saveBtn.textContent = 'Save';
            }
        }

        async function deleteDesignation(code) {
            if (!(await showConfirm(`Delete designation "${code}"? This only works if no users currently hold it.`, 'Delete'))) return false;
            try {
                const resp = await fetch(`${API_BASE}/api/v1/admin/roles/designations/delete`, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json', 'Authorization': `Bearer ${token}` },
                    body: JSON.stringify({ code })
                });
                const data = await resp.json();
                if (resp.ok) {
                    await loadNonSystemDesignations();
                    renderDesignationsList();
                    renderDesignationPermissionsMatrix();
                    return true;
                } else {
                    alert(data.message || 'Failed to delete designation.');
                    return false;
                }
            } catch (err) {
                alert('Network error communicating with server.');
                return false;
            }
        }

        async function deleteFromRenameModal() {
            const kind = document.getElementById('rename-modal-kind').value;
            const code = document.getElementById('rename-modal-code').value;
            const deleted = kind === 'category'
                ? await deleteDocumentCategory(code)
                : await deleteDesignation(code);
            if (deleted) closeRenameModal();
        }

        function renderRolesUserTable(usersList) {
            const tbody = document.getElementById('roles-user-rows-target');
            tbody.innerHTML = '';

            // Everyone (including yourself) is listed here — this is a visibility list,
            // not just an edit queue. Self-demotion protection only disables the Manage
            // Designations action on your own row (and is independently enforced by the
            // backend); it must never make you disappear from a list you have permission
            // to view.
            const manageable = usersList
                .filter(u => !u.designations.includes('super_admin'))
                .sort((a, b) => (b.designations.length > 0 ? 1 : 0) - (a.designations.length > 0 ? 1 : 0));

            if (manageable.length === 0) {
                tbody.innerHTML = '<tr><td colspan="3" style="text-align: center; color: var(--muted-foreground); font-weight: 600;">No residents found.</td></tr>';
                return;
            }

            manageable.forEach(u => {
                const tr = document.createElement('tr');
                tr.innerHTML = `
                    <td><div class="truncate" style="font-weight: 700; color: var(--foreground); max-width:260px;" title="${truncTitle(u.email)}">${escapeHtml(truncText(u.email, 40))}</div></td>
                    <td>
                        ${u.designations.length > 0
                            ? u.designations.map(d => `<span class="badge ${d.toLowerCase()}">${escapeHtml(d)}</span>`).join('')
                            : '<span style="color:var(--muted-foreground); font-size:12px;">Resident</span>'}
                    </td>
                    <td>
                        ${canManageRoles && u.user_id !== user.user_id
                            ? `<button class="action-select" style="cursor:pointer;" onclick="openManageDesignationsModal(${u.user_id}, '${escapeHtml(u.email)}')">Manage Designations</button>`
                            : (u.user_id === user.user_id
                                ? '<span style="color:var(--muted-foreground); font-size:12px;">(You)</span>'
                                : '')}
                    </td>
                `;
                tbody.appendChild(tr);
            });
        }

        document.getElementById('roles-search-input').addEventListener('input', (e) => {
            const query = e.target.value.toLowerCase().trim();
            const filtered = allUsers.filter(u => u.email.toLowerCase().includes(query));
            renderRolesUserTable(filtered);
        });

        function openAddStaffModal() {
            document.getElementById('add-staff-email').value = '';
            const feedback = document.getElementById('add-staff-feedback');
            feedback.style.display = 'none';
            feedback.innerHTML = '';
            document.getElementById('add-staff-modal').style.display = 'flex';
        }

        function closeAddStaffModal() {
            document.getElementById('add-staff-modal').style.display = 'none';
        }

        async function submitAddStaff() {
            const email = document.getElementById('add-staff-email').value.trim();
            const feedback = document.getElementById('add-staff-feedback');
            const btn = document.getElementById('add-staff-submit-btn');

            if (!email) {
                feedback.style.display = 'block';
                feedback.className = 'feedback-box error';
                feedback.innerHTML = `<span class="icon">!</span> Email is required.`;
                return;
            }

            btn.disabled = true;
            btn.textContent = 'Creating...';
            try {
                const response = await fetch(`${API_BASE}/api/v1/admin/users/create-staff`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}`, 'Content-Type': 'application/json' },
                    body: JSON.stringify({ email })
                });
                const data = await response.json();

                if (response.ok) {
                    await loadUsers();
                    renderRolesUserTable(allUsers);
                    closeAddStaffModal();
                } else {
                    feedback.style.display = 'block';
                    feedback.className = 'feedback-box error';
                    feedback.innerHTML = `<span class="icon">!</span> ${escapeHtml(data.message || 'Failed to create staff account.')}`;
                }
            } catch (err) {
                feedback.style.display = 'block';
                feedback.className = 'feedback-box error';
                feedback.innerHTML = `<span class="icon">!</span> Network error communicating with server.`;
            } finally {
                btn.disabled = false;
                btn.textContent = 'Create';
            }
        }

        function openManageDesignationsModal(userId, email) {
            const u = allUsers.find(x => x.user_id === userId);
            if (!u) return;

            document.getElementById('manage-designations-user-id').value = userId;
            document.getElementById('manage-designations-title').textContent = `Manage Designations: ${email}`;

            const currentNonSystem = u.designations.filter(d => d !== 'super_admin');
            const checklist = document.getElementById('manage-designations-checklist');
            checklist.innerHTML = allNonSystemDesignations.length > 0
                ? allNonSystemDesignations.map(d => `
                    <label style="display:flex; align-items:center; gap:12px; font-size:15px; cursor:pointer; padding:10px 8px; border-radius:6px; transition: background 0.15s;"
                        onmouseover="this.style.background='var(--muted)';" onmouseout="this.style.background='transparent';">
                        <input type="checkbox" class="designation-checklist-checkbox" data-designation="${escapeHtml(d.code)}" style="width:20px; height:20px; flex-shrink:0; margin:0;" ${currentNonSystem.includes(d.code) ? 'checked' : ''}>
                        <span style="word-break:break-word;">${escapeHtml(d.label)}</span>
                    </label>
                `).join('')
                : '<span style="color:var(--muted-foreground); font-size:12px;">No designations exist yet.</span>';

            document.getElementById('manage-designations-feedback').innerHTML = '';
            document.getElementById('manage-designations-modal').style.display = 'flex';
        }

        function closeManageDesignationsModal() {
            document.getElementById('manage-designations-modal').style.display = 'none';
        }

        async function saveManageDesignations() {
            const feedback = document.getElementById('manage-designations-feedback');
            const saveBtn = document.getElementById('manage-designations-save-btn');
            const userId = parseInt(document.getElementById('manage-designations-user-id').value, 10);
            const u = allUsers.find(x => x.user_id === userId);
            if (!u) return;

            const currentNonSystem = u.designations.filter(d => d !== 'super_admin');
            const changes = [];
            document.querySelectorAll('.designation-checklist-checkbox').forEach(cb => {
                const designationCode = cb.dataset.designation;
                const wasAssigned = currentNonSystem.includes(designationCode);
                if (cb.checked !== wasAssigned) {
                    changes.push({ designationCode, action: cb.checked ? 'assign' : 'revoke' });
                }
            });

            if (changes.length === 0) {
                feedback.className = 'feedback-box';
                feedback.innerHTML = 'No changes to save.';
                return;
            }

            const removals = changes.filter(c => c.action === 'revoke');
            if (removals.length > 0) {
                const names = removals.map(c => c.designationCode).join(', ');
                if (!(await showConfirm(`Remove designation(s) ${names} from this user? They will lose whatever access those grant.`, 'Remove'))) {
                    return;
                }
            }

            saveBtn.disabled = true;
            saveBtn.textContent = 'Saving...';
            try {
                for (const c of changes) {
                    const resp = await fetch(`${API_BASE}/api/v1/admin/roles/designations/${c.action}`, {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json', 'Authorization': `Bearer ${token}` },
                        body: JSON.stringify({ user_id: userId, designation_code: c.designationCode })
                    });
                    if (!resp.ok) {
                        const data = await resp.json();
                        throw new Error(data.message || 'Failed to update designation.');
                    }
                }
                await loadUsers();
                renderRolesUserTable(allUsers);
                closeManageDesignationsModal();
            } catch (err) {
                feedback.className = 'feedback-box error';
                feedback.innerHTML = `<span class="icon">!</span> ${escapeHtml(err.message || 'Network error communicating with server.')}`;
            } finally {
                saveBtn.disabled = false;
                saveBtn.textContent = 'Save Changes';
            }
        }

        let allCategoryDesignationRows = [];

        async function loadCategoryMatrix() {
            const tbody = document.getElementById('category-access-designations-rows-target');

            try {
                const resp = await fetch(`${API_BASE}/api/v1/admin/roles/document-category-designations`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}` }
                });
                if (!resp.ok) {
                    tbody.innerHTML = '<tr><td colspan="2" style="text-align: center; color: var(--danger);">Failed to load access data.</td></tr>';
                    return;
                }
                allCategoryDesignationRows = await resp.json();
                renderCategoryAccessDesignationsList();
            } catch (err) {
                tbody.innerHTML = '<tr><td colspan="2" style="text-align: center; color: var(--danger);">Connection error.</td></tr>';
            }
        }

        function renderCategoryAccessDesignationsList() {
            const tbody = document.getElementById('category-access-designations-rows-target');

            if (allNonSystemDesignations.length === 0) {
                tbody.innerHTML = '<tr><td colspan="2" style="text-align: center; color: var(--muted-foreground); font-weight: 600;">No designations yet.</td></tr>';
                return;
            }
            if (allAccessGrantableCategories.length === 0) {
                tbody.innerHTML = '<tr><td colspan="2" style="text-align: center; color: var(--muted-foreground); font-weight: 600;">No categories yet.</td></tr>';
                return;
            }

            tbody.innerHTML = allNonSystemDesignations.map(d => `
                <tr>
                    <td style="font-weight:700;"><span class="truncate" style="max-width:280px;" title="${truncTitle(d.label)}">${escapeHtml(truncText(d.label, 50))}</span></td>
                    <td>${canManageDesignations
                        ? `<button class="action-select" style="cursor:pointer;" onclick="openManageCategoryAccessModal('${escapeHtml(d.code)}', '${escapeHtml(d.label)}')">Manage Access</button>`
                        : ''}</td>
                </tr>
            `).join('');
        }

        function openManageCategoryAccessModal(designationCode, designationLabel) {
            document.getElementById('manage-category-access-designation-code').value = designationCode;
            document.getElementById('manage-category-access-title').textContent = `Category Access: ${designationLabel}`;

            const designation = allNonSystemDesignations.find(d => d.code === designationCode);
            const hasManageAll = !!(designation && (designation.permission_codes || []).includes('documents.all.manage'));
            const warning = document.getElementById('manage-category-access-warning');
            if (hasManageAll) {
                warning.style.display = 'block';
                warning.textContent = `${designationLabel} already has the "Manage All Documents" permission, which gives it full access to every category. Changing anything below won't have any effect unless that permission is removed.`;
            } else {
                warning.style.display = 'none';
            }

            const byKey = new Map(allCategoryDesignationRows.map(r => [`${r.category}::${r.designation_code}`, r]));
            const checklist = document.getElementById('manage-category-access-checklist');
            checklist.innerHTML = allAccessGrantableCategories.length > 0
                ? allAccessGrantableCategories.map(cat => {
                    const cell = byKey.get(`${cat.code}::${designationCode}`) || { can_read: false, can_manage: false };
                    // PUBLIC is readable by every authenticated user regardless of this
                    // matrix (see document access resolution) — Read is always on and
                    // not something to toggle off; only Manage (upload/sign/delete) is
                    // a real choice here.
                    const isPublic = cat.code === 'PUBLIC';
                    return `
                        <div style="display:flex; align-items:center; padding:8px; border-radius:6px;">
                            <div style="flex:1; font-size:14px; word-break:break-word;">${escapeHtml(cat.label)}</div>
                            <div style="width:60px; text-align:center;">
                                <input type="checkbox" class="category-access-checkbox" data-category="${escapeHtml(cat.code)}" data-field="can_read" style="width:20px; height:20px;" ${(isPublic || cell.can_read) ? 'checked' : ''} ${isPublic ? 'disabled' : ''}>
                            </div>
                            <div style="width:70px; text-align:center;">
                                <input type="checkbox" class="category-access-checkbox" data-category="${escapeHtml(cat.code)}" data-field="can_manage" style="width:20px; height:20px;" ${cell.can_manage ? 'checked' : ''}>
                            </div>
                        </div>
                    `;
                }).join('')
                : '<span style="color:var(--muted-foreground); font-size:12px;">No categories exist yet.</span>';

            document.getElementById('manage-category-access-feedback').innerHTML = '';
            document.getElementById('manage-category-access-modal').style.display = 'flex';
        }

        function closeManageCategoryAccessModal() {
            document.getElementById('manage-category-access-modal').style.display = 'none';
        }

        async function saveManageCategoryAccess() {
            const feedback = document.getElementById('manage-category-access-feedback');
            const saveBtn = document.getElementById('manage-category-access-save-btn');
            const designationCode = document.getElementById('manage-category-access-designation-code').value;
            const byKey = new Map(allCategoryDesignationRows.map(r => [`${r.category}::${r.designation_code}`, r]));

            const changes = [];
            allAccessGrantableCategories.forEach(cat => {
                const cell = byKey.get(`${cat.code}::${designationCode}`) || { can_read: false, can_manage: false };
                const readCb = document.querySelector(`.category-access-checkbox[data-category="${cat.code}"][data-field="can_read"]`);
                const manageCb = document.querySelector(`.category-access-checkbox[data-category="${cat.code}"][data-field="can_manage"]`);
                if (readCb.checked !== !!cell.can_read || manageCb.checked !== !!cell.can_manage) {
                    changes.push({ category: cat.code, can_read: readCb.checked, can_manage: manageCb.checked });
                }
            });

            if (changes.length === 0) {
                feedback.className = 'feedback-box';
                feedback.innerHTML = 'No changes to save.';
                return;
            }

            saveBtn.disabled = true;
            saveBtn.textContent = 'Saving...';
            try {
                await Promise.all(changes.map(c => fetch(`${API_BASE}/api/v1/admin/roles/document-category-designations/toggle`, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json', 'Authorization': `Bearer ${token}` },
                    body: JSON.stringify({ category: c.category, designation_code: designationCode, can_read: c.can_read, can_manage: c.can_manage })
                })));
                await loadCategoryMatrix();
                closeManageCategoryAccessModal();
            } catch (err) {
                feedback.className = 'feedback-box error';
                feedback.innerHTML = `<span class="icon">!</span> Failed to save changes.`;
            } finally {
                saveBtn.disabled = false;
                saveBtn.textContent = 'Save Changes';
            }
        }

        async function deleteDocumentCategory(code) {
            if (!(await showConfirm(`Delete category "${code}"? This only works if no documents use it.`, 'Delete'))) return false;
            try {
                const resp = await fetch(`${API_BASE}/api/v1/admin/roles/document-categories/delete`, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json', 'Authorization': `Bearer ${token}` },
                    body: JSON.stringify({ code })
                });
                const data = await resp.json();
                if (resp.ok) {
                    await loadNonSystemCategories();
                    renderCategoriesList();
                    populateSubcategoryParentSelect();
                    renderSubcategoriesList();
                    await loadCategoryMatrix();
                    return true;
                } else {
                    alert(data.message || 'Failed to delete category.');
                    return false;
                }
            } catch (err) {
                alert('Connection error while deleting category.');
                return false;
            }
        }

        async function loadPermissionsCatalog() {
            try {
                const resp = await fetch(`${API_BASE}/api/v1/admin/roles/permissions`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}` }
                });
                if (resp.ok) {
                    allPermissions = sortPermissionsForDisplay(await resp.json());
                }
            } catch (err) {
                console.error('Failed to load permissions catalog:', err);
            }
        }

        function renderDesignationPermissionsMatrix() {
            const headerRow = document.getElementById('designation-permissions-header');
            const tbody = document.getElementById('designation-permissions-rows-target');

            headerRow.innerHTML = '<th style="position:sticky; left:0; z-index:2; background-color:var(--background); box-shadow: 2px 0 4px rgba(0,0,0,0.06);">Designation</th>' +
                allPermissions.map(p => {
                    const info = PERMISSION_INFO[p.code];
                    const title = (info && info.title) || p.label;
                    return `<th style="white-space:nowrap;">
                        <span>${escapeHtml(title)}</span>
                        <button type="button" class="perm-info-btn" onclick="showPermInfo('${escapeHtml(p.code)}')" aria-label="What does ${escapeHtml(title)} do?">i</button>
                    </th>`;
                }).join('');

            if (allNonSystemDesignations.length === 0) {
                tbody.innerHTML = `<tr><td colspan="${allPermissions.length + 1}" style="text-align: center; color: var(--muted-foreground); font-weight: 600;">No designations yet.</td></tr>`;
                return;
            }

            tbody.innerHTML = allNonSystemDesignations.map(d => {
                // A caller can never edit the permissions of a designation they themselves
                // hold — self-escalation guard, enforced server-side too (see toggle_designation_permission).
                const isOwn = !!(user && (user.designations || []).includes(d.code));
                return `
                <tr>
                    <td style="font-weight:700; position:sticky; left:0; z-index:1; background-color:var(--background); box-shadow: 2px 0 4px rgba(0,0,0,0.06); max-width:180px;">
                        <span class="truncate" style="max-width:150px; display:inline-block; vertical-align:middle;" title="${truncTitle(d.label)}">${escapeHtml(truncText(d.label, 24))}</span>
                        ${isOwn ? '<span title="You hold this designation — only another admin can change its permissions." style="color:var(--muted-foreground); font-size:11px; font-weight:600; margin-left:6px;">(yours)</span>' : ''}
                    </td>
                    ${allPermissions.map(p => `
                        <td style="text-align:center;">
                            <input type="checkbox" class="perm-matrix-checkbox" data-designation="${escapeHtml(d.code)}" data-permission="${escapeHtml(p.code)}"
                                onmousedown="event.preventDefault()"
                                ${isOwn ? 'disabled' : ''}
                                ${(d.permission_codes || []).includes(p.code) ? 'checked' : ''}>
                        </td>
                    `).join('')}
                </tr>
            `;
            }).join('');
        }

        async function saveDesignationPermissions() {
            const feedback = document.getElementById('designation-permissions-feedback');
            const saveBtn = document.getElementById('save-designation-permissions-btn');
            const changes = [];
            document.querySelectorAll('.perm-matrix-checkbox').forEach(cb => {
                const designationCode = cb.dataset.designation;
                const permissionCode = cb.dataset.permission;
                const designation = allNonSystemDesignations.find(d => d.code === designationCode);
                const wasEnabled = !!(designation && (designation.permission_codes || []).includes(permissionCode));
                if (cb.checked !== wasEnabled) {
                    changes.push({ designationCode, permissionCode, enabled: cb.checked });
                }
            });

            if (changes.length === 0) {
                feedback.style.display = 'block';
                feedback.className = 'feedback-box';
                feedback.innerHTML = 'No changes to save.';
                return;
            }

            saveBtn.disabled = true;
            saveBtn.textContent = 'Saving...';
            try {
                await Promise.all(changes.map(c => fetch(`${API_BASE}/api/v1/admin/roles/designations/permissions/toggle`, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json', 'Authorization': `Bearer ${token}` },
                    body: JSON.stringify({ designation_code: c.designationCode, permission_code: c.permissionCode, enabled: c.enabled })
                })));
                await loadNonSystemDesignations();
                renderDesignationPermissionsMatrix();
                feedback.style.display = 'block';
                feedback.className = 'feedback-box success';
                feedback.innerHTML = `<span class="icon">✓</span> Saved ${changes.length} change(s).`;
            } catch (err) {
                feedback.style.display = 'block';
                feedback.className = 'feedback-box error';
                feedback.innerHTML = `<span class="icon">!</span> Failed to save changes.`;
                await loadNonSystemDesignations();
                renderDesignationPermissionsMatrix();
            } finally {
                saveBtn.disabled = false;
                saveBtn.textContent = 'Save Changes';
            }
        }

        async function submitCreateDesignation() {
            const feedback = document.getElementById('create-designation-feedback');
            const labelInput = document.getElementById('new-designation-label');
            const label = labelInput.value.trim();
            const code = label.toLowerCase().replace(/\s+/g, '_');

            feedback.style.display = 'none';
            if (!label) return;

            try {
                const resp = await fetch(`${API_BASE}/api/v1/admin/roles/designations/create`, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json', 'Authorization': `Bearer ${token}` },
                    body: JSON.stringify({ code, label })
                });
                const data = await resp.json();
                feedback.style.display = 'block';
                if (resp.ok) {
                    feedback.className = 'feedback-box success';
                    feedback.innerHTML = `<span class="icon">✓</span> Designation "${escapeHtml(label)}" created.`;
                    labelInput.value = '';
                    await loadNonSystemDesignations();
                    renderDesignationsList();
                    renderDesignationPermissionsMatrix();
                    await loadCategoryMatrix();
                } else {
                    feedback.className = 'feedback-box error';
                    feedback.innerHTML = `<span class="icon">!</span> ${escapeHtml(data.message || 'Failed to create designation.')}`;
                }
            } catch (err) {
                feedback.style.display = 'block';
                feedback.className = 'feedback-box error';
                feedback.innerHTML = `<span class="icon">!</span> Network error communicating with server.`;
            }
        }

        // ==========================================
        // DOCUMENT CONTROL ROOM ACTIONS
        // ==========================================

        let allAdminDocs = [];
        let subbarCategoryFilter = 'ALL';
        let subbarSubcategoryFilter = 'ALL';

        async function loadAdminDocuments() {
            try {
                const response = await fetch(`${API_BASE}/api/v1/admin/documents/list`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}` }
                });

                if (response.ok) {
                    allAdminDocs = await response.json();
                    renderAdminCategoryFilterChips();
                    filterAdminDocs();
                } else {
                    document.getElementById('admin-doc-rows-target').innerHTML = `<tr><td colspan="3" style="text-align: center; color: var(--foreground);">Failed to fetch database files.</td></tr>`;
                }
            } catch (err) {
                document.getElementById('admin-doc-rows-target').innerHTML = '<tr><td colspan="3" style="text-align: center; color: var(--foreground);">Database connection offline.</td></tr>';
            }
        }

        function renderAdminCategoryFilterChips() {
            const bar = document.getElementById('admin-filter-bar');
            const categories = [...new Set(allAdminDocs.map(d => d.document.category))];

            bar.innerHTML = '<button class="filter-chip" id="admin-filter-all" onclick="filterAdminDocsBySubbar(\'ALL\')">All</button>' +
                categories.map(cat => {
                    const chipId = 'admin-filter-' + cat.toLowerCase().replace(/_/g, '-');
                    return `<button class="filter-chip" id="${chipId}" onclick="filterAdminDocsBySubbar('${cat}')">${escapeHtml(formatCategory(cat))}</button>`;
                }).join('') +
                '<button class="filter-chip" id="admin-filter-deleted" onclick="filterAdminDocsBySubbar(\'DELETED\')">Deleted Files</button>';

            if (subbarCategoryFilter !== 'ALL' && subbarCategoryFilter !== 'DELETED' && !categories.includes(subbarCategoryFilter)) {
                subbarCategoryFilter = 'ALL';
            }

            const activeChipId = subbarCategoryFilter === 'DELETED'
                ? 'admin-filter-deleted'
                : 'admin-filter-' + subbarCategoryFilter.toLowerCase().replace(/_/g, '-');
            const activeChip = document.getElementById(activeChipId);
            if (activeChip) activeChip.classList.add('active');

            renderAdminSubcategoryFilterChips();
        }

        function renderAdminSubcategoryFilterChips() {
            const bar = document.getElementById('admin-subcategory-filter-bar');
            if (subbarCategoryFilter === 'ALL' || subbarCategoryFilter === 'DELETED') {
                bar.style.display = 'none';
                bar.innerHTML = '';
                return;
            }

            const docsInCategory = allAdminDocs.filter(d => d.document.category === subbarCategoryFilter);
            const subcats = [...new Map(
                docsInCategory
                    .filter(d => d.metadata && d.metadata.subcategory_code)
                    .map(d => [d.metadata.subcategory_code, d.metadata.subcategory_label])
            ).entries()];

            if (subcats.length === 0) {
                bar.style.display = 'none';
                bar.innerHTML = '';
                subbarSubcategoryFilter = 'ALL';
                return;
            }

            bar.style.display = 'flex';
            bar.innerHTML = '<button class="sub-filter-chip active" id="admin-subfilter-all" onclick="filterAdminDocsBySubcategory(\'ALL\')">All</button>' +
                subcats.map(([code, label]) => {
                    const chipId = 'admin-subfilter-' + code.toLowerCase().replace(/_/g, '-');
                    return `<button class="sub-filter-chip" id="${chipId}" onclick="filterAdminDocsBySubcategory('${code}')">${escapeHtml(label)}</button>`;
                }).join('');

            if (subbarSubcategoryFilter !== 'ALL' && !subcats.some(([code]) => code === subbarSubcategoryFilter)) {
                subbarSubcategoryFilter = 'ALL';
            }
        }

        function filterAdminDocsBySubbar(category) {
            subbarCategoryFilter = category;
            subbarSubcategoryFilter = 'ALL';

            // Update active state of filter chips
            document.querySelectorAll('#admin-filter-bar .filter-chip').forEach(el => el.classList.remove('active'));
            const chipId = 'admin-filter-' + category.toLowerCase().replace(/_/g, '-');
            const target = document.getElementById(chipId);
            if (target) target.classList.add('active');

            renderAdminSubcategoryFilterChips();
            filterAdminDocs();
        }

        function filterAdminDocsBySubcategory(code) {
            subbarSubcategoryFilter = code;

            document.querySelectorAll('#admin-subcategory-filter-bar .sub-filter-chip').forEach(el => el.classList.remove('active'));
            const chipId = 'admin-subfilter-' + code.toLowerCase().replace(/_/g, '-');
            const target = document.getElementById(chipId);
            if (target) target.classList.add('active');

            filterAdminDocs();
        }

        function filterAdminDocs() {
            const showingDeleted = subbarCategoryFilter === 'DELETED';

            const filtered = allAdminDocs.filter(d => {
                const doc = d.document;
                if (showingDeleted) return doc.is_deleted;

                const matchesCat = (subbarCategoryFilter === 'ALL' || doc.category === subbarCategoryFilter);
                const matchesSubcat = (subbarSubcategoryFilter === 'ALL' ||
                    (d.metadata && d.metadata.subcategory_code === subbarSubcategoryFilter));
                return matchesCat && matchesSubcat && !doc.is_deleted;
            });

            renderAdminDocsTable(filtered);
        }

        function getCategoryBadgeClass(cat) {
            switch(cat) {
                case 'RESIDENT_SPECIFIC': return 'cat-resident';
                case 'PUBLIC': return 'cat-generic';
                default: return 'cat-operational';
            }
        }

        function formatCategory(cat) {
            switch(cat) {
                case 'RESIDENT_SPECIFIC': return 'Resident Specific';
                case 'PUBLIC': return 'Public';
                default: return cat.toLowerCase().split('_').map(w => w.charAt(0).toUpperCase() + w.slice(1)).join(' ');
            }
        }

        function renderAdminDocsTable(docsList) {
            const tbody = document.getElementById('admin-doc-rows-target');
            tbody.innerHTML = '';
            
            if (docsList.length === 0) {
                tbody.innerHTML = '<tr><td colspan="3" style="text-align: center; color: var(--muted-foreground); padding: 32px; font-weight: 500;">No documents found.</td></tr>';
                return;
            }

            docsList.forEach(docWithMeta => {
                const doc = docWithMeta.document;
                const meta = docWithMeta.metadata;
                const flat = docWithMeta.flat;
                const tr = document.createElement('tr');

                let badgeHtml = doc.is_deleted
                    ? '<span class="badge deleted">Deleted</span>'
                    : '';

                const subcategoryChip = (meta && meta.subcategory_label)
                    ? `<span style="display:inline-block; margin-top:6px; padding:3px 9px; font-size:11px; font-weight:600; border-radius:999px; background:var(--background); border:1px solid var(--border); color:var(--foreground);">${escapeHtml(meta.subcategory_label)}</span>`
                    : '';

                const flatHtml = flat
                    ? `<div style="font-size:11px; color:var(--muted-foreground); margin-top:6px;">Flat: <strong>${escapeHtml(flat.wing)}-${escapeHtml(flat.flat_number)}</strong></div>`
                    : '';

                tr.innerHTML = `
                    <td>
                        <div style="display:flex; align-items:center; gap:8px; max-width:480px;">
                            <div class="truncate" style="font-weight: 700; color: var(--primary); cursor: pointer; font-size: 1.125rem;" title="${truncTitle(doc.title)}" onclick="downloadDocument('${escapeHtml(doc.document_id)}', true)">${escapeHtml(truncText(doc.title, 70))}</div>
                            ${badgeHtml}
                        </div>
                        <div class="truncate-2-lines" style="font-size:13px; color:var(--muted-foreground); margin-top:4px; max-width: 480px; line-height: 1.45;" title="${truncTitle(doc.description)}">
                            ${escapeHtml(doc.description) || 'No description...'}
                        </div>
                    </td>
                    <td>
                        <span class="doc-category-badge ${getCategoryBadgeClass(doc.category)}">${formatCategory(doc.category)}</span>
                        ${subcategoryChip}
                        ${flatHtml}
                    </td>
                    <td>
                        <div style="display: flex; gap: 8px; align-items: center; flex-wrap: wrap;">
                            ${!doc.is_deleted
                                ? `<button class="action-select" style="cursor:pointer;" onclick="openEditDocumentModal('${escapeHtml(doc.document_id)}')">Edit</button>`
                                : '<span style="color:var(--muted-foreground); font-style:italic; font-size: 12px;">Deleted</span>'}
                        </div>
                    </td>
                `;
                tbody.appendChild(tr);
            });
        }

        async function downloadDocument(docId, isAdmin = false) {
            const url = isAdmin 
                ? `${API_BASE}/api/v1/admin/documents/${docId}/get` 
                : `${API_BASE}/api/v1/documents/${docId}`;
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
                        alert('No download link returned by server.');
                    }
                } else {
                    const errText = await readErrorMessage(response);
                    alert(`Failed to get download link: ${errText}`);
                }
            } catch (err) {
                console.error(err);
                alert('Connection error while fetching download link.');
            }
        }

        async function deleteDocument(docId) {
            if (!(await showConfirm('Are you sure you want to delete this document? Regular residents will lose access immediately.', 'Delete'))) return false;
            try {
                const response = await fetch(`${API_BASE}/api/v1/admin/documents/${docId}/delete`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}` }
                });

                if (response.ok) {
                    loadAdminDocuments();
                    return true;
                } else {
                    alert('Failed to delete document.');
                    return false;
                }
            } catch (e) {
                alert('Connection failure deleting document.');
                return false;
            }
        }

        async function deleteDocumentFromEditModal() {
            const docId = document.getElementById('edit-doc-id').value;
            const deleted = await deleteDocument(docId);
            if (deleted) closeEditDocumentModal();
        }

        function openEditDocumentModal(docId) {
            const docWithMeta = allAdminDocs.find(d => d.document.document_id === docId);
            if (!docWithMeta) return;
            const doc = docWithMeta.document;
            const meta = docWithMeta.metadata;

            document.getElementById('edit-doc-id').value = docId;
            document.getElementById('edit-doc-original-category').value = doc.category;
            document.getElementById('edit-doc-title').value = doc.title;
            document.getElementById('edit-doc-desc').value = doc.description || '';

            const categorySelect = document.getElementById('edit-doc-category');
            if (doc.category === 'RESIDENT_SPECIFIC') {
                // Flat-mapped documents can't have their category changed here — that would
                // require re-mapping flats, which this modal doesn't handle.
                categorySelect.innerHTML = `<option value="RESIDENT_SPECIFIC">${escapeHtml(formatCategory('RESIDENT_SPECIFIC'))}</option>`;
                categorySelect.disabled = true;
            } else {
                categorySelect.disabled = false;
                categorySelect.innerHTML = uploadFormCategories
                    .filter(c => c.code !== 'RESIDENT_SPECIFIC')
                    .map(c => `<option value="${escapeHtml(c.code)}">${escapeHtml(formatCategory(c.code))}</option>`)
                    .join('');
                categorySelect.value = doc.category;
            }

            document.getElementById('edit-doc-tags').value = meta ? meta.tags.join(', ') : '';
            document.getElementById('edit-doc-date').value = meta ? meta.document_date : '';
            document.getElementById('edit-doc-valid-from').value = (meta && meta.valid_from) || '';
            document.getElementById('edit-doc-valid-until').value = (meta && meta.valid_until) || '';

            handleEditCategoryChange();
            if (meta && meta.subcategory_code) {
                document.getElementById('edit-doc-subcategory').value = meta.subcategory_code;
            }

            document.getElementById('edit-doc-feedback').style.display = 'none';
            document.getElementById('edit-document-modal').style.display = 'flex';
        }

        function closeEditDocumentModal() {
            document.getElementById('edit-document-modal').style.display = 'none';
        }

        function handleEditCategoryChange() {
            const category = document.getElementById('edit-doc-category').value;
            const originalCategory = document.getElementById('edit-doc-original-category').value;

            document.getElementById('edit-doc-category-warning').style.display =
                (category !== originalCategory) ? 'block' : 'none';

            const subcatSelect = document.getElementById('edit-doc-subcategory');
            const selectedCat = uploadFormCategories.find(c => c.code === category);
            const matching = selectedCat ? selectedCat.subcategories : [];
            subcatSelect.innerHTML = '<option value="">None</option>' +
                matching.map(s => `<option value="${escapeHtml(s.code)}">${escapeHtml(s.label)}</option>`).join('');
        }

        async function submitEditDocument() {
            const feedback = document.getElementById('edit-doc-feedback');
            const saveBtn = document.getElementById('edit-doc-save-btn');
            feedback.style.display = 'none';

            const docId = document.getElementById('edit-doc-id').value;
            const title = document.getElementById('edit-doc-title').value.trim();
            const description = document.getElementById('edit-doc-desc').value.trim() || null;
            const category = document.getElementById('edit-doc-category').value;
            const subcategory_code = document.getElementById('edit-doc-subcategory').value || null;
            const tagsInput = document.getElementById('edit-doc-tags').value.trim();
            const tags = tagsInput ? tagsInput.split(',').map(t => t.trim()) : [];
            const document_date = document.getElementById('edit-doc-date').value || null;
            const valid_from = document.getElementById('edit-doc-valid-from').value || null;
            const valid_until = document.getElementById('edit-doc-valid-until').value || null;
            const hasMetadata = subcategory_code || tags.length > 0 || document_date || valid_from || valid_until;
            const today = new Date().toISOString().slice(0, 10);

            if (!title) {
                feedback.className = 'feedback-box error';
                feedback.innerHTML = '<span class="icon">!</span> Title is required.';
                feedback.style.display = 'block';
                return;
            }

            const payload = {
                title,
                description,
                category,
                metadata: hasMetadata ? {
                    subcategory_code,
                    tags,
                    document_date: document_date || today,
                    valid_from,
                    valid_until
                } : null
            };

            saveBtn.disabled = true;
            saveBtn.textContent = 'Saving...';
            try {
                const resp = await fetch(`${API_BASE}/api/v1/admin/documents/${docId}/update`, {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json', 'Authorization': `Bearer ${token}` },
                    body: JSON.stringify(payload)
                });
                if (resp.ok) {
                    closeEditDocumentModal();
                    await loadAdminDocuments();
                } else {
                    feedback.className = 'feedback-box error';
                    feedback.innerHTML = `<span class="icon">!</span> ${escapeHtml(await readErrorMessage(resp))}`;
                    feedback.style.display = 'block';
                }
            } catch (err) {
                feedback.className = 'feedback-box error';
                feedback.innerHTML = '<span class="icon">!</span> Network error communicating with server.';
                feedback.style.display = 'block';
            } finally {
                saveBtn.disabled = false;
                saveBtn.textContent = 'Save';
            }
        }

        // Form Submission to Create/Publish Document
        const regDocForm = document.getElementById('register-doc-form');
        const docFeedback = document.getElementById('doc-feedback');

        regDocForm.addEventListener('submit', async (e) => {
            e.preventDefault();
            docFeedback.style.display = 'none';

            const title = document.getElementById('doc-title').value.trim();
            const description = document.getElementById('doc-desc').value.trim() || null;
            const category = document.getElementById('doc-category').value;

            // Collect flat mapping for RESIDENT_SPECIFIC documents
            let flat = null;
            if (category === 'RESIDENT_SPECIFIC') {
                const wing = document.getElementById('doc-flat-wing').value.trim().toUpperCase();
                const flat_number = document.getElementById('doc-flat-number').value.trim();
                if (!wing || !flat_number) {
                    docFeedback.textContent = 'Resident-specific documents require a flat mapping (wing + flat number).';
                    docFeedback.className = 'feedback-box error';
                    docFeedback.style.display = 'block';
                    return;
                }
                flat = { wing, flat_number };
            }

            // Optional Metadata fields. document_date is required by the backend whenever
            // metadata is sent at all — default it to today so picking a subcategory (or
            // tags, or a validity range) without also touching the Date field doesn't
            // silently drop everything (metadata used to be gated on document_date alone).
            const subcategory_code = document.getElementById('doc-subcategory').value || null;
            const tagsInput = document.getElementById('doc-tags').value.trim();
            const tags = tagsInput ? tagsInput.split(',').map(t => t.trim()) : [];
            const document_date = document.getElementById('doc-date').value || null;
            const valid_from = document.getElementById('doc-valid-from').value || null;
            const valid_until = document.getElementById('doc-valid-until').value || null;
            const hasMetadata = subcategory_code || tags.length > 0 || document_date || valid_from || valid_until;
            const today = new Date().toISOString().slice(0, 10);

            // Formulate payload
            const payload = {
                title,
                description,
                category,
                flat,
                metadata: hasMetadata ? {
                    subcategory_code,
                    tags,
                    document_date: document_date || today,
                    valid_from,
                    valid_until
                } : null
            };

            const fileInput = document.getElementById('doc-file');
            if (!fileInput.files || fileInput.files.length === 0) {
                docFeedback.textContent = 'Please select a file to upload.';
                docFeedback.className = 'feedback-box error';
                docFeedback.style.display = 'block';
                return;
            }
            const file = fileInput.files[0];

            // Validate file size (Max 1GB)
            const MAX_SIZE = 1024 * 1024 * 1024; // 1GB
            if (file.size > MAX_SIZE) {
                docFeedback.textContent = 'File size must be 1GB or less.';
                docFeedback.className = 'feedback-box error';
                docFeedback.style.display = 'block';
                return;
            }

            // Validate file type based on extension
            const allowedExtensions = ['.pdf', '.doc', '.docx', '.xls', '.xlsx', '.txt', '.jpg', '.jpeg', '.png'];
            const fileName = file.name.toLowerCase();
            const isValidExtension = allowedExtensions.some(ext => fileName.endsWith(ext));
            if (!isValidExtension) {
                docFeedback.textContent = 'Invalid file type. Only PDF, Word, Excel, Text, and image (JPG/PNG) files are allowed.';
                docFeedback.className = 'feedback-box error';
                docFeedback.style.display = 'block';
                return;
            }

            try {
                // Step 1: Get presigned PUT URL
                const urlRes = await fetch(`${API_BASE}/api/v1/admin/documents/upload-url`, {
                    method: 'POST',
                    headers: {
                        'Authorization': `Bearer ${token}`,
                        'Content-Type': 'application/json'
                    },
                    body: JSON.stringify({
                        ...payload,
                        filename: file.name,
                        content_type: file.type || 'application/octet-stream'
                    })
                });

                if (!urlRes.ok) {
                    const errText = await readErrorMessage(urlRes);
                    docFeedback.textContent = `Error: ${errText}`;
                    docFeedback.className = 'feedback-box error';
                    docFeedback.style.display = 'block';
                    return;
                }

                const { document_id, upload_url } = await urlRes.json();

                // Step 2: PUT file directly to S3
                docFeedback.textContent = 'Uploading file...';
                docFeedback.className = 'feedback-box';
                docFeedback.style.display = 'block';

                const putRes = await fetch(upload_url, {
                    method: 'PUT',
                    headers: { 'Content-Type': file.type || 'application/octet-stream' },
                    body: file
                });

                if (!putRes.ok) {
                    docFeedback.textContent = 'Upload to storage failed. Please try again.';
                    docFeedback.className = 'feedback-box error';
                    return;
                }

                // Step 3: Confirm upload
                docFeedback.textContent = 'Confirming upload...';
                
                const confirmRes = await fetch(`${API_BASE}/api/v1/admin/documents/confirm-upload`, {
                    method: 'POST',
                    headers: {
                        'Authorization': `Bearer ${token}`,
                        'Content-Type': 'application/json'
                    },
                    body: JSON.stringify({ document_id })
                });

                if (!confirmRes.ok) {
                    const errText = await readErrorMessage(confirmRes);
                    docFeedback.textContent = `Confirmation failed: ${errText}`;
                    docFeedback.className = 'feedback-box error';
                    return;
                }

                docFeedback.textContent = '✓ Document added!';
                docFeedback.className = 'feedback-box success';
                regDocForm.reset();
                handleCategoryChange();
                loadAdminDocuments();
                
            } catch (err) {
                docFeedback.textContent = 'Connection error.';
                docFeedback.className = 'feedback-box error';
                docFeedback.style.display = 'block';
            }
        });

        // Initialize Resident table initially
        loadUsers();
    </script>
</body>
</html>
"#;
