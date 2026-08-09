-- P0 accounting: chart of accounts, double-entry ledger, billing, GST, interest, payments, receipts.

CREATE TYPE account_type AS ENUM ('ASSET', 'LIABILITY', 'EQUITY', 'INCOME', 'EXPENSE');
CREATE TYPE normal_balance AS ENUM ('DEBIT', 'CREDIT');

CREATE TABLE IF NOT EXISTS funds (
    fund_id SERIAL PRIMARY KEY,
    code VARCHAR(32) NOT NULL UNIQUE,
    name VARCHAR(255) NOT NULL,
    is_restricted BOOLEAN NOT NULL DEFAULT FALSE,
    min_contribution_rule JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS chart_of_accounts (
    account_id SERIAL PRIMARY KEY,
    code VARCHAR(20) NOT NULL UNIQUE,
    name VARCHAR(255) NOT NULL,
    account_type account_type NOT NULL,
    normal_balance normal_balance NOT NULL,
    parent_account_id INT REFERENCES chart_of_accounts(account_id),
    fund_id INT REFERENCES funds(fund_id),
    is_system BOOLEAN NOT NULL DEFAULT FALSE,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_by INT REFERENCES users(user_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS fiscal_years (
    fiscal_year_id SERIAL PRIMARY KEY,
    label VARCHAR(16) NOT NULL UNIQUE,
    start_date DATE NOT NULL,
    end_date DATE NOT NULL,
    status VARCHAR(16) NOT NULL DEFAULT 'OPEN',
    closed_at TIMESTAMPTZ,
    closed_by INT REFERENCES users(user_id),
    UNIQUE (start_date, end_date)
);

-- Explicit reopen-with-reason trail so a closed FY is never silently mutable again.
CREATE TABLE IF NOT EXISTS fiscal_year_reopenings (
    reopening_id SERIAL PRIMARY KEY,
    fiscal_year_id INT NOT NULL REFERENCES fiscal_years(fiscal_year_id),
    reopened_by INT NOT NULL REFERENCES users(user_id),
    reason TEXT NOT NULL,
    reopened_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    reclosed_at TIMESTAMPTZ
);

-- Double-entry ledger. Append-only: corrections post a new reversing transaction,
-- never mutate a posted one — enforced both in application code (post_transaction
-- is the only writer) and at the DB level via the rules below.
CREATE TABLE IF NOT EXISTS ledger_transactions (
    transaction_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    fiscal_year_id INT NOT NULL REFERENCES fiscal_years(fiscal_year_id),
    transaction_date DATE NOT NULL,
    description TEXT NOT NULL,
    source_type VARCHAR(32) NOT NULL,
    source_id UUID,
    reverses_transaction_id UUID REFERENCES ledger_transactions(transaction_id),
    created_by INT NOT NULL REFERENCES users(user_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS ledger_entries (
    entry_id BIGSERIAL PRIMARY KEY,
    transaction_id UUID NOT NULL REFERENCES ledger_transactions(transaction_id),
    account_id INT NOT NULL REFERENCES chart_of_accounts(account_id),
    flat_id INT REFERENCES flats(flat_id),
    vendor_id INT,
    debit NUMERIC(14,2) NOT NULL DEFAULT 0 CHECK (debit >= 0),
    credit NUMERIC(14,2) NOT NULL DEFAULT 0 CHECK (credit >= 0),
    CONSTRAINT chk_entry_one_sided CHECK (
        (debit > 0 AND credit = 0) OR (debit = 0 AND credit > 0)
    )
);

CREATE INDEX IF NOT EXISTS idx_ledger_entries_transaction ON ledger_entries(transaction_id);
CREATE INDEX IF NOT EXISTS idx_ledger_entries_account ON ledger_entries(account_id);
CREATE INDEX IF NOT EXISTS idx_ledger_entries_flat ON ledger_entries(flat_id) WHERE flat_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_ledger_transactions_fy ON ledger_transactions(fiscal_year_id);
CREATE INDEX IF NOT EXISTS idx_ledger_transactions_source ON ledger_transactions(source_type, source_id);

CREATE RULE ledger_entries_no_update AS ON UPDATE TO ledger_entries DO INSTEAD NOTHING;
CREATE RULE ledger_entries_no_delete AS ON DELETE TO ledger_entries DO INSTEAD NOTHING;
CREATE RULE ledger_transactions_no_update AS ON UPDATE TO ledger_transactions DO INSTEAD NOTHING;
CREATE RULE ledger_transactions_no_delete AS ON DELETE TO ledger_transactions DO INSTEAD NOTHING;

-- Billing plans & assignments.

CREATE TABLE IF NOT EXISTS billing_plans (
    billing_plan_id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    billing_type VARCHAR(16) NOT NULL, -- 'FLAT_RATE', 'PER_SQFT', 'SLAB'
    config JSONB NOT NULL,
    frequency VARCHAR(16) NOT NULL DEFAULT 'MONTHLY', -- MONTHLY, QUARTERLY
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_by INT NOT NULL REFERENCES users(user_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS flat_billing_assignments (
    flat_id INT PRIMARY KEY REFERENCES flats(flat_id),
    billing_plan_id INT NOT NULL REFERENCES billing_plans(billing_plan_id),
    effective_from DATE NOT NULL DEFAULT CURRENT_DATE,
    assigned_by INT NOT NULL REFERENCES users(user_id)
);

-- GST rule set — CA-configured, never hardcoded (see rates flagged for sign-off).
CREATE TABLE IF NOT EXISTS gst_rules (
    gst_rule_id SERIAL PRIMARY KEY,
    per_member_monthly_exempt_threshold NUMERIC(10,2) NOT NULL,
    society_turnover_registration_threshold NUMERIC(14,2) NOT NULL,
    gst_rate_pct NUMERIC(5,2) NOT NULL,
    hsn_sac_code VARCHAR(16) NOT NULL,
    society_gstin VARCHAR(15),
    effective_from DATE NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_by INT NOT NULL REFERENCES users(user_id)
);

CREATE TABLE IF NOT EXISTS invoices (
    invoice_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    invoice_number VARCHAR(32) NOT NULL UNIQUE,
    flat_id INT NOT NULL REFERENCES flats(flat_id),
    fiscal_year_id INT NOT NULL REFERENCES fiscal_years(fiscal_year_id),
    billing_period_start DATE NOT NULL,
    billing_period_end DATE NOT NULL,
    due_date DATE NOT NULL,
    subtotal NUMERIC(12,2) NOT NULL,
    gst_amount NUMERIC(12,2) NOT NULL DEFAULT 0,
    total_amount NUMERIC(12,2) NOT NULL,
    amount_paid NUMERIC(12,2) NOT NULL DEFAULT 0,
    status VARCHAR(16) NOT NULL DEFAULT 'UNPAID', -- UNPAID, PARTIALLY_PAID, PAID, VOID
    gstin_applicable BOOLEAN NOT NULL DEFAULT FALSE,
    ledger_transaction_id UUID REFERENCES ledger_transactions(transaction_id),
    created_by INT NOT NULL REFERENCES users(user_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    voided_at TIMESTAMPTZ,
    void_reason TEXT
);

CREATE TABLE IF NOT EXISTS invoice_line_items (
    line_item_id SERIAL PRIMARY KEY,
    invoice_id UUID NOT NULL REFERENCES invoices(invoice_id),
    description VARCHAR(255) NOT NULL,
    account_id INT NOT NULL REFERENCES chart_of_accounts(account_id),
    hsn_sac_code VARCHAR(16),
    amount NUMERIC(12,2) NOT NULL,
    -- Rate actually applied at posting time, not just the resulting amount — so a GST
    -- summary report stays correct even after gst_rules.gst_rate_pct later changes.
    gst_rate_pct NUMERIC(5,2) NOT NULL DEFAULT 0,
    gst_amount NUMERIC(12,2) NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_invoices_flat ON invoices(flat_id);
CREATE INDEX IF NOT EXISTS idx_invoices_status ON invoices(status) WHERE status IN ('UNPAID', 'PARTIALLY_PAID');
CREATE UNIQUE INDEX IF NOT EXISTS idx_invoices_flat_period ON invoices(flat_id, billing_period_start) WHERE status != 'VOID';

CREATE SEQUENCE IF NOT EXISTS invoice_number_seq START 1;
CREATE SEQUENCE IF NOT EXISTS receipt_number_seq START 1;

-- Interest on overdue balances — rate/cap is CA/bylaws/state Act defined, never hardcoded.
CREATE TABLE IF NOT EXISTS interest_rules (
    interest_rule_id SERIAL PRIMARY KEY,
    annual_rate_pct NUMERIC(5,2) NOT NULL,
    grace_period_days INT NOT NULL DEFAULT 0,
    compounding VARCHAR(16) NOT NULL DEFAULT 'SIMPLE', -- SIMPLE or MONTHLY
    effective_from DATE NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_by INT NOT NULL REFERENCES users(user_id)
);

CREATE TABLE IF NOT EXISTS interest_charges (
    interest_charge_id SERIAL PRIMARY KEY,
    invoice_id UUID NOT NULL REFERENCES invoices(invoice_id),
    interest_rule_id INT NOT NULL REFERENCES interest_rules(interest_rule_id),
    overdue_amount NUMERIC(12,2) NOT NULL,
    days_overdue INT NOT NULL,
    interest_amount NUMERIC(12,2) NOT NULL,
    charged_date DATE NOT NULL,
    ledger_transaction_id UUID NOT NULL REFERENCES ledger_transactions(transaction_id)
);

-- One interest charge per invoice per day, so re-running the job is idempotent.
CREATE UNIQUE INDEX IF NOT EXISTS idx_interest_charges_invoice_date ON interest_charges(invoice_id, charged_date);

-- Payments, recorded manually by an admin/accountant from a bank statement, cheque,
-- UPI reference, or cash — no payment gateway integration exists or is planned.
CREATE TABLE IF NOT EXISTS payments (
    payment_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    invoice_id UUID NOT NULL REFERENCES invoices(invoice_id),
    amount NUMERIC(12,2) NOT NULL,
    payment_method VARCHAR(32) NOT NULL, -- 'bank_transfer', 'upi', 'cheque', 'cash', ...
    reference_number VARCHAR(128), -- UTR/cheque number/UPI ref — absent for cash
    recorded_by INT NOT NULL REFERENCES users(user_id),
    confirmed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    ledger_transaction_id UUID NOT NULL REFERENCES ledger_transactions(transaction_id)
);

-- Prevents the same bank/UPI/cheque reference from being recorded twice; cash payments
-- (no reference_number) are exempt since there's nothing to de-duplicate against.
CREATE UNIQUE INDEX IF NOT EXISTS idx_payments_reference
    ON payments(payment_method, reference_number) WHERE reference_number IS NOT NULL;

CREATE TABLE IF NOT EXISTS receipts (
    receipt_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    receipt_number VARCHAR(32) NOT NULL UNIQUE,
    payment_id UUID NOT NULL UNIQUE REFERENCES payments(payment_id),
    generated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- A resident self-reporting "I paid this, here's my transaction reference" for an
-- offline payment (UPI/bank transfer/cheque made outside the app). This is only a
-- claim — it never touches the ledger by itself. An admin reviews it against the
-- actual bank statement and either confirms it (which posts a real payment through
-- the same accounting::payments::record_payment path as an admin-entered payment) or
-- rejects it with a reason. Prevents a resident's unverified claim from ever being
-- treated as confirmed money.
CREATE TABLE IF NOT EXISTS payment_claims (
    claim_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    invoice_id UUID NOT NULL REFERENCES invoices(invoice_id),
    claimed_by INT NOT NULL REFERENCES users(user_id),
    amount NUMERIC(12,2) NOT NULL,
    payment_method VARCHAR(32) NOT NULL,
    reference_number VARCHAR(128) NOT NULL,
    note TEXT,
    status VARCHAR(16) NOT NULL DEFAULT 'PENDING', -- PENDING, CONFIRMED, REJECTED
    submitted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    reviewed_by INT REFERENCES users(user_id),
    reviewed_at TIMESTAMPTZ,
    review_note TEXT,
    -- Set once CONFIRMED — the real payment/receipt this claim resulted in.
    payment_id UUID REFERENCES payments(payment_id)
);

CREATE INDEX IF NOT EXISTS idx_payment_claims_invoice ON payment_claims(invoice_id);
CREATE INDEX IF NOT EXISTS idx_payment_claims_status ON payment_claims(status) WHERE status = 'PENDING';

-- Prevents duplicate PENDING claims for the same reference; reviewed claims free it up.
CREATE UNIQUE INDEX IF NOT EXISTS idx_payment_claims_pending_reference
    ON payment_claims(payment_method, reference_number) WHERE status = 'PENDING';

-- Society's own bank account details, printed on generated invoice/bill PDFs so
-- residents know where to NEFT/transfer payment. Single current row; admin-editable,
-- never hardcoded in application code.
CREATE TABLE IF NOT EXISTS payment_instructions (
    payment_instructions_id SERIAL PRIMARY KEY,
    account_holder_name VARCHAR(255) NOT NULL,
    bank_name VARCHAR(255) NOT NULL,
    account_number VARCHAR(64) NOT NULL,
    ifsc_code VARCHAR(11) NOT NULL,
    branch VARCHAR(255),
    contact_email VARCHAR(255),
    updated_by INT REFERENCES users(user_id),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- The society's own legal identity, printed on the header of every generated invoice
-- PDF (name, registration number, building address, phone). Single current row,
-- admin-editable, never hardcoded — GST registration number stays on gst_rules since
-- it's already tracked there.
-- `state` is one of India's 28 states / 8 union territories (a genuinely fixed,
-- non-admin-extensible set) — validated against that fixed list in the API layer, not
-- here. Knowing the state matters beyond the printed address: a future bylaws-reading
-- feature needs it to look up the right state's Co-operative Societies Act.
CREATE TABLE IF NOT EXISTS society_profile (
    society_profile_id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    registration_number VARCHAR(255),
    address VARCHAR(500) NOT NULL,
    state VARCHAR(100),
    phone VARCHAR(32),
    updated_by INT REFERENCES users(user_id),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Seed: funds, starter chart of accounts, current fiscal year.

INSERT INTO funds (code, name, is_restricted) VALUES
    ('OPERATING', 'Operating Fund', FALSE),
    ('SINKING', 'Sinking Fund', TRUE),
    ('REPAIR', 'Repair Fund', TRUE)
ON CONFLICT (code) DO NOTHING;

INSERT INTO chart_of_accounts (code, name, account_type, normal_balance, is_system) VALUES
    ('1000', 'Bank - Operating Account', 'ASSET', 'DEBIT', TRUE),
    ('1200', 'Member Dues Receivable', 'ASSET', 'DEBIT', TRUE),
    ('2100', 'GST Payable', 'LIABILITY', 'CREDIT', TRUE),
    ('2200', 'Vendor Payables', 'LIABILITY', 'CREDIT', TRUE),
    ('2300', 'TDS Payable', 'LIABILITY', 'CREDIT', TRUE),
    ('4000', 'Maintenance Income', 'INCOME', 'CREDIT', TRUE),
    ('4100', 'Late Payment Interest Income', 'INCOME', 'CREDIT', TRUE),
    ('4200', 'Miscellaneous Income', 'INCOME', 'CREDIT', TRUE),
    ('5100', 'Security Expense', 'EXPENSE', 'DEBIT', TRUE),
    ('5200', 'Housekeeping Expense', 'EXPENSE', 'DEBIT', TRUE),
    ('5300', 'Electricity Expense', 'EXPENSE', 'DEBIT', TRUE),
    ('5400', 'Repairs & Maintenance Expense', 'EXPENSE', 'DEBIT', TRUE),
    ('5500', 'Administrative Expense', 'EXPENSE', 'DEBIT', TRUE)
ON CONFLICT (code) DO NOTHING;

INSERT INTO fiscal_years (label, start_date, end_date)
SELECT
    'FY' || fy_start_year || '-' || RIGHT((fy_start_year + 1)::text, 2),
    make_date(fy_start_year, 4, 1),
    make_date(fy_start_year + 1, 3, 31)
FROM (
    SELECT CASE WHEN EXTRACT(MONTH FROM CURRENT_DATE) >= 4
                THEN EXTRACT(YEAR FROM CURRENT_DATE)::int
                ELSE EXTRACT(YEAR FROM CURRENT_DATE)::int - 1
           END AS fy_start_year
) t
ON CONFLICT DO NOTHING;

INSERT INTO permissions (code, label) VALUES
    ('accounting.manage', 'Manage Accounting'),
    ('accounting.view', 'View Accounting')
ON CONFLICT (code) DO NOTHING;

INSERT INTO designation_permissions (designation_id, permission_id)
SELECT d.designation_id, p.permission_id
FROM (VALUES
    ('super_admin', 'accounting.manage'),
    ('super_admin', 'accounting.view')
) AS seed(designation_code, permission_code)
JOIN designations d ON d.code = seed.designation_code
JOIN permissions p ON p.code = seed.permission_code
ON CONFLICT DO NOTHING;
