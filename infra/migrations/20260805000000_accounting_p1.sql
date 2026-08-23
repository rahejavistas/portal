-- P1 accounting: fund contributions, TDS, vendors/expenses, bank reconciliation, budgets.

-- ============ Fund contributions (sinking/repair) ============

CREATE TABLE IF NOT EXISTS fund_contribution_rules (
    rule_id SERIAL PRIMARY KEY,
    fund_id INT NOT NULL REFERENCES funds(fund_id),
    contribution_type VARCHAR(20) NOT NULL, -- 'FIXED' or 'PCT_OF_MAINTENANCE'
    amount_or_pct NUMERIC(10,2) NOT NULL,   -- CA/bylaws-defined minimum, never hardcoded
    effective_from DATE NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE
);

CREATE TABLE IF NOT EXISTS flat_fund_assignments (
    flat_id INT NOT NULL REFERENCES flats(flat_id),
    fund_id INT NOT NULL REFERENCES funds(fund_id),
    PRIMARY KEY (flat_id, fund_id)
);

-- ============ TDS on vendor/contractor payments ============

CREATE TABLE IF NOT EXISTS tds_rules (
    tds_rule_id SERIAL PRIMARY KEY,
    section VARCHAR(16) NOT NULL,          -- '194C', '194J', '194I', ...
    payment_category VARCHAR(64) NOT NULL, -- 'contractor', 'professional_fees', 'rent', ...
    rate_pct NUMERIC(5,2) NOT NULL,
    threshold_single_payment NUMERIC(12,2) NOT NULL,
    threshold_annual_aggregate NUMERIC(12,2) NOT NULL,
    effective_from DATE NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_by INT NOT NULL REFERENCES users(user_id)
);

-- ============ Vendors, purchase orders, expenses ============

CREATE TABLE IF NOT EXISTS vendors (
    vendor_id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    category VARCHAR(64) NOT NULL, -- security, housekeeping, electricity, repairs, admin...
    gstin VARCHAR(15),
    pan VARCHAR(10),
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS purchase_orders (
    po_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    po_number VARCHAR(32) NOT NULL UNIQUE,
    vendor_id INT NOT NULL REFERENCES vendors(vendor_id),
    description TEXT NOT NULL,
    amount NUMERIC(12,2) NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'DRAFT', -- DRAFT, ISSUED, FULFILLED, CANCELLED
    created_by INT NOT NULL REFERENCES users(user_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE SEQUENCE IF NOT EXISTS po_number_seq START 1;

-- Vendor bill entry with its own lightweight approval status, not the process engine.
CREATE TABLE IF NOT EXISTS vendor_expenses (
    expense_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    po_id UUID REFERENCES purchase_orders(po_id),
    vendor_id INT NOT NULL REFERENCES vendors(vendor_id),
    account_id INT NOT NULL REFERENCES chart_of_accounts(account_id), -- expense category
    vendor_invoice_number VARCHAR(64),
    invoice_document_id UUID REFERENCES documents(document_id),
    amount NUMERIC(12,2) NOT NULL,
    tds_amount NUMERIC(12,2) NOT NULL DEFAULT 0,
    net_payable NUMERIC(12,2) NOT NULL,
    status VARCHAR(20) NOT NULL DEFAULT 'PENDING_APPROVAL', -- PENDING_APPROVAL, APPROVED, REJECTED
    ledger_transaction_id UUID REFERENCES ledger_transactions(transaction_id), -- set once APPROVED
    created_by INT NOT NULL REFERENCES users(user_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    reviewed_by INT REFERENCES users(user_id),
    reviewed_at TIMESTAMPTZ,
    rejection_reason TEXT
);

CREATE INDEX IF NOT EXISTS idx_vendor_expenses_status ON vendor_expenses(status) WHERE status = 'PENDING_APPROVAL';
CREATE INDEX IF NOT EXISTS idx_vendor_expenses_vendor ON vendor_expenses(vendor_id);

CREATE TABLE IF NOT EXISTS tds_records (
    tds_record_id SERIAL PRIMARY KEY,
    expense_id UUID NOT NULL REFERENCES vendor_expenses(expense_id),
    tds_rule_id INT NOT NULL REFERENCES tds_rules(tds_rule_id),
    vendor_id INT NOT NULL REFERENCES vendors(vendor_id),
    fiscal_year_id INT NOT NULL REFERENCES fiscal_years(fiscal_year_id),
    gross_amount NUMERIC(12,2) NOT NULL,
    tds_amount NUMERIC(12,2) NOT NULL,
    deducted_on DATE NOT NULL,
    -- Filled in once the CA/accountant actually remits the TDS — this system computes
    -- and tracks Form 16A source data, it never e-files.
    deposited_challan_number VARCHAR(64),
    deposited_at DATE
);

CREATE INDEX IF NOT EXISTS idx_tds_records_vendor_fy ON tds_records(vendor_id, fiscal_year_id);

-- ============ Bank reconciliation ============

CREATE TABLE IF NOT EXISTS bank_statement_imports (
    import_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    imported_by INT NOT NULL REFERENCES users(user_id),
    source_filename VARCHAR(255),
    imported_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS bank_statement_lines (
    line_id BIGSERIAL PRIMARY KEY,
    import_id UUID NOT NULL REFERENCES bank_statement_imports(import_id),
    transaction_date DATE NOT NULL,
    description TEXT NOT NULL,
    amount NUMERIC(12,2) NOT NULL, -- signed: +credit / -debit, as printed on the statement
    matched_ledger_transaction_id UUID REFERENCES ledger_transactions(transaction_id),
    match_status VARCHAR(16) NOT NULL DEFAULT 'UNMATCHED' -- UNMATCHED, MATCHED, IGNORED
);

CREATE INDEX IF NOT EXISTS idx_bank_lines_unmatched ON bank_statement_lines(match_status) WHERE match_status = 'UNMATCHED';
CREATE INDEX IF NOT EXISTS idx_bank_lines_import ON bank_statement_lines(import_id);

-- A ledger transaction can be claimed by at most one bank line — without this, two
-- different statement lines could both be manually matched to the same transaction,
-- silently breaking reconciliation with no error.
CREATE UNIQUE INDEX IF NOT EXISTS idx_bank_lines_one_match_per_transaction
    ON bank_statement_lines(matched_ledger_transaction_id) WHERE matched_ledger_transaction_id IS NOT NULL;

-- ============ Budgets ============

CREATE TABLE IF NOT EXISTS budgets (
    budget_id SERIAL PRIMARY KEY,
    fiscal_year_id INT NOT NULL REFERENCES fiscal_years(fiscal_year_id),
    created_by INT NOT NULL REFERENCES users(user_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (fiscal_year_id)
);

CREATE TABLE IF NOT EXISTS budget_lines (
    budget_line_id SERIAL PRIMARY KEY,
    budget_id INT NOT NULL REFERENCES budgets(budget_id),
    account_id INT NOT NULL REFERENCES chart_of_accounts(account_id),
    budgeted_amount NUMERIC(12,2) NOT NULL,
    UNIQUE (budget_id, account_id)
);

-- ============ Fiscal year close: track the closing entry so a reopen can reverse it ============

ALTER TABLE fiscal_years ADD COLUMN IF NOT EXISTS closing_transaction_id UUID REFERENCES ledger_transactions(transaction_id);

-- ============ New chart-of-accounts rows this phase needs ============

INSERT INTO chart_of_accounts (code, name, account_type, normal_balance, is_system) VALUES
    ('2400', 'Member Advance / Credit Balance', 'LIABILITY', 'CREDIT', TRUE),
    ('3000', 'Accumulated Surplus (Retained Earnings)', 'EQUITY', 'CREDIT', TRUE)
ON CONFLICT (code) DO NOTHING;

-- All P1 features gate on the same accounting.manage/accounting.view from P0.
