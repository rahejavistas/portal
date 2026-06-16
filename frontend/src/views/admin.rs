pub const ADMIN_HTML: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Vistas Portal - Admin Suite</title>
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
            padding: 0;
            min-height: 100vh;
            overflow-x: hidden;
        }

        /* Layout Container */
        .layout-container {
            display: flex;
            width: 100%;
            min-height: 100vh;
            position: relative;
        }

        /* Decorative Background Shapes */
        .bg-shape-top-right {
            position: absolute;
            width: 600px;
            height: 600px;
            border-radius: 50%;
            background: radial-gradient(circle, rgba(59, 130, 246, 0.04) 0%, rgba(59, 130, 246, 0) 70%);
            top: -150px;
            right: -150px;
            z-index: 1;
            pointer-events: none;
        }

        /* Left Main Sidebar */
        .sidebar {
            width: 260px;
            border-right: 1px solid var(--border);
            display: flex;
            flex-direction: column;
            padding: 40px 24px;
            justify-content: space-between;
            position: fixed;
            height: 100vh;
            left: 0;
            top: 0;
            z-index: 100;
            background-color: var(--background);
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

        @media (max-width: 992px) {
            .layout-container {
                flex-direction: column;
            }
            .sidebar {
                width: 100%;
                height: auto;
                position: relative;
                border-right: none;
                border-bottom: 1px solid var(--border);
                padding: 24px;
            }
            .sidebar-brand {
                display: flex;
                flex-direction: column;
                align-items: center;
                text-align: center;
            }
            .sidebar-brand p {
                margin-bottom: 20px;
            }
            .nav-menu {
                flex-direction: row;
                justify-content: center;
                width: 100%;
                gap: 8px;
            }
            .nav-item {
                flex: 1;
                text-align: center;
                padding: 10px;
            }
            .workspace {
                margin-left: 0 !important;
                padding: 40px 24px;
                width: 100%;
            }
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
        .setup-link-alert {
            margin-top: 20px;
            background: #EFF6FF;
            border: 2px dashed var(--primary);
            padding: 16px;
            border-radius: var(--radius-lg);
        }
        .link-text {
            word-break: break-all;
            background-color: var(--background);
            padding: 8px 12px;
            border: 1px solid var(--border);
            border-radius: var(--radius-md);
            font-family: monospace;
            font-size: 12px;
            margin-top: 8px;
            color: var(--primary);
            font-weight: 600;
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
    </style>
</head>
<body>
    <div class="bg-shape-top-right"></div>

    <div class="layout-container">
        <!-- Sidebar Navigation -->
        <aside class="sidebar">
            <div class="sidebar-brand">
                <h1>Admin</h1>
                <p>Raheja Vistas Admin Room</p>
                
                <nav class="nav-menu">
                    <button class="nav-item active" id="suite-btn-residents" onclick="switchSuite('residents')">
                        Residents
                    </button>
                    <button class="nav-item" id="suite-btn-documents" onclick="switchSuite('documents')">
                        Documents
                    </button>
                    <a href="/process" class="nav-item" id="suite-btn-workflows" style="text-decoration: none;">
                        Processes
                    </a>
                </nav>
            </div>

            <div>
                <button class="logout-btn" id="logout-btn">Log Out</button>
            </div>
        </aside>



        <!-- Main Workspace Contents -->
        <main class="workspace" id="main-workspace">
            
            <!-- RESIDENTS SUITE PANEL -->
            <section class="suite-panel active" id="suite-residents">
                <h2>Residents</h2>
                
                <div class="split-grid">
                    <!-- Left: Add Form -->
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

                        <div class="setup-link-alert" id="link-alert" style="display: none;">
                            <strong style="color: var(--foreground); font-size: 13px;">Resident added!</strong>
                            <div style="font-size: 12px; color: var(--muted-foreground); margin-top: 5px; line-height: 1.4;">
                                Password setup link:
                            </div>
                            <div class="link-text" id="new-setup-link"></div>
                        </div>
                    </div>

                    <!-- Right: Resident list -->
                    <div class="card">
                        <h3 style="margin-top:0; font-size:20px; font-weight:800; margin-bottom: 24px;">Registry</h3>
                        <div style="margin-bottom: 24px;">
                            <input type="text" id="search-input" placeholder="Search residents...">
                        </div>
                        <div class="table-container">
                            <table>
                                <thead>
                                    <tr>
                                        <th>Resident</th>
                                        <th>Role</th>
                                        <th>Update Role</th>
                                    </tr>
                                </thead>
                                <tbody id="user-rows-target">
                                    <tr>
                                        <td colspan="3" style="text-align: center; color: var(--muted-foreground); font-weight: 600;">Loading registry...</td>
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
                </div>

                <!-- Subtab 1: Document Database Explorer -->
                <div class="subtab-panel active" id="subtab-panel-db">
                    
                    <!-- Category Filter Bar (Desktop & Mobile) -->
                    <div class="category-filter-bar" id="admin-filter-bar">
                        <button class="filter-chip active" id="admin-filter-all" onclick="filterAdminDocsBySubbar('ALL')">All</button>
                        <button class="filter-chip" id="admin-filter-resident-specific" onclick="filterAdminDocsBySubbar('RESIDENT_SPECIFIC')">Resident Specific</button>
                        <button class="filter-chip" id="admin-filter-generic" onclick="filterAdminDocsBySubbar('GENERIC')">Generic</button>
                        <button class="filter-chip" id="admin-filter-meeting-minutes" onclick="filterAdminDocsBySubbar('MEETING_MINUTES')">Minutes</button>
                        <button class="filter-chip" id="admin-filter-operational" onclick="filterAdminDocsBySubbar('OPERATIONAL')">Operational</button>
                    </div>

                    <!-- Sub-Category Filter Bar (Dynamic) -->
                    <div class="category-filter-bar" id="admin-sub-filter-bar" style="display: none; gap: 8px; margin-top: -12px; margin-bottom: 24px; padding-bottom: 4px;">
                        <!-- Dynamically populated sub-category chips -->
                    </div>

                    <div class="card">
                        <h3 style="margin-top:0; font-size:20px; font-weight:800; margin-bottom: 24px;">Explorer</h3>
                        
                        <!-- Filters for DB Explorer (Secondary Row) -->
                        <div style="display: flex; gap: 15px; margin-bottom: 24px; flex-wrap: wrap;">
                            <div style="flex: 1; min-width: 150px;">
                                <label style="font-size: 11px; margin-bottom: 5px;">Access Level</label>
                                <select id="filter-access" onchange="filterAdminDocs()" style="padding: 8px 12px; font-size: 13px;">
                                    <option value="ALL">All Access Levels</option>
                                    <option value="PUBLIC">Public</option>
                                    <option value="COMMITTEE_ONLY">Committee Only</option>
                                    <option value="PRIVATE">Private</option>
                                </select>
                            </div>
                            <div style="flex: 1; min-width: 150px;">
                                <label style="font-size: 11px; margin-bottom: 5px;">Status</label>
                                <select id="filter-status" onchange="filterAdminDocs()" style="padding: 8px 12px; font-size: 13px;">
                                    <option value="ALL">All Statuses</option>
                                    <option value="ACTIVE">Active</option>
                                    <option value="DELETED">Deleted</option>
                                </select>
                            </div>
                        </div>

                        <div class="table-container">
                            <table>
                                <thead>
                                    <tr>
                                        <th>Document</th>
                                        <th>Status</th>
                                        <th>Actions</th>
                                    </tr>
                                </thead>
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
                    <div class="card" style="max-width: 700px;">
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
                                <input type="file" id="doc-file" accept=".pdf,.doc,.docx,.xls,.xlsx,.txt" required style="padding: 8px; width: 100%; background-color: var(--muted);">
                            </div>
                            <div class="form-row-2">
                                <div class="form-group">
                                    <label for="doc-category">Category</label>
                                    <select id="doc-category" required onchange="handleCategoryChange()">
                                        <option value="GENERIC">Generic</option>
                                        <option value="RESIDENT_SPECIFIC">Resident</option>
                                        <option value="MEETING_MINUTES">Meeting Minutes</option>
                                        <option value="OPERATIONAL">Operational</option>
                                    </select>
                                </div>
                                <div class="form-group">
                                    <label for="doc-type">Type</label>
                                    <select id="doc-type" required>
                                        <!-- Dynamically loaded based on category selection -->
                                    </select>
                                </div>
                            </div>

                            <div class="form-row-2">
                                <div class="form-group">
                                    <label for="doc-access">Access Level</label>
                                    <select id="doc-access" required>
                                        <option value="PUBLIC">Public</option>
                                        <option value="COMMITTEE_ONLY">Committee Only</option>
                                        <option value="PRIVATE">Private</option>
                                    </select>
                                </div>
                                <div class="form-group"></div>
                            </div>

                            <!-- Resident Flat (shown only for RESIDENT_SPECIFIC) -->
                            <div id="flat-mapping-section" style="display: none;">
                                <h4 style="margin: 25px 0 15px 0; color: var(--foreground); font-size:14px; text-transform: uppercase; padding-bottom: 8px; font-weight: 800; border-bottom: 1px solid var(--border);">Flat Visibility Mapping</h4>
                                <div class="form-row-2">
                                    <div class="form-group">
                                        <label for="doc-flat-wing">Wing</label>
                                        <input type="text" id="doc-flat-wing" placeholder="e.g. A" maxlength="2">
                                    </div>
                                    <div class="form-group">
                                        <label for="doc-flat-number">Flat Number</label>
                                        <input type="text" id="doc-flat-number" placeholder="e.g. 301">
                                    </div>
                                </div>
                            </div>

                            <!-- Optional Details fields -->
                            <h4 style="margin: 25px 0 15px 0; color: var(--foreground); font-size:14px; text-transform: uppercase; padding-bottom: 8px; font-weight: 800; border-bottom: 1px solid var(--border);">Other Info (Optional)</h4>
                            
                            <div class="form-row-2">
                                <div class="form-group">
                                    <label for="doc-subcategory">Subcategory</label>
                                    <input type="text" id="doc-subcategory" placeholder="e.g. Audit, Wing A, Finance">
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
            </section>
        </main>
    </div>

    <!-- Revoke Flat Modal -->
    <div id="revoke-flat-modal" style="display:none; position:fixed; top:0; left:0; width:100vw; height:100vh; background:rgba(0,0,0,0.5); z-index:9999; justify-content:center; align-items:center;">
        <div style="background:var(--background); padding:24px; border-radius:12px; width:400px; max-width:90%; border:1px solid var(--border); box-shadow: 0 10px 30px rgba(0,0,0,0.5);">
            <h3 style="margin-top:0; color:var(--foreground); font-size:18px;">Revoke Resident Flat</h3>
            <p style="color:var(--muted-foreground); font-size:14px; margin-bottom:20px; line-height:1.5;">Select the flat mapping you want to revoke for this user.</p>
            
            <div class="form-group">
                <label for="revoke-flat-select">Select Flat</label>
                <select id="revoke-flat-select" style="width:100%; padding:10px; background:var(--input-bg); border:1px solid var(--border); color:var(--foreground); border-radius:6px; margin-bottom:24px;"></select>
            </div>
            
            <input type="hidden" id="revoke-flat-user-id">
            <div class="feedback-box" id="revoke-feedback" style="margin-bottom: 15px;"></div>
            
            <div style="display:flex; justify-content:flex-end; gap:12px;">
                <button onclick="closeRevokeFlatModal()" style="padding:10px 16px; background:transparent; border:1px solid var(--border); color:var(--foreground); border-radius:6px; cursor:pointer;">Cancel</button>
                <button onclick="submitRevokeFlat()" class="del-action-btn" style="padding:10px 16px;">Revoke Flat</button>
            </div>
        </div>
    </div>

    <script>
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

        const API_BASE = window.location.port === "4000" ? `http://${window.location.hostname}:3000` : "";
        const token = localStorage.getItem('vfs_token');
        const userStr = localStorage.getItem('vfs_user');
        const user = userStr ? JSON.parse(userStr) : null;
        
        if (!token || !user) {
            localStorage.clear();
            window.location.href = '/login';
        } else {
            if (!user.roles || !user.roles.includes('Admin')) {
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
                        if (!freshUser.roles || !freshUser.roles.includes('Admin')) {
                            window.location.href = '/dashboard';
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

        // Category Type Mapping
        const typeMappings = {
            GENERIC: [
                { value: 'GENERIC_RULEBOOK', text: 'Rulebook' },
                { value: 'GENERIC_BYLAWS', text: 'Bylaws' }
            ],
            RESIDENT_SPECIFIC: [
                { value: 'RESIDENT_KYC', text: 'Resident KYC Details' },
                { value: 'RESIDENT_PROPERTY', text: 'Property Ownership Files' },
                { value: 'RESIDENT_RENTAL', text: 'Rental Agreement Copies' }
            ],
            MEETING_MINUTES: [
                { value: 'MINUTES_AGM', text: 'Annual General Meeting (AGM)' },
                { value: 'MINUTES_GBM', text: 'General Body Meeting (GBM)' },
                { value: 'MINUTES_MGMT', text: 'Management Committee Minutes' }
            ],
            OPERATIONAL: [
                { value: 'OPERATIONAL_VENDOR_CONTRACT', text: 'Vendor Contract Files' },
                { value: 'OPERATIONAL_PAYMENT', text: 'Operations Payment Slips' },
                { value: 'OPERATIONAL_SLA', text: 'Service Level Agreement (SLA)' }
            ]
        };

        // Category → Default Access Level Mapping (server-enforced)
        const categoryDefaultAccess = {
            GENERIC: 'PUBLIC',
            RESIDENT_SPECIFIC: 'PRIVATE',
            MEETING_MINUTES: 'PUBLIC',
            OPERATIONAL: 'PUBLIC'
        };

        function handleCategoryChange() {
            const cat = document.getElementById('doc-category').value;
            const typeSelect = document.getElementById('doc-type');
            typeSelect.innerHTML = '';
            
            if (typeMappings[cat]) {
                typeMappings[cat].forEach(t => {
                    const opt = document.createElement('option');
                    opt.value = t.value;
                    opt.textContent = t.text;
                    typeSelect.appendChild(opt);
                });
            }

            // Auto-set the access level based on category default (user can override)
            const accessSelect = document.getElementById('doc-access');
            if (categoryDefaultAccess[cat]) {
                accessSelect.value = categoryDefaultAccess[cat];
            }

            // Show/hide flat mapping section for RESIDENT_SPECIFIC category
            const flatSection = document.getElementById('flat-mapping-section');
            flatSection.style.display = (cat === 'RESIDENT_SPECIFIC') ? 'block' : 'none';
        }

        // Initialize category dynamic selection
        handleCategoryChange();

        // Suite initialization from URL parameter
        const urlParams = new URLSearchParams(window.location.search);
        const initialTab = urlParams.get('tab');
        if (initialTab === 'documents') {
            switchSuite('documents');
        } else {
            switchSuite('residents');
        }

        // High Level Suite Toggle
        function switchSuite(suiteName) {
            document.querySelectorAll('.nav-item').forEach(item => item.classList.remove('active'));
            
            document.getElementById(`suite-btn-${suiteName}`).classList.add('active');
            
            // Switch panels
            document.getElementById('suite-residents').style.display = suiteName === 'residents' ? 'block' : 'none';
            document.getElementById('suite-documents').style.display = suiteName === 'documents' ? 'block' : 'none';

            if (suiteName === 'documents') {
                loadAdminDocuments();
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
            }
        }

        // Resident Onboarding Handling
        const onboardForm = document.getElementById('onboard-form');
        const feedback = document.getElementById('feedback');
        const linkAlert = document.getElementById('link-alert');
        const newSetupLink = document.getElementById('new-setup-link');

        onboardForm.addEventListener('submit', async (e) => {
            e.preventDefault();
            feedback.style.display = 'none';
            linkAlert.style.display = 'none';

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

                    // Retrieve reset/registration password setup link
                    const resetResp = await fetch(`${API_BASE}/api/v1/forgot-password`, {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ email })
                    });
                    const resetData = await resetResp.json();
                    if (resetResp.ok && resetData.status === 'success') {
                        if (resetData.reset_link) {
                            let finalLink = resetData.reset_link.replace('3000', '4000').replace('localhost:4000', window.location.host);
                            newSetupLink.textContent = finalLink;
                            linkAlert.style.display = 'block';
                        } else {
                            newSetupLink.textContent = 'Check the server logs or email for the setup link.';
                            linkAlert.style.display = 'block';
                        }
                    }

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
                    feedback.innerHTML = `<span class="icon">✓</span> ${data.message}`;
                    setTimeout(() => {
                        closeRevokeFlatModal();
                        loadUsers();
                    }, 1000);
                } else {
                    feedback.className = 'feedback-box error';
                    feedback.innerHTML = `<span class="icon">!</span> ${data.message || 'Failed to revoke flat.'}`;
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
                tbody.innerHTML = '<tr><td colspan="3" style="text-align: center; color: var(--muted-foreground); font-weight: 600;">No residents found.</td></tr>';
                return;
            }

            usersList.forEach(u => {
                const tr = document.createElement('tr');
                const hasAdmin = u.roles.includes('Admin');
                const hasCommittee = u.roles.includes('Committee');
                const isSelf = u.user_id === user.user_id;
                
                let mappingText = '';
                if (hasAdmin || u.wing === 'TBD' || u.flat_number === 'TBD') {
                    mappingText = `<span style="color:var(--foreground); font-style:italic;">Admin Account</span>`;
                } else {
                    const wings = u.wing.split(', ');
                    const flats = u.flat_number.split(', ');
                    const mappings = wings.map((w, i) => `${escapeHtml(w)}-${escapeHtml(flats[i])}`).join(', ');
                    mappingText = `Flats: <strong>${mappings}</strong>`;
                }

                // Only show Revoke Flat if they actually have a flat assigned
                const hasRealFlat = u.wing !== 'TBD' && u.flat_number !== 'TBD';
                const revokeFlatHtml = hasRealFlat 
                    ? `<button class="del-action-btn" style="font-size: 11px; padding: 6px 10px; white-space: nowrap;" onclick="openRevokeFlatModal(${u.user_id}, '${escapeHtml(u.wing)}', '${escapeHtml(u.flat_number)}')">Revoke Flat</button>` 
                    : '';

                // Self-admin: show label instead of dropdown
                const roleControlHtml = isSelf
                    ? `<span style="color:var(--foreground); font-weight:700; font-size:12px;">Admin (You)</span>`
                    : `<select class="action-select" id="role-select-${escapeHtml(u.user_id)}">
                            <option value="resident" ${!hasAdmin && !hasCommittee ? 'selected' : ''}>Set Resident</option>
                            <option value="committee" ${hasCommittee && !hasAdmin ? 'selected' : ''}>Set Committee</option>
                            <option value="admin" ${hasAdmin ? 'selected' : ''}>Set Admin</option>
                       </select>`;

                tr.innerHTML = `
                    <td>
                        <div style="font-weight: 700; color: var(--foreground);">${escapeHtml(u.email)}</div>
                        <div style="font-size: 12px; color: var(--muted-foreground); margin-top: 3px;">${mappingText}</div>
                    </td>
                    <td>
                        ${u.roles.length > 0 
                            ? u.roles.map(r => `<span class="badge ${r.toLowerCase()}">${escapeHtml(r)}</span>`).join('') 
                            : '<span style="color:var(--muted-foreground); font-size:12px;">Resident</span>'}
                    </td>
                    <td>
                        <div style="display: flex; flex-direction: row; align-items: center; justify-content: flex-end; gap: 10px;">
                            ${roleControlHtml}
                            ${revokeFlatHtml}
                        </div>
                    </td>
                `;

                // Only attach change listener for non-self users
                if (!isSelf) {
                    const sel = tr.querySelector(`#role-select-${u.user_id}`);
                    sel.addEventListener('change', async () => {
                        const val = sel.value;
                        const targetRole = val === 'committee' ? 'Committee'
                                         : val === 'admin' ? 'Admin'
                                         : null;
                        await changeRole(u.user_id, targetRole);
                        loadUsers();
                    });
                }

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

        async function changeRole(user_id, target_role) {
            try {
                await fetch(`${API_BASE}/api/v1/admin/users/roles`, {
                    method: 'POST',
                    headers: { 
                        'Content-Type': 'application/json',
                        'Authorization': `Bearer ${token}`
                    },
                    body: JSON.stringify({ user_id, target_role })
                });
            } catch (err) {
                alert('Failed to connect and adjust user role.');
            }
        }

        // ==========================================
        // DOCUMENT CONTROL ROOM ACTIONS
        // ==========================================
        
        let allAdminDocs = [];
        let subbarCategoryFilter = 'ALL';
        let subbarTypeFilter = 'ALL';

        async function loadAdminDocuments() {
            try {
                const response = await fetch(`${API_BASE}/api/v1/admin/documents/list`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}` }
                });

                if (response.ok) {
                    allAdminDocs = await response.json();
                    filterAdminDocs();
                } else {
                    document.getElementById('admin-doc-rows-target').innerHTML = `<tr><td colspan="3" style="text-align: center; color: var(--foreground);">Failed to fetch database files.</td></tr>`;
                }
            } catch (err) {
                document.getElementById('admin-doc-rows-target').innerHTML = '<tr><td colspan="3" style="text-align: center; color: var(--foreground);">Database connection offline.</td></tr>';
            }
        }

        const SUB_CATEGORIES = {
            'RESIDENT_SPECIFIC': [
                { type: 'ALL', label: 'All Flat Docs' },
                { type: 'RESIDENT_PROPERTY', label: 'Property Docs' },
                { type: 'RESIDENT_KYC', label: 'KYC Docs' },
                { type: 'RESIDENT_RENTAL', label: 'Rental Agreements' }
            ],
            'GENERIC': [
                { type: 'ALL', label: 'All Generic Docs' },
                { type: 'GENERIC_RULEBOOK', label: 'Rulebooks' },
                { type: 'GENERIC_BYLAWS', label: 'Bylaws' }
            ],
            'MEETING_MINUTES': [
                { type: 'ALL', label: 'All Minutes' },
                { type: 'MINUTES_AGM', label: 'AGM Minutes' },
                { type: 'MINUTES_GBM', label: 'GBM Minutes' },
                { type: 'MINUTES_MGMT', label: 'Management Minutes' }
            ],
            'OPERATIONAL': [
                { type: 'ALL', label: 'All Operational' },
                { type: 'OPERATIONAL_VENDOR_CONTRACT', label: 'Vendor Contracts' },
                { type: 'OPERATIONAL_PAYMENT', label: 'Payment Receipts' },
                { type: 'OPERATIONAL_SLA', label: 'SLA Contracts' }
            ]
        };

        function filterAdminDocsBySubbar(category) {
            subbarCategoryFilter = category;
            subbarTypeFilter = 'ALL';

            // Update active state of filter chips
            document.querySelectorAll('#admin-filter-bar .filter-chip').forEach(el => el.classList.remove('active'));
            const chipId = 'admin-filter-' + category.toLowerCase().replace(/_/g, '-');
            const target = document.getElementById(chipId);
            if (target) target.classList.add('active');

            renderAdminSubFilters(category);
            filterAdminDocs();
        }

        function renderAdminSubFilters(category) {
            const subBar = document.getElementById('admin-sub-filter-bar');
            if (category === 'ALL' || !SUB_CATEGORIES[category]) {
                subBar.style.display = 'none';
                subBar.innerHTML = '';
                return;
            }

            subBar.style.display = 'flex';
            subBar.innerHTML = '';
            
            SUB_CATEGORIES[category].forEach(sub => {
                const button = document.createElement('button');
                button.className = 'sub-filter-chip' + (subbarTypeFilter === sub.type ? ' active' : '');
                button.textContent = sub.label;
                button.onclick = () => filterAdminDocsBySubtype(sub.type);
                subBar.appendChild(button);
            });
        }

        function filterAdminDocsBySubtype(subtype) {
            subbarTypeFilter = subtype;
            
            // Update active state of sub-filter chips
            document.querySelectorAll('#admin-sub-filter-bar .sub-filter-chip').forEach((el, index) => {
                const categoryList = SUB_CATEGORIES[subbarCategoryFilter];
                if (categoryList && categoryList[index]) {
                    if (categoryList[index].type === subtype) {
                        el.classList.add('active');
                    } else {
                        el.classList.remove('active');
                    }
                }
            });

            filterAdminDocs();
        }

        function filterAdminDocs() {
            const accessFilter = document.getElementById('filter-access').value;
            const statusFilter = document.getElementById('filter-status').value;

            const filtered = allAdminDocs.filter(d => {
                const doc = d.document;
                const matchesCat = (subbarCategoryFilter === 'ALL' || doc.category === subbarCategoryFilter);
                const matchesType = (subbarTypeFilter === 'ALL' || doc.document_type === subbarTypeFilter);
                const matchesAccess = (accessFilter === 'ALL' || doc.access_level === accessFilter);
                const matchesStatus = (statusFilter === 'ALL' || 
                    (statusFilter === 'ACTIVE' && !doc.is_deleted) || 
                    (statusFilter === 'DELETED' && doc.is_deleted));
                return matchesCat && matchesType && matchesAccess && matchesStatus;
            });

            renderAdminDocsTable(filtered);
        }

        function getCategoryBadgeClass(cat) {
            switch(cat) {
                case 'RESIDENT_SPECIFIC': return 'cat-resident';
                case 'GENERIC': return 'cat-generic';
                case 'MEETING_MINUTES': return 'cat-minutes';
                case 'OPERATIONAL': return 'cat-operational';
                default: return '';
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
                const tr = document.createElement('tr');
                
                let badgeHtml = doc.is_deleted 
                    ? '<span class="badge deleted">Deleted</span>'
                    : '';

                let metaString = 'N/A';
                if (meta) {
                    metaString = `
                        <div>Subcategory: <strong>${escapeHtml(meta.subcategory) || 'None'}</strong></div>
                        <div style="font-size:11px; margin-top:2px;">Tags: ${meta.tags.map(t => `#${escapeHtml(t)}`).join(', ') || 'None'}</div>
                        <div style="font-size:11px; margin-top:2px;">Date: ${escapeHtml(meta.document_date)}</div>
                    `;
                }

                tr.innerHTML = `
                    <td>
                        <div style="font-weight: 700; color: var(--primary); cursor: pointer; font-size: 1.125rem;" onclick="downloadDocument('${escapeHtml(doc.document_id)}', true)">${escapeHtml(doc.title)}</div>
                        <div style="font-size:13px; color:var(--muted-foreground); margin-top:4px; max-width: 480px; line-height: 1.45;">
                            ${escapeHtml(doc.description) || 'No description...'}
                        </div>
                        <div style="font-size:11px; color:var(--muted-foreground); margin-top:8px;">
                            Category: <strong>${escapeHtml(doc.category)}</strong> &nbsp;|&nbsp; 
                            Type: <strong>${escapeHtml(doc.document_type)}</strong> &nbsp;|&nbsp; 
                            Access: <strong>${escapeHtml(doc.access_level)}</strong>
                        </div>
                    </td>
                    <td>
                        ${badgeHtml}
                        <div style="font-size: 11px; color:var(--muted-foreground); margin-top:5px; line-height: 1.45;">${metaString}</div>
                    </td>
                    <td>
                        <div style="display: flex; gap: 8px; align-items: center;">
                            <button class="download-btn" style="padding: 6px 12px; font-size: 11px;" onclick="downloadDocument('${escapeHtml(doc.document_id)}', true)">Download</button>
                            ${!doc.is_deleted 
                                ? `<button class="del-action-btn" onclick="deleteDocument('${escapeHtml(doc.document_id)}')">Delete</button>`
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
                    const errText = await response.text();
                    alert(`Failed to get download link: ${errText}`);
                }
            } catch (err) {
                console.error(err);
                alert('Connection error while fetching download link.');
            }
        }

        async function deleteDocument(docId) {
            if (!confirm('Are you sure you want to delete this document? Regular residents will lose access immediately.')) return;
            try {
                const response = await fetch(`${API_BASE}/api/v1/admin/documents/${docId}/delete`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}` }
                });

                if (response.ok) {
                    loadAdminDocuments();
                } else {
                    alert('Failed to delete document.');
                }
            } catch (e) {
                alert('Connection failure deleting document.');
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
            const document_type = document.getElementById('doc-type').value;
            const access_level = document.getElementById('doc-access').value;

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

            // Optional Metadata fields
            const subcategory = document.getElementById('doc-subcategory').value.trim() || null;
            const tagsInput = document.getElementById('doc-tags').value.trim();
            const tags = tagsInput ? tagsInput.split(',').map(t => t.trim()) : [];
            const document_date = document.getElementById('doc-date').value || null;
            const valid_from = document.getElementById('doc-valid-from').value || null;
            const valid_until = document.getElementById('doc-valid-until').value || null;

            // Formulate payload
            const payload = {
                title,
                description,
                category,
                document_type,
                access_level,
                flat,
                metadata: document_date ? {
                    subcategory,
                    tags,
                    document_date,
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
            const allowedExtensions = ['.pdf', '.doc', '.docx', '.xls', '.xlsx', '.txt'];
            const fileName = file.name.toLowerCase();
            const isValidExtension = allowedExtensions.some(ext => fileName.endsWith(ext));
            if (!isValidExtension) {
                docFeedback.textContent = 'Invalid file type. Only PDF, Word, Excel, and Text files are allowed.';
                docFeedback.className = 'feedback-box error';
                docFeedback.style.display = 'block';
                return;
            }

            const formData = new FormData();
            formData.append('payload', JSON.stringify(payload));
            formData.append('file', file);

            try {
                const response = await fetch(`${API_BASE}/api/v1/admin/documents/create`, {
                    method: 'POST',
                    headers: {
                        'Authorization': `Bearer ${token}`
                    },
                    body: formData
                });

                if (response.ok) {
                    docFeedback.textContent = '✓ Document added!';
                    docFeedback.className = 'feedback-box success';
                    docFeedback.style.display = 'block';
                    regDocForm.reset();
                    handleCategoryChange();
                } else {
                    const errorMsg = await response.text();
                    docFeedback.textContent = `Error: ${errorMsg}`;
                    docFeedback.className = 'feedback-box error';
                    docFeedback.style.display = 'block';
                }
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
