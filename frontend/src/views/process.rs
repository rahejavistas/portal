pub const PROCESS_HTML: &str = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Vistas Portal - Processes</title>
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
            --danger: #EF4444;
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

        .layout-container {
            display: flex;
            width: 100%;
            min-height: 100vh;
            position: relative;
        }

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

        /* Sidebar */
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
            margin-bottom: 12px;
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

        /* Workspace */
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

        /* Tab subtabs */
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

        /* Cards */
        .card {
            background-color: var(--background);
            border-radius: var(--radius-lg);
            padding: 40px;
            margin-bottom: 32px;
            box-shadow: none;
            border: none;
        }

        /* Tables */
        .table-container {
            overflow-x: auto;
            border-radius: var(--radius-lg);
            background-color: var(--background);
            margin-top: 16px;
        }
        table {
            width: 100%;
            border-collapse: collapse;
            text-align: left;
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

        .process-title {
            font-weight: 700;
            font-size: 1.125rem;
            color: var(--primary);
            cursor: pointer;
            text-decoration: none;
            transition: color 0.15s ease;
        }
        .process-title:hover {
            color: var(--primary-hover);
            text-decoration: underline;
        }

        /* Status Pills */
        .status-pill {
            display: inline-block;
            padding: 4px 10px;
            border-radius: 9999px;
            font-size: 0.6875rem;
            font-weight: 700;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            background-color: var(--muted);
            color: var(--muted-foreground);
        }
        .status-pill.completed {
            background-color: #D1FAE5;
            color: #065F46;
        }
        .status-pill.pending_review {
            background-color: #FEF3C7;
            color: #92400E;
        }
        .status-pill.open {
            background-color: #DBEAFE;
            color: #1E40AF;
        }
        .status-pill.locked {
            background-color: var(--muted);
            color: var(--muted-foreground);
        }

        .btn {
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
            display: inline-block;
            text-align: center;
        }
        .btn:hover {
            background-color: var(--primary-hover);
            transform: scale(1.05);
        }
        .btn-secondary {
            background-color: var(--background);
            color: var(--foreground);
            border: 2px solid var(--border);
        }
        .btn-secondary:hover {
            background-color: var(--muted);
        }
        .btn-danger {
            background-color: var(--danger);
        }
        .btn-danger:hover {
            background-color: #DC2626;
        }

        /* Modal */
        .modal-overlay {
            display: none;
            position: fixed;
            top: 0;
            left: 0;
            width: 100vw;
            height: 100vh;
            background: rgba(0, 0, 0, 0.5);
            z-index: 9999;
            justify-content: center;
            align-items: center;
            padding: 20px;
        }
        .modal-content {
            background: var(--background);
            border-radius: var(--radius-lg);
            width: 100%;
            max-width: 500px;
            padding: 24px;
            position: relative;
        }
        .close-modal-btn {
            position: absolute;
            top: 16px;
            right: 16px;
            background: none;
            border: none;
            font-size: 24px;
            cursor: pointer;
            color: var(--muted-foreground);
        }

        /* Form Controls */
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
        .form-group select, .form-group input {
            padding: 10px;
            border: 1px solid var(--border);
            border-radius: var(--radius-md);
            font-size: 0.875rem;
        }

        /* Workspace layout */
        .process-workspace {
            display: flex;
            gap: 32px;
            margin-top: 24px;
            align-items: flex-start;
        }
        .process-sidebar {
            width: 280px;
            flex-shrink: 0;
            display: flex;
            flex-direction: column;
            gap: 12px;
        }
        .process-content-panel {
            flex-grow: 1;
            background-color: var(--background);
            border-radius: var(--radius-lg);
            padding: 32px;
            border: 1px solid var(--border);
        }
        @media (max-width: 992px) {
            .process-workspace {
                flex-direction: column;
                gap: 24px;
            }
            .process-sidebar {
                width: 100%;
            }
        }

        /* Step nav items (Sidebar) */
        .step-nav-item {
            background-color: var(--background);
            border: 1px solid var(--border);
            border-radius: var(--radius-lg);
            padding: 16px 20px;
            display: flex;
            align-items: center;
            justify-content: space-between;
            cursor: pointer;
            transition: all 0.2s ease;
            text-align: left;
            width: 100%;
        }
        .step-nav-item:hover {
            border-color: var(--primary);
            background-color: var(--muted);
            transform: translateY(-2px);
        }
        .step-nav-item.active {
            border-color: var(--primary);
            background-color: #EFF6FF;
            box-shadow: 0 4px 12px rgba(59, 130, 246, 0.08);
        }
        .step-nav-item.active .step-nav-name {
            color: var(--primary);
            font-weight: 700;
        }
        .step-nav-info {
            display: flex;
            flex-direction: column;
            gap: 4px;
        }
        .step-nav-num {
            font-size: 0.75rem;
            text-transform: uppercase;
            font-weight: 700;
            color: var(--muted-foreground);
            letter-spacing: 0.05em;
        }
        .step-nav-name {
            font-size: 0.9375rem;
            font-weight: 600;
            color: var(--foreground);
        }
        .step-nav-status {
            display: flex;
            align-items: center;
            justify-content: center;
            width: 24px;
            height: 24px;
            border-radius: 50%;
            font-size: 0.75rem;
            flex-shrink: 0;
            margin-left: 12px;
            font-weight: 700;
        }
        .step-nav-status.completed {
            background-color: #D1FAE5;
            color: #065F46;
        }
        .step-nav-status.pending_review {
            background-color: #FEF3C7;
            color: #92400E;
        }
        .step-nav-status.open {
            background-color: #DBEAFE;
            color: #1E40AF;
        }
        .step-nav-status.locked {
            background-color: var(--muted);
            color: var(--muted-foreground);
        }

        /* Step cards (Timeline) */
        .step-timeline {
            display: flex;
            flex-direction: column;
            gap: 24px;
            margin-top: 24px;
        }
        .step-card {
            border: 1px solid var(--border);
            border-radius: var(--radius-lg);
            background-color: var(--background);
            padding: 24px;
            position: relative;
            transition: all 0.2s ease;
        }
        .step-card.locked {
            opacity: 0.6;
            background-color: #FAFAFA;
        }
        .step-card.completed {
            border-left: 4px solid var(--secondary);
        }
        .step-card.pending_review {
            border-left: 4px solid var(--accent);
        }
        .step-card.open {
            border-left: 4px solid var(--primary);
            box-shadow: 0 4px 12px rgba(59, 130, 246, 0.05);
        }
        .step-header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 12px;
        }
        .step-title {
            font-size: 1.25rem;
            font-weight: 700;
            margin: 0;
        }
        .step-desc {
            font-size: 0.875rem;
            color: var(--muted-foreground);
            margin: 0 0 20px 0;
        }
        
        /* Field layouts inside steps */
        .fields-container {
            display: flex;
            flex-direction: column;
            gap: 16px;
            border-top: 1px solid var(--border);
            padding-top: 16px;
        }
        .field-row {
            display: flex;
            flex-direction: column;
            gap: 8px;
            padding: 12px;
            background-color: #F8FAFC;
            border-radius: var(--radius-md);
        }
        .field-label {
            font-weight: 600;
            font-size: 0.875rem;
        }
        .field-input-group {
            display: flex;
            gap: 12px;
            align-items: center;
            margin-top: 4px;
        }
        .field-value-display {
            font-size: 0.875rem;
            color: var(--foreground);
            background: #EFF6FF;
            padding: 6px 12px;
            border-radius: var(--radius-md);
            border: 1px solid #BFDBFE;
            display: inline-flex;
            align-items: center;
            gap: 8px;
        }
        .file-upload-input {
            font-size: 0.8125rem;
        }
        .history-box {
            margin-top: 24px;
            padding-top: 20px;
            border-top: 1px dashed var(--border);
        }
        .history-title {
            font-size: 0.875rem;
            font-weight: 700;
            text-transform: uppercase;
            color: var(--muted-foreground);
            margin-bottom: 12px;
        }
        .history-item {
            font-size: 0.8125rem;
            padding: 6px 12px;
            border-left: 2px solid var(--border);
            margin-bottom: 8px;
            background: #F9FAFB;
        }
    </style>
</head>
<body>
    <div class="bg-shape-top-right"></div>
    <div class="layout-container">
        <aside class="sidebar">
            <div class="sidebar-brand" id="sidebar-brand-container">
                <!-- Dynamically populated based on user role -->
            </div>
            <div>
                <button class="logout-btn" id="logout-btn">Log Out</button>
            </div>
        </aside>

        <main class="workspace">
            <!-- LIST VIEW SCREEN -->
            <section id="screen-list">
                <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 24px; flex-wrap: wrap; gap: 16px;">
                    <div>
                        <h2>Processes</h2>
                        <p class="section-desc" style="margin-bottom: 0;">Manage Your Processes</p>
                    </div>
                    <button class="btn" onclick="openStartProcessModal()">Start New Process</button>
                </div>

                <div class="subtab-bar" id="process-subtab-bar" style="display: none;">
                    <button class="subtab-btn active" id="tab-btn-admin-review" onclick="switchProcessTab('ADMIN_REVIEW_REQUIRED')">Admin Review Required</button>
                    <button class="subtab-btn" id="tab-btn-user-input" onclick="switchProcessTab('USER_INPUT_REQUIRED')">User Input Required</button>
                    <button class="subtab-btn" id="tab-btn-completed" onclick="switchProcessTab('COMPLETED')">Completed</button>
                </div>

                <div id="filters-container" style="display: flex; gap: 16px; margin-bottom: 24px; flex-wrap: wrap;">
                    <div class="form-group" style="margin-bottom: 0; flex-grow: 1; min-width: 200px;">
                        <label for="filter-search">Search Process / ID</label>
                        <input type="text" id="filter-search" placeholder="Search by Process name or ID..." oninput="filterProcesses()">
                    </div>
                    <div class="form-group" id="admin-flat-filter-group" style="display: none; margin-bottom: 0; flex-grow: 1; min-width: 200px;">
                        <label for="filter-flat">Filter by Flat</label>
                        <input type="text" id="filter-flat" placeholder="e.g. K-0302" oninput="filterProcesses()">
                    </div>
                    <div class="form-group" id="admin-email-filter-group" style="display: none; margin-bottom: 0; flex-grow: 1; min-width: 200px;">
                        <label for="filter-email">Filter by Email</label>
                        <input type="text" id="filter-email" placeholder="e.g. resident@vistas.com" oninput="filterProcesses()">
                    </div>
                </div>

                <div class="table-container">
                    <table>
                        <thead>
                            <tr>
                                <th>Process Name</th>
                                <th>Flat</th>
                                <th>State</th>
                                <th>Started On</th>
                            </tr>
                        </thead>
                        <tbody id="process-rows-target">
                            <tr>
                                <td colspan="5" style="text-align: center; color: var(--muted-foreground); padding: 40px; font-weight: 500;">
                                    Loading workflows...
                                </td>
                            </tr>
                        </tbody>
                    </table>
                </div>
            </section>

            <!-- DETAILS VIEW SCREEN -->
            <section id="screen-details" style="display: none;">
                <div style="margin-bottom: 24px;">
                    <button class="btn btn-secondary" onclick="showProcessListScreen()" style="margin-bottom: 16px; font-size: 1rem; padding: 10px 16px;">
                        &larr; Back to Processes
                    </button>
                    <div style="display: flex; justify-content: space-between; align-items: flex-start; flex-wrap: wrap; gap: 16px;">
                        <div>
                            <h2 id="details-process-title" style="margin-bottom: 4px; font-size: 1.75rem;">Process Details</h2>
                            <p class="section-desc" id="details-process-subtitle" style="font-size: 0.95rem; margin-bottom: 0;">Loading...</p>
                        </div>
                    </div>
                </div>

                <div class="process-workspace">
                    <!-- Step Navigation Sidebar -->
                    <div class="process-sidebar" id="details-sidebar-target">
                        <!-- Step list items will be rendered here -->
                    </div>

                    <!-- Selected Step Interaction Panel -->
                    <div class="process-content-panel" id="details-content-target">
                        <!-- Details of the selected step will be rendered here -->
                    </div>
                </div>
            </section>
        </main>
    </div>

    <!-- START PROCESS MODAL -->
    <div class="modal-overlay" id="start-process-modal">
        <div class="modal-content">
            <button class="close-modal-btn" onclick="closeStartProcessModal()">&times;</button>
            <h3 style="margin-top: 0; font-size: 1.5rem; font-weight: 800; margin-bottom: 24px;">Start New Process</h3>
            <form id="start-process-form" onsubmit="handleStartProcess(event)">
                <div class="form-group">
                    <label for="select-process-template">Process Type</label>
                    <select id="select-process-template" required>
                        <option value="tenant_onboarding_v1">Tenant Onboarding Process</option>
                    </select>
                </div>
                <div class="form-group" id="flat-select-group" style="display: none;">
                    <label for="select-process-flat">Select Flat</label>
                    <select id="select-process-flat">
                        <!-- Options populated dynamically -->
                    </select>
                </div>
                <div id="start-process-status" style="margin-top: 12px; font-size: 0.875rem; font-weight: 600;"></div>
                <div style="display: flex; justify-content: flex-end; gap: 12px; margin-top: 24px;">
                    <button type="button" class="btn btn-secondary" onclick="closeStartProcessModal()">Cancel</button>
                    <button type="submit" class="btn" id="start-process-submit-btn">Start</button>
                </div>
            </form>
        </div>
    </div>

    <script>
        const API_BASE = window.location.port === "4000" ? `http://${window.location.hostname}:3000` : "";
        let token = localStorage.getItem('vfs_token');
        let user = JSON.parse(localStorage.getItem('vfs_user') || 'null');
        let allProcesses = [];
        let currentTab = 'ADMIN_REVIEW_REQUIRED';
        let userFlats = [];
        let currentProcessDetails = null;
        let currentSelectedStepId = null;

        // Redirect if not logged in
        if (!token || !user) {
            window.location.href = '/login';
        }

        // Populate sidebar based on role
        const isAdmin = user.roles && user.roles.includes('Admin');
        const sidebarBrandContainer = document.getElementById('sidebar-brand-container');
        if (sidebarBrandContainer) {
            if (isAdmin) {
                sidebarBrandContainer.innerHTML = `
                    <h1>Admin</h1>
                    <p>Raheja Vistas Admin Room</p>
                    <nav class="nav-menu">
                        <a href="/admin" class="nav-item" style="text-decoration: none;">Residents</a>
                        <a href="/admin?tab=documents" class="nav-item" style="text-decoration: none;">Documents</a>
                        <a href="/process" class="nav-item active" style="text-decoration: none;">Processes</a>
                    </nav>
                `;
            } else {
                sidebarBrandContainer.innerHTML = `
                    <h1>Vistas</h1>
                    <p>Resident Portal</p>
                    <nav class="nav-menu">
                        <a href="/dashboard" class="nav-item" style="text-decoration: none;">Profile</a>
                        <a href="/dashboard?tab=documents" class="nav-item" style="text-decoration: none;">Documents</a>
                        <a href="/process" class="nav-item active" style="text-decoration: none;">Processes</a>
                    </nav>
                `;
            }
        }

        // Setup Log Out button
        document.getElementById('logout-btn').onclick = () => {
            localStorage.removeItem('vfs_token');
            localStorage.removeItem('vfs_user');
            window.location.href = '/login';
        };

        // Escapes HTML values safely
        function escapeHtml(str) {
            if (!str) return '';
            return str
                .replace(/&/g, "&amp;")
                .replace(/</g, "&lt;")
                .replace(/>/g, "&gt;")
                .replace(/"/g, "&quot;")
                .replace(/'/g, "&#039;");
        }

        // Helper to format ISO dates to "05 Jan 2026, 04:10PM" format
        function formatDateMinimal(isoStr) {
            if (!isoStr) return '';
            try {
                const date = new Date(isoStr);
                const day = String(date.getDate()).padStart(2, '0');
                const month = date.toLocaleDateString('en-US', { month: 'short' });
                const year = date.getFullYear();
                
                let hours = date.getHours();
                const minutes = String(date.getMinutes()).padStart(2, '0');
                const ampm = hours >= 12 ? 'PM' : 'AM';
                hours = hours % 12;
                hours = hours ? hours : 12;
                const hourStr = String(hours).padStart(2, '0');
                
                return `${day} ${month} ${year}, ${hourStr}:${minutes}${ampm}`;
            } catch(e) {
                return isoStr;
            }
        }

        function formatDate(isoStr) {
            return formatDateMinimal(isoStr);
        }

        // Switch between tabs
        function switchProcessTab(tab) {
            currentTab = tab;
            document.getElementById('tab-btn-admin-review').className = 'subtab-btn' + (tab === 'ADMIN_REVIEW_REQUIRED' ? ' active' : '');
            document.getElementById('tab-btn-user-input').className = 'subtab-btn' + (tab === 'USER_INPUT_REQUIRED' ? ' active' : '');
            renderProcessList();
        }

        // Load process list from backend
        async function loadProcesses() {
            if (user && user.roles && (user.roles.includes('admin') || user.roles.includes('Admin'))) {
                const adminEmailGroup = document.getElementById('admin-email-filter-group');
                if (adminEmailGroup) adminEmailGroup.style.display = 'flex';
                const adminFlatGroup = document.getElementById('admin-flat-filter-group');
                if (adminFlatGroup) adminFlatGroup.style.display = 'flex';
                const processSubtabBar = document.getElementById('process-subtab-bar');
                if (processSubtabBar) processSubtabBar.style.display = 'flex';
                currentTab = 'ADMIN_REVIEW_REQUIRED';
            } else {
                currentTab = 'ALL';
            }
            try {
                const response = await fetch(`${API_BASE}/api/v1/process/list`, {
                    method: 'POST',
                    headers: {
                        'Authorization': `Bearer ${token}`
                    }
                });
                if (response.ok) {
                    const data = await response.json();
                    allProcesses = data.processes;
                    userFlats = data.flats;
                    renderProcessList();
                } else if (response.status === 401) {
                    window.location.href = '/login';
                } else {
                    const errText = await response.text();
                    document.getElementById('process-rows-target').innerHTML = `
                        <tr>
                            <td colspan="5" style="text-align: center; color: var(--danger); padding: 40px; font-weight: 500;">
                                Failed to load processes: ${escapeHtml(errText)}
                            </td>
                        </tr>
                    `;
                }
            } catch(e) {
                console.error(e);
                document.getElementById('process-rows-target').innerHTML = `
                    <tr>
                        <td colspan="5" style="text-align: center; color: var(--danger); padding: 40px; font-weight: 500;">
                            Network error while loading processes.
                        </td>
                    </tr>
                `;
            }
        }

        // Apply input filters on input change
        function filterProcesses() {
            renderProcessList();
        }

        // Render processes in table
        function renderProcessList() {
            const tbody = document.getElementById('process-rows-target');
            tbody.innerHTML = '';

            const searchQuery = document.getElementById('filter-search').value.trim().toLowerCase();
            const flatQuery = document.getElementById('filter-flat').value.trim().toLowerCase();
            const emailQuery = document.getElementById('filter-email').value.trim().toLowerCase();
            const isAdmin = user.roles && (user.roles.includes('admin') || user.roles.includes('Admin'));

            // Sort by creation date descending (oldest one should be below)
            allProcesses.sort((a, b) => new Date(b.created_at) - new Date(a.created_at));

            const filtered = allProcesses.filter(p => {
                // Use the backend-provided status to determine if user or admin action is required
                const isUserAction = p.status === 'USER_INPUT_REQUIRED';
                const isCompleted = p.status === 'COMPLETED';
                
                if (isAdmin) {
                    if (currentTab === 'USER_INPUT_REQUIRED' && !isUserAction) return false;
                    if (currentTab === 'ADMIN_REVIEW_REQUIRED' && p.status !== 'ADMIN_REVIEW_REQUIRED') return false;
                    if (currentTab === 'COMPLETED' && !isCompleted) return false;
                } else {
                    // Resident sees all processes, we don't hide completed ones anymore.
                }

                // 1. Search Query (Process Name, Template ID, or Process ID)
                if (searchQuery) {
                    const templateName = p.template_id === 'tenant_onboarding_v1' ? 'Tenant Onboarding Process' : p.template_id;
                    const matchesName = templateName.toLowerCase().includes(searchQuery);
                    const matchesId = p.process_id.toLowerCase().includes(searchQuery);
                    if (!matchesName && !matchesId) {
                        return false;
                    }
                }

                // 2. Flat Query
                if (flatQuery && (!p.flat_label || !p.flat_label.toLowerCase().includes(flatQuery))) {
                    return false;
                }

                // 3. Email Query (Admin-only filter)
                if (isAdmin && emailQuery && (!p.user_email || !p.user_email.toLowerCase().includes(emailQuery))) {
                    return false;
                }

                return true;
            });

            if (filtered.length === 0) {
                tbody.innerHTML = `
                    <tr>
                        <td colspan="5" style="text-align: center; color: var(--muted-foreground); padding: 40px; font-weight: 500;">
                            No processes found.
                        </td>
                    </tr>
                `;
                return;
            }

            filtered.forEach(p => {
                const tr = document.createElement('tr');
                let stateText = "User Input Required";
                if (p.status === 'ADMIN_REVIEW_REQUIRED') stateText = "Admin Review Required";
                if (p.status === 'COMPLETED') stateText = "Completed";
                
                tr.innerHTML = `
                    <td>
                        <div class="process-title" onclick="viewProcessDetail('${p.process_id}')">
                            ${p.template_id === 'tenant_onboarding_v1' ? 'Tenant Onboarding Process' : escapeHtml(p.template_id)}
                        </div>
                        <div style="font-size: 0.75rem; color: var(--muted-foreground); margin-top: 4px;">
                            ID: ${p.process_id.substring(0, 8)} ${isAdmin ? `(${escapeHtml(p.user_email)})` : ''}
                        </div>
                    </td>
                    <td>
                        <strong>${p.flat_label ? escapeHtml(p.flat_label) : 'TBD'}</strong>
                    </td>
                    <td>
                        <span style="font-size: 0.8125rem; font-weight: 600; color: var(--primary);">${stateText}</span>
                    </td>
                    <td>
                        ${formatDate(p.created_at)}
                    </td>
                `;
                tbody.appendChild(tr);
            });
        }

        // Open Modal to start process
        async function openStartProcessModal() {
            const statusEl = document.getElementById('start-process-status');
            const flatGroup = document.getElementById('flat-select-group');
            const flatSelect = document.getElementById('select-process-flat');
            statusEl.textContent = '';
            
            document.getElementById('start-process-modal').style.display = 'flex';

            // Admins cannot start a process for themselves
            if (user.roles && (user.roles.includes('admin') || user.roles.includes('Admin'))) {
                statusEl.style.color = 'var(--danger)';
                statusEl.textContent = 'Administrative accounts cannot initiate processes directly. Please use the resident view.';
                document.getElementById('start-process-submit-btn').disabled = true;
                return;
            }

            document.getElementById('start-process-submit-btn').disabled = false;

            // Populate from preloaded userFlats
            flatSelect.innerHTML = '';
            if (userFlats.length > 1) {
                flatGroup.style.display = 'block';
                userFlats.forEach(f => {
                    const opt = document.createElement('option');
                    opt.value = f.flat_id;
                    opt.textContent = `${escapeHtml(f.wing)}-${escapeHtml(f.flat_number)}`;
                    flatSelect.appendChild(opt);
                });
            } else {
                flatGroup.style.display = 'none';
            }
        }

        function closeStartProcessModal() {
            document.getElementById('start-process-modal').style.display = 'none';
            document.getElementById('start-process-form').reset();
        }

        // Handle process creation
        async function handleStartProcess(e) {
            e.preventDefault();
            const templateId = document.getElementById('select-process-template').value;
            const flatSelect = document.getElementById('select-process-flat');
            const flatGroup = document.getElementById('flat-select-group');
            const statusEl = document.getElementById('start-process-status');
            const submitBtn = document.getElementById('start-process-submit-btn');

            let flatId = null;
            if (flatGroup.style.display === 'block' && flatSelect.value) {
                flatId = parseInt(flatSelect.value, 10);
            } else if (userFlats.length === 1) {
                flatId = userFlats[0].flat_id;
            }

            submitBtn.disabled = true;
            submitBtn.textContent = 'Starting...';
            statusEl.textContent = '';

            try {
                const payload = {
                    template_id: templateId,
                    flat_id: flatId
                };

                const response = await fetch(`${API_BASE}/api/v1/process/state`, {
                    method: 'POST',
                    headers: {
                        'Authorization': `Bearer ${token}`,
                        'Content-Type': 'application/json'
                    },
                    body: JSON.stringify(payload)
                });

                if (response.ok) {
                    const data = await response.json();
                    closeStartProcessModal();
                    await loadProcesses();
                    viewProcessDetail(data.process_id);
                } else {
                    const errText = await response.text();
                    statusEl.style.color = 'var(--danger)';
                    statusEl.textContent = `Failed to start process: ${errText}`;
                }
            } catch(e) {
                statusEl.style.color = 'var(--danger)';
                statusEl.textContent = 'Connection error.';
            } finally {
                submitBtn.disabled = false;
                submitBtn.textContent = 'Start';
            }
        }

        // Toggle Screen
        function showProcessListScreen() {
            document.getElementById('screen-list').style.display = 'block';
            document.getElementById('screen-details').style.display = 'none';
            const menuBtn = document.getElementById('menu-btn-list');
            if (menuBtn) menuBtn.classList.add('active');
        }

        function showProcessDetailsScreen() {
            document.getElementById('screen-list').style.display = 'none';
            document.getElementById('screen-details').style.display = 'block';
            const menuBtn = document.getElementById('menu-btn-list');
            if (menuBtn) menuBtn.classList.remove('active');
        }

        // View single process details
        async function viewProcessDetail(processId) {
            showProcessDetailsScreen();
            
            document.getElementById('details-process-title').textContent = 'Loading Workflow...';
            document.getElementById('details-process-subtitle').textContent = 'Fetching current step statuses and reviews from the backend.';
            document.getElementById('details-sidebar-target').innerHTML = `
                <div style="text-align: center; color: var(--muted-foreground); padding: 12px; font-size: 0.875rem;">
                    Loading stages...
                </div>
            `;
            document.getElementById('details-content-target').innerHTML = `
                <div style="text-align: center; color: var(--muted-foreground); padding: 40px;">
                    Retrieving workflow details...
                </div>
            `;

            try {
                const response = await fetch(`${API_BASE}/api/v1/process/state`, {
                    method: 'POST',
                    headers: {
                        'Authorization': `Bearer ${token}`,
                        'Content-Type': 'application/json'
                    },
                    body: JSON.stringify({ process_id: processId })
                });

                if (response.ok) {
                    currentProcessDetails = await response.json();
                    renderProcessDetails();
                } else {
                    const errText = await response.text();
                    document.getElementById('details-process-title').textContent = 'Error Loading Process';
                    document.getElementById('details-process-subtitle').textContent = escapeHtml(errText);
                    document.getElementById('details-sidebar-target').innerHTML = '';
                    document.getElementById('details-content-target').innerHTML = '';
                }
            } catch(e) {
                console.error(e);
                document.getElementById('details-process-title').textContent = 'Error Loading Process';
                document.getElementById('details-process-subtitle').textContent = 'Connection error occurred.';
                document.getElementById('details-sidebar-target').innerHTML = '';
                document.getElementById('details-content-target').innerHTML = '';
            }
        }

        // Render Process Details page
        function renderProcessDetails() {
            if (!currentProcessDetails) return;

            const p = currentProcessDetails;
            document.getElementById('details-process-title').textContent = p.process_name || p.template_id;
            
            const completionBadge = p.is_completed ? '<span class="status-pill completed" style="margin-left: 12px;">Completed</span>' : '<span class="status-pill open" style="margin-left: 12px;">In Progress</span>';
            document.getElementById('details-process-title').innerHTML += completionBadge;
            document.getElementById('details-process-subtitle').textContent = `Flat: ${p.flat_label ? escapeHtml(p.flat_label) : 'TBD'} | ID: ${p.process_id.substring(0, 8)}`;

            // Render steps in the sidebar
            const sidebar = document.getElementById('details-sidebar-target');
            sidebar.innerHTML = '';

            const steps = p.steps || [];
            if (steps.length === 0) {
                sidebar.innerHTML = '<div style="color: var(--muted-foreground); text-align: center; padding: 12px;">No stages available</div>';
                return;
            }

            // Set default selected step if not set or not in current list
            if (!currentSelectedStepId || !steps.some(s => s.template_step_id === currentSelectedStepId)) {
                // Find first OPEN or PENDING_REVIEW step, otherwise default to first step
                const activeStep = steps.find(s => s.status === 'OPEN' || s.status === 'PENDING_REVIEW');
                currentSelectedStepId = activeStep ? activeStep.template_step_id : steps[0].template_step_id;
            }

            steps.forEach((step, index) => {
                const isActive = step.template_step_id === currentSelectedStepId;
                
                let statusLabel = 'Locked';
                if (step.status === 'COMPLETED') statusLabel = 'Completed';
                else if (step.status === 'PENDING_REVIEW') statusLabel = 'Reviewing';
                else if (step.status === 'OPEN') statusLabel = 'Active';

                const btn = document.createElement('button');
                btn.className = 'step-nav-item' + (isActive ? ' active' : '');
                btn.onclick = () => selectStep(step.template_step_id);
                
                btn.innerHTML = `
                    <div class="step-nav-info">
                        <span class="step-nav-num">Stage ${index + 1}</span>
                        <span class="step-nav-name">${escapeHtml(step.name)}</span>
                    </div>
                    <span class="step-nav-status ${step.status.toLowerCase()}">${statusLabel}</span>
                `;
                sidebar.appendChild(btn);
            });

            // Render the currently selected step content in the content panel
            renderSelectedStepContent();
        }

        function selectStep(stepId) {
            currentSelectedStepId = stepId;
            renderProcessDetails();
        }

        // Helper to render fields list for a step/substep
        function renderFieldsList(fields, stepStatus, stepId, parentStatus = null) {
            let html = '';
            const isCurrentAdmin = user.roles && (user.roles.includes('admin') || user.roles.includes('Admin'));
            
            fields.forEach(f => {
                const fieldId = f.template_field_id;
                const filledBy = f.filled_by; // 'user' or 'admin'
                const type = f.type; // 'document' or 'text'
                const val = f.value || { text_value: null, document_id: null, document_title: null };

                let inputHtml = '';
                let isReadOnly = true;
                if (stepStatus === 'OPEN' || parentStatus === 'OPEN') {
                    if (filledBy === 'user' && !isCurrentAdmin) {
                        isReadOnly = false;
                    } else if (filledBy === 'admin' && isCurrentAdmin) {
                        isReadOnly = false;
                    }
                }

                if (type === 'document') {
                    const history = f.history || [];
                    let allDocs = [];
                    if (val.document_id) allDocs.push({ document_id: val.document_id, document_title: val.document_title, updated_at: val.updated_at });
                    for (const h of history) {
                        if (h.document_id) allDocs.push(h);
                    }
                    
                    const uniqueDocs = [];
                    const seenIds = new Set();
                    for (const doc of allDocs) {
                        if (!seenIds.has(doc.document_id)) {
                            seenIds.add(doc.document_id);
                            uniqueDocs.push(doc);
                        }
                    }

                    if (uniqueDocs.length > 0) {
                        inputHtml = uniqueDocs.map(doc => `
                            <div class="field-value-display" style="padding: 10px 16px; border-radius: var(--radius-lg); font-size: 0.9375rem; margin-bottom: 8px; background-color: var(--background); border: 1px solid var(--border);">
                                <span style="font-weight: 600;">Document: ${escapeHtml(doc.document_title || 'File')}</span>
                                <button type="button" class="btn btn-secondary" style="padding: 6px 12px; font-size: 0.75rem; margin-left: 12px;" onclick="downloadProcessDoc('${escapeHtml(doc.document_id)}')">Download</button>
                            </div>
                        `).join('');
                    } else {
                        inputHtml = `<span style="font-size: 0.875rem; color: var(--muted-foreground); font-style: italic; display: block; margin-bottom: 8px;">No document uploaded yet</span>`;
                    }

                    if (!isReadOnly) {
                        inputHtml += `
                            <div class="field-input-group" style="margin-top: 12px; display: flex; align-items: center; gap: 12px;">
                                <button type="button" class="btn btn-secondary" onclick="openDocumentPickerModal(${stepId}, ${fieldId})" style="font-size: 0.875rem; padding: 8px 16px;">Choose Document</button>
                                <span id="upload-progress-${fieldId}" style="font-size: 0.8125rem; font-weight: 700; color: var(--primary);"></span>
                            </div>
                        `;
                    }
                } else {
                    // text field
                    if (isReadOnly) {
                        inputHtml = `
                            <div class="field-value-display" style="background-color: var(--muted); border-color: var(--border); color: var(--foreground); padding: 10px 16px; border-radius: var(--radius-lg); font-size: 0.9375rem; display: block; min-height: 44px; width: 100%; max-width: 500px;">
                                ${val.text_value ? escapeHtml(val.text_value) : '<em style="color: var(--muted-foreground);">No remarks entered</em>'}
                            </div>
                        `;
                    } else {
                        inputHtml = `
                            <input type="text" id="text-input-${fieldId}" class="text-value-input" style="width: 100%; max-width: 500px; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-lg); font-size: 0.9375rem;" value="${escapeHtml(val.text_value || '')}" placeholder="Enter details...">
                        `;
                    }
                }

                // History removed as per request
                let historyHtml = '';

                let filledByHtml = '';
                if (filledBy === 'admin') {
                    filledByHtml = `<div style="font-size: 0.75rem; color: #b45309; margin-bottom: 10px; text-transform: uppercase; font-weight: 600;">To be completed by Admin</div>`;
                }

                html += `
                    <div class="field-row" style="padding: 16px; background-color: #F8FAFC; border-radius: var(--radius-lg); border: 1px solid var(--border); margin-bottom: 12px;">
                        <span class="field-label" style="font-size: 0.95rem; font-weight: 700; display: block; margin-bottom: 4px;">
                            ${escapeHtml(f.name)} ${f.is_optional ? '<span style="color: var(--muted-foreground); font-weight: normal; font-size: 0.8125rem;">(Optional)</span>' : ''}
                        </span>
                        ${filledByHtml}
                        <div style="margin-top: 6px; margin-bottom: 6px;">${inputHtml}</div>
                        ${historyHtml}
                    </div>
                `;
            });

            // Save button for this step
            const hasEditableTextFields = fields.some(f => {
                const isFUser = f.filled_by === 'user';
                return (isFUser && !isCurrentAdmin && f.type === 'text') || (!isFUser && isCurrentAdmin && f.type === 'text');
            });
            const canSave = (stepStatus === 'OPEN' || parentStatus === 'OPEN') && hasEditableTextFields;

            if (canSave) {
                html += `
                    <div style="display: flex; justify-content: flex-end; margin-top: 16px;">
                        <button class="btn" style="font-size: 0.875rem; padding: 12px 24px;" id="save-step-btn-${stepId}" onclick="saveStepFieldsSubmit(${stepId})">Save Remarks</button>
                    </div>
                `;
            }

            return html;
        }

        // Render the currently selected step content in the content panel
        function renderSelectedStepContent() {
            const p = currentProcessDetails;
            const contentPanel = document.getElementById('details-content-target');
            contentPanel.innerHTML = '';

            const selectedStep = p.steps.find(s => s.template_step_id === currentSelectedStepId);
            if (!selectedStep) {
                contentPanel.innerHTML = '<div style="color: var(--muted-foreground); text-align: center; padding: 40px;">Select a stage to view details.</div>';
                return;
            }

            const stepStatus = selectedStep.status; // LOCKED, OPEN, PENDING_REVIEW, COMPLETED
            const stepId = selectedStep.template_step_id;

            // Render header
            let statusBanner = '';
            if (stepStatus === 'LOCKED') {
                statusBanner = `
                    <div style="background-color: var(--muted); border: 1px solid var(--border); padding: 16px; border-radius: var(--radius-lg); margin-bottom: 24px; display: flex; align-items: center; gap: 12px;">
                        <div>
                            <strong style="color: var(--muted-foreground); font-size: 1rem; display: block;">Locked Stage</strong>
                            <span style="font-size: 0.875rem; color: var(--muted-foreground);">This stage will unlock automatically once the previous stages are completed and approved.</span>
                        </div>
                    </div>
                `;
            } else if (stepStatus === 'OPEN') {
                const isCurrentAdmin = user.roles && (user.roles.includes('admin') || user.roles.includes('Admin'));
                const hasAdminFieldsOnly = selectedStep.fields && selectedStep.fields.length > 0 && selectedStep.fields.every(f => f.filled_by === 'admin');
                
                if (isCurrentAdmin || !hasAdminFieldsOnly) {
                    statusBanner = `
                        <div style="background-color: #EFF6FF; border: 1px solid #BFDBFE; padding: 16px; border-radius: var(--radius-lg); margin-bottom: 24px; display: flex; align-items: center; gap: 12px;">
                            <div>
                                <strong style="color: #1E40AF; font-size: 1rem; display: block;">Active Stage</strong>
                                <span style="font-size: 0.875rem; color: #1E40AF;">Please upload the required files or fill in the remarks below.</span>
                            </div>
                        </div>
                    `;
                } else {
                    statusBanner = `
                        <div style="background-color: #FFFBEB; border: 1px solid #FDE68A; padding: 16px; border-radius: var(--radius-lg); margin-bottom: 24px; display: flex; align-items: center; gap: 12px;">
                            <div>
                                <strong style="color: #92400E; font-size: 1rem; display: block;">Under Admin Review</strong>
                                <span style="font-size: 0.875rem; color: #92400E;">This stage is currently waiting for Admin action (remarks/completion).</span>
                            </div>
                        </div>
                    `;
                }
            } else if (stepStatus === 'PENDING_REVIEW') {
                statusBanner = `
                    <div style="background-color: #FFFBEB; border: 1px solid #FDE68A; padding: 16px; border-radius: var(--radius-lg); margin-bottom: 24px; display: flex; align-items: center; gap: 12px;">
                        <div>
                            <strong style="color: #92400E; font-size: 1rem; display: block;">Under Review</strong>
                            <span style="font-size: 0.875rem; color: #92400E;">Admin is currently reviewing your submissions for this stage.</span>
                        </div>
                    </div>
                `;
            } else if (stepStatus === 'COMPLETED') {
                statusBanner = `
                    <div style="background-color: #ECFDF5; border: 1px solid #A7F3D0; padding: 16px; border-radius: var(--radius-lg); margin-bottom: 24px; display: flex; align-items: center; gap: 12px;">
                        <div>
                            <strong style="color: #065F46; font-size: 1rem; display: block;">Stage Approved</strong>
                            <span style="font-size: 0.875rem; color: #065F46;">This stage has been completed and approved by the administration.</span>
                        </div>
                    </div>
                `;
            }

            // Render ongoing reviews by admin for this step
            const stepReviewIds = [selectedStep.template_step_id];
            if (selectedStep.substeps) {
                selectedStep.substeps.forEach(sub => stepReviewIds.push(sub.template_step_id));
            }
            const relevantReviews = (p.reviews || []).filter(r => stepReviewIds.includes(r.template_step_id));

            let reviewsHtml = '';
            if (relevantReviews.length > 0) {
                reviewsHtml = `
                    <button class="btn btn-secondary" id="toggle-history-btn" style="margin-bottom: 24px; font-size: 0.875rem; padding: 10px 20px; border: 1px solid var(--border); background: var(--card); color: var(--foreground); cursor: pointer;" onclick="toggleReviewHistory()">View Review History</button>
                    <div id="review-history-container" style="display: none; margin-bottom: 32px;">
                        <h4 style="margin: 0 0 16px 0; font-size: 1.125rem; font-weight: 800; color: var(--foreground);">Admin Review Comments</h4>
                        <div style="display: flex; flex-direction: column; gap: 12px;">
                `;
                relevantReviews.forEach(r => {
                    const isApp = r.is_approved;
                    const dateFormatted = formatDateMinimal(r.created_at);
                    const labelColor = isApp ? '#065F46' : '#991B1B';
                    const bgColor = isApp ? '#ECFDF5' : '#FEE2E2';
                    const borderLeftColor = isApp ? 'var(--secondary)' : 'var(--danger)';

                    reviewsHtml += `
                        <div style="background-color: ${bgColor}; border-left: 4px solid ${borderLeftColor}; padding: 16px; border-radius: var(--radius-lg);">
                            <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px; flex-wrap: wrap; gap: 8px;">
                                <strong style="font-size: 0.95rem; color: ${labelColor};">
                                    ${isApp ? 'Approved' : 'Revision Required'}
                                </strong>
                                <span style="font-size: 0.8125rem; color: var(--muted-foreground); font-weight: 600;">
                                    ${dateFormatted}
                                </span>
                            </div>
                            <p style="margin: 0; font-size: 0.9375rem; color: var(--foreground); line-height: 1.5;">
                                ${r.notes ? escapeHtml(r.notes) : '<em>No comments provided</em>'}
                            </p>
                        </div>
                    `;
                });
                reviewsHtml += `
                        </div>
                    </div>
                `;
            }

            let html = `
                <div style="margin-bottom: 24px;">
                    <h3 style="margin: 0 0 8px 0; font-size: 1.5rem; font-weight: 800; color: var(--foreground);">${escapeHtml(selectedStep.name)}</h3>
                    <p style="margin: 0; font-size: 1rem; color: var(--muted-foreground); line-height: 1.5;">${escapeHtml(selectedStep.description || '')}</p>
                </div>
                ${statusBanner}
                ${reviewsHtml}
            `;

            // If the selected step has fields directly, render them
            if (selectedStep.fields && selectedStep.fields.length > 0 && stepStatus !== 'LOCKED') {
                html += `<div class="fields-container" style="border-top: none; padding-top: 0; margin-bottom: 24px;">`;
                html += renderFieldsList(selectedStep.fields, stepStatus, stepId);
                html += `</div>`;
            }

            // If the selected step has substeps, render each substep card vertically
            if (selectedStep.substeps && selectedStep.substeps.length > 0) {
                html += `<div style="display: flex; flex-direction: column; gap: 24px; margin-top: 16px;">`;
                
                selectedStep.substeps.forEach(sub => {
                    const subStatus = sub.status;
                    const subId = sub.template_step_id;
                    const subStatusLabel = subStatus.replace('_', ' ');
                    
                    let subBadgeClass = 'locked';
                    if (subStatus === 'COMPLETED') subBadgeClass = 'completed';
                    else if (subStatus === 'PENDING_REVIEW') subBadgeClass = 'pending_review';
                    else if (subStatus === 'OPEN') subBadgeClass = 'open';

                    html += `
                        <div class="step-card ${subStatus.toLowerCase()}" style="padding: 20px; border-radius: var(--radius-lg); border-color: ${subStatus === 'OPEN' ? 'var(--primary)' : 'var(--border)'};">
                            <div class="step-header" style="margin-bottom: 8px;">
                                <h4 class="step-title" style="font-size: 1.125rem; font-weight: 700;">${escapeHtml(sub.name)}</h4>
                                <span class="status-pill ${subBadgeClass}">${subStatusLabel}</span>
                            </div>
                            <p class="step-desc" style="margin-bottom: 16px; font-size: 0.875rem;">${escapeHtml(sub.description || '')}</p>
                    `;

                    if (sub.fields && sub.fields.length > 0 && subStatus !== 'LOCKED') {
                        html += `<div class="fields-container" style="border-top: 1px solid var(--border); padding-top: 16px;">`;
                        html += renderFieldsList(sub.fields, subStatus, subId, stepStatus);
                        html += `</div>`;
                    }

                    html += `</div>`;
                });

                html += `</div>`;
            }

            // Admin review logic moved above

            // Render admin action panel (Approve / Reject) if user is admin
            const isCurrentAdmin = user.roles && (user.roles.includes('admin') || user.roles.includes('Admin'));
            if (isCurrentAdmin) {
                const isCompleted = stepStatus === 'COMPLETED';
                const rejectButtonLabel = isCompleted ? 'Revoke & Reopen Step' : 'Send Back / Reject';
                const rejectOnClick = isCompleted ? `submitAdminRevoke(${stepId})` : `submitAdminReview(${stepId}, false)`;
                const approveOnClick = `submitAdminReview(${stepId}, true)`;
                const approveButtonLabel = isCompleted ? 'Approved' : 'Approve Step';
                const approveButtonClass = isCompleted ? 'btn btn-secondary' : 'btn';
                const approveDisabled = isCompleted ? 'disabled' : '';

                html += `
                    <div style="border-top: 2px solid var(--border); padding-top: 24px; margin-top: 32px;">
                        <h4 style="margin: 0 0 16px 0; font-size: 1.25rem; font-weight: 800; color: var(--foreground);">Admin Action Panel</h4>
                        <div class="form-group" style="margin-bottom: 16px;">
                            <label for="review-notes-${stepId}" style="font-weight: 700; font-size: 0.8125rem; text-transform: uppercase;">Review Remarks (Shown to Resident)</label>
                            <textarea id="review-notes-${stepId}" style="width: 100%; min-height: 100px; padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-lg); font-family: inherit; font-size: 0.9375rem; line-height: 1.5;" placeholder="Enter comments or instructions..."></textarea>
                        </div>
                        <div style="display: flex; gap: 16px; justify-content: flex-end;">
                            <button class="btn btn-danger" style="font-size: 0.875rem; padding: 12px 24px;" onclick="${rejectOnClick}">${rejectButtonLabel}</button>
                            <button class="${approveButtonClass}" style="font-size: 0.875rem; padding: 12px 24px;" ${approveDisabled} onclick="${approveOnClick}">${approveButtonLabel}</button>
                        </div>
                    </div>
                `;
            }

            contentPanel.innerHTML = html;
        }

        // Fetch presigned URL and download file
        async function downloadProcessDoc(documentId) {
            const processId = currentProcessDetails.process_id;
            try {
                const response = await fetch(`${API_BASE}/api/v1/process/download-url`, {
                    method: 'POST',
                    headers: {
                        'Authorization': `Bearer ${token}`,
                        'Content-Type': 'application/json'
                    },
                    body: JSON.stringify({
                        process_id: processId,
                        document_id: documentId
                    })
                });

                if (response.ok) {
                    const data = await response.json();
                    if (data.download_url) {
                        window.open(data.download_url, '_blank');
                    } else {
                        alert('Download URL not returned by server.');
                    }
                } else {
                    const errText = await response.text();
                    alert(`Download failed: ${errText}`);
                }
            } catch(e) {
                console.error(e);
                alert('Connection error while retrieving file.');
            }
        }

        // Upload a document for a process field
        async function uploadFieldDocument(stepId, fieldId) {
            const fileInput = document.getElementById(`file-input-${fieldId}`);
            const progressEl = document.getElementById(`upload-progress-${fieldId}`);
            
            if (fileInput.files.length === 0) return;
            const file = fileInput.files[0];

            if (file.size > 10 * 1024 * 1024) {
                alert('File size must be under 10MB.');
                fileInput.value = '';
                return;
            }

            progressEl.style.color = 'var(--primary)';
            progressEl.textContent = 'Uploading...';

            const processId = currentProcessDetails.process_id;
            const formData = new FormData();
            formData.append('process_id', processId);
            formData.append('title', file.name);
            formData.append('file', file);

            try {
                const response = await fetch(`${API_BASE}/api/v1/process/upload`, {
                    method: 'POST',
                    headers: {
                        'Authorization': `Bearer ${token}`
                    },
                    body: formData
                });

                if (response.ok) {
                    const docObj = await response.json();
                    const docId = docObj.document ? docObj.document.document_id : docObj.document_id;
                    
                    // Call save step directly to attach the uploaded document to the field
                    const payload = {
                        process_id: processId,
                        template_step_id: stepId,
                        fields: [
                            {
                                template_field_id: fieldId,
                                text_value: null,
                                document_id: docId
                            }
                        ]
                    };

                    const saveRes = await fetch(`${API_BASE}/api/v1/process/save-step`, {
                        method: 'POST',
                        headers: {
                            'Authorization': `Bearer ${token}`,
                            'Content-Type': 'application/json'
                        },
                        body: JSON.stringify(payload)
                    });

                    if (saveRes.ok) {
                        progressEl.style.color = 'var(--secondary)';
                        progressEl.textContent = 'Success!';
                        // Reload state
                        await viewProcessDetail(processId);
                    } else {
                        const err = await saveRes.text();
                        progressEl.style.color = 'var(--danger)';
                        progressEl.textContent = 'Save failed.';
                        alert(`Failed to save step: ${err}`);
                    }
                } else {
                    const errText = await response.text();
                    progressEl.style.color = 'var(--danger)';
                    progressEl.textContent = 'Upload failed.';
                    alert(`Upload failed: ${errText}`);
                }
            } catch(e) {
                console.error(e);
                progressEl.style.color = 'var(--danger)';
                progressEl.textContent = 'Error.';
                alert('Connection error during upload.');
            }
        }

        // Save step fields (mainly text inputs)
        async function saveStepFieldsSubmit(stepId) {
            const step = findStepInTree(currentProcessDetails.steps, stepId);
            if (!step) return;

            const fieldsPayload = [];
            let hasError = false;
            const isCurrentAdmin = user.roles && (user.roles.includes('admin') || user.roles.includes('Admin'));

            step.fields.forEach(f => {
                const fieldId = f.template_field_id;
                const isFieldEditable = (f.filled_by === 'user' && !isCurrentAdmin) || (f.filled_by === 'admin' && isCurrentAdmin);
                
                if (isFieldEditable) {
                    if (f.type === 'text') {
                        const inputEl = document.getElementById(`text-input-${fieldId}`);
                        if (inputEl) {
                            const val = inputEl.value.trim();
                            if (!f.is_optional && !val) {
                                alert(`Field "${f.name}" is required.`);
                                hasError = true;
                                return;
                            }
                            fieldsPayload.push({
                                template_field_id: fieldId,
                                text_value: val ? val : null,
                                document_id: null
                            });
                        }
                    } else if (f.type === 'document') {
                        const existingDocId = f.value ? f.value.document_id : null;
                        fieldsPayload.push({
                            template_field_id: fieldId,
                            text_value: null,
                            document_id: existingDocId
                        });
                    }
                } else {
                    // Preserve non-editable fields' current values
                    const val = f.value || { text_value: null, document_id: null };
                    fieldsPayload.push({
                        template_field_id: fieldId,
                        text_value: val.text_value,
                        document_id: val.document_id
                    });
                }
            });

            if (hasError) return;

            const saveBtn = document.getElementById(`save-step-btn-${stepId}`);
            saveBtn.disabled = true;
            saveBtn.textContent = 'Saving...';

            const payload = {
                process_id: currentProcessDetails.process_id,
                template_step_id: stepId,
                fields: fieldsPayload
            };

            try {
                const response = await fetch(`${API_BASE}/api/v1/process/save-step`, {
                    method: 'POST',
                    headers: {
                        'Authorization': `Bearer ${token}`,
                        'Content-Type': 'application/json'
                    },
                    body: JSON.stringify(payload)
                });

                if (response.ok) {
                    alert('Step inputs saved successfully!');
                    await viewProcessDetail(currentProcessDetails.process_id);
                } else {
                    const errText = await response.text();
                    alert(`Failed to save step inputs: ${errText}`);
                }
            } catch(e) {
                console.error(e);
                alert('Connection error while saving step.');
            } finally {
                saveBtn.disabled = false;
                saveBtn.textContent = 'Save Step Inputs';
            }
        }

        // Submit administrative approval or rejection for a step
        async function submitAdminReview(stepId, isApproved) {
            const notesEl = document.getElementById(`review-notes-${stepId}`);
            const notes = notesEl ? notesEl.value.trim() : '';

            const payload = {
                process_id: currentProcessDetails.process_id,
                template_step_id: stepId,
                is_approved: isApproved,
                notes: notes ? notes : null
            };

            try {
                const response = await fetch(`${API_BASE}/api/v1/admin/process/review`, {
                    method: 'POST',
                    headers: {
                        'Authorization': `Bearer ${token}`,
                        'Content-Type': 'application/json'
                    },
                    body: JSON.stringify(payload)
                });

                if (response.ok) {
                    alert(`Step review submitted successfully: ${isApproved ? 'Approved' : 'Rejected'}`);
                    await viewProcessDetail(currentProcessDetails.process_id);
                } else {
                    const errText = await response.text();
                    alert(`Failed to submit review: ${errText}`);
                }
            } catch(e) {
                console.error(e);
                alert('Connection error while submitting step review.');
            }
        }

        // Submit administrative revoke for an already completed step
        async function submitAdminRevoke(stepId) {
            const notesEl = document.getElementById(`review-notes-${stepId}`);
            const notes = notesEl ? notesEl.value.trim() : '';

            if (!confirm('Are you sure you want to reopen this step? It will allow the resident to modify documents.')) {
                return;
            }

            const payload = {
                process_id: currentProcessDetails.process_id,
                template_step_id: stepId,
                notes: notes ? notes : null
            };

            try {
                const response = await fetch(`${API_BASE}/api/v1/admin/process/revoke`, {
                    method: 'POST',
                    headers: {
                        'Authorization': `Bearer ${token}`,
                        'Content-Type': 'application/json'
                    },
                    body: JSON.stringify(payload)
                });

                if (response.ok) {
                    alert('Step has been successfully reopened for resident action.');
                    await viewProcessDetail(currentProcessDetails.process_id);
                } else {
                    const errText = await response.text();
                    alert(`Failed to reopen step: ${errText}`);
                }
            } catch(e) {
                console.error(e);
                alert('Connection error while revoking step.');
            }
        }

        // Recursive search for step in the templates tree
        function findStepInTree(stepsList, stepId) {
            for (let s of stepsList) {
                if (s.template_step_id === stepId) return s;
                if (s.substeps && s.substeps.length > 0) {
                    const found = findStepInTree(s.substeps, stepId);
                    if (found) return found;
                }
            }
            return null;
        }

        function toggleReviewHistory() {
            const container = document.getElementById('review-history-container');
            const btn = document.getElementById('toggle-history-btn');
            if (!container || !btn) return;
            if (container.style.display === 'none') {
                container.style.display = 'block';
                btn.textContent = 'Hide Review History';
            } else {
                container.style.display = 'none';
                btn.textContent = 'View Review History';
            }
        }

        // ==========================================
        // DOCUMENT PICKER MODAL
        // ==========================================
        let currentPickerStepId = null;
        let currentPickerFieldId = null;

        async function openDocumentPickerModal(stepId, fieldId) {
            currentPickerStepId = stepId;
            currentPickerFieldId = fieldId;
            document.getElementById('docPickerModal').style.display = 'flex';
            document.getElementById('picker-new-upload').value = '';
            document.getElementById('picker-existing-docs').innerHTML = '<p>Loading documents...</p>';
            
            try {
                const isCurrentAdmin = user.roles && (user.roles.includes('admin') || user.roles.includes('Admin'));
                const url = isCurrentAdmin ? `${API_BASE}/api/v1/admin/documents/list` : `${API_BASE}/api/v1/documents`;
                
                const response = await fetch(url, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}` }
                });
                
                if (response.ok) {
                    const docs = await response.json();
                    const listHtml = docs.map(d => {
                        const doc = d.document || d;
                        return `
                            <div style="padding: 12px; border: 1px solid var(--border); border-radius: var(--radius-md); margin-bottom: 8px; display: flex; justify-content: space-between; align-items: center; background: var(--background);">
                                <div>
                                    <strong style="display: block; font-size: 0.9375rem;">${escapeHtml(doc.title)}</strong>
                                    <span style="font-size: 0.8125rem; color: var(--muted-foreground);">${formatDateMinimal(doc.uploaded_date)}</span>
                                </div>
                                <button type="button" class="btn btn-secondary" style="padding: 6px 12px; font-size: 0.75rem;" onclick="selectExistingDocument('${doc.document_id}')">Select</button>
                            </div>
                        `;
                    }).join('');
                    
                    document.getElementById('picker-existing-docs').innerHTML = listHtml || '<p style="color: var(--muted-foreground); font-size: 0.875rem;">No documents found in your library.</p>';
                } else {
                    document.getElementById('picker-existing-docs').innerHTML = '<p style="color: var(--danger);">Failed to load documents.</p>';
                }
            } catch (e) {
                console.error(e);
                document.getElementById('picker-existing-docs').innerHTML = '<p style="color: var(--danger);">Connection error.</p>';
            }
        }

        function closeDocumentPickerModal() {
            document.getElementById('docPickerModal').style.display = 'none';
        }

        async function uploadNewDocumentFromPicker() {
            const fileInput = document.getElementById('picker-new-upload');
            if (!fileInput.files || fileInput.files.length === 0) {
                alert('Please select a file to upload.');
                return;
            }
            const file = fileInput.files[0];
            
            // Re-use existing upload logic
            document.getElementById('docPickerModal').style.display = 'none';
            const mockInput = document.createElement('input');
            mockInput.type = 'file';
            // We just call the logic
            const formData = new FormData();
            formData.append('process_id', currentProcessDetails.process_id);
            formData.append('title', file.name);
            formData.append('file', file);
            
            const progressEl = document.getElementById(`upload-progress-${currentPickerFieldId}`);
            if(progressEl) progressEl.textContent = 'Uploading...';
            
            try {
                const response = await fetch(`${API_BASE}/api/v1/process/upload`, {
                    method: 'POST',
                    headers: { 'Authorization': `Bearer ${token}` },
                    body: formData
                });
                if (response.ok) {
                    const docObj = await response.json();
                    const docId = docObj.document ? docObj.document.document_id : docObj.document_id;
                    await attachDocumentToField(currentPickerStepId, currentPickerFieldId, docId, progressEl);
                } else {
                    alert('Upload failed: ' + await response.text());
                    if(progressEl) progressEl.textContent = 'Failed';
                }
            } catch (e) {
                alert('Connection error during upload.');
                if(progressEl) progressEl.textContent = 'Failed';
            }
        }

        async function selectExistingDocument(documentId) {
            document.getElementById('docPickerModal').style.display = 'none';
            const progressEl = document.getElementById(`upload-progress-${currentPickerFieldId}`);
            if(progressEl) progressEl.textContent = 'Linking...';
            await attachDocumentToField(currentPickerStepId, currentPickerFieldId, documentId, progressEl);
        }

        async function attachDocumentToField(stepId, fieldId, documentId, progressEl) {
            const payload = {
                process_id: currentProcessDetails.process_id,
                template_step_id: stepId,
                fields: [{
                    template_field_id: fieldId,
                    text_value: null,
                    document_id: documentId
                }]
            };
            try {
                const saveRes = await fetch(`${API_BASE}/api/v1/process/save-step`, {
                    method: 'POST',
                    headers: {
                        'Authorization': `Bearer ${token}`,
                        'Content-Type': 'application/json'
                    },
                    body: JSON.stringify(payload)
                });
                if (saveRes.ok) {
                    if(progressEl) { progressEl.style.color = 'var(--secondary)'; progressEl.textContent = 'Success!'; }
                    await viewProcessDetail(currentProcessDetails.process_id);
                } else {
                    alert('Failed to save step: ' + await saveRes.text());
                    if(progressEl) { progressEl.style.color = 'var(--danger)'; progressEl.textContent = 'Failed'; }
                }
            } catch (e) {
                alert('Connection error.');
                if(progressEl) { progressEl.style.color = 'var(--danger)'; progressEl.textContent = 'Failed'; }
            }
        }

        // Initial Load
        loadProcesses();
    </script>

    <!-- Document Picker Modal -->
    <div id="docPickerModal" style="display: none; position: fixed; inset: 0; background: rgba(0,0,0,0.5); z-index: 1000; justify-content: center; align-items: center;">
        <div style="background: var(--background); width: 100%; max-width: 600px; max-height: 85vh; border-radius: var(--radius-lg); display: flex; flex-direction: column; overflow: hidden; box-shadow: 0 10px 25px rgba(0,0,0,0.1);">
            <div style="padding: 20px; border-bottom: 1px solid var(--border); display: flex; justify-content: space-between; align-items: center;">
                <h3 style="margin: 0; font-size: 1.25rem;">Choose Document</h3>
                <button type="button" onclick="closeDocumentPickerModal()" style="background: none; border: none; font-size: 1.5rem; cursor: pointer; color: var(--muted-foreground);">&times;</button>
            </div>
            
            <div style="padding: 20px; overflow-y: auto;">
                <h4 style="margin: 0 0 12px 0; font-size: 1rem;">Upload New</h4>
                <div style="display: flex; gap: 12px; margin-bottom: 32px;">
                    <input type="file" id="picker-new-upload" style="flex: 1; border: 1px solid var(--border); border-radius: var(--radius-md); padding: 8px;">
                    <button type="button" class="btn btn-primary" onclick="uploadNewDocumentFromPicker()">Upload & Select</button>
                </div>
                
                <h4 style="margin: 0 0 12px 0; font-size: 1rem;">Select from My Library</h4>
                <div id="picker-existing-docs" style="display: flex; flex-direction: column; gap: 8px;">
                    <!-- Populated dynamically -->
                </div>
            </div>
        </div>
    </div>
</body>
</html>
"#;
