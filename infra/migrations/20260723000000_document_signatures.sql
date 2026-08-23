-- E-signature tracking. document_signers is a curated signer directory, one row per
-- user account. Signers are attached to a document at review time, not in the
-- template, in sign_order; signing uploads a new document, the original is untouched.
CREATE TYPE signature_status AS ENUM ('PENDING', 'COMPLETED');

CREATE TABLE IF NOT EXISTS document_signers (
    signer_id SERIAL PRIMARY KEY,
    user_id INT NOT NULL UNIQUE REFERENCES users(user_id) ON DELETE CASCADE,
    name VARCHAR(255) NOT NULL,
    phone VARCHAR(20) NOT NULL,
    email VARCHAR(255) NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_by INT REFERENCES users(user_id) ON DELETE SET NULL,
    CONSTRAINT chk_document_signers_phone_digits CHECK (phone ~ '^[0-9]+$')
);

CREATE TABLE IF NOT EXISTS document_signature_requirements (
    requirement_id SERIAL PRIMARY KEY,
    document_id UUID NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
    signed_document_id UUID REFERENCES documents(document_id) ON DELETE SET NULL,
    process_id UUID NOT NULL REFERENCES processes(process_id) ON DELETE CASCADE,
    template_step_id INT NOT NULL,
    template_field_id INT NOT NULL,
    signer_id INT NOT NULL REFERENCES document_signers(signer_id) ON DELETE CASCADE,
    -- 1-based signing-chain position; lower orders must COMPLETE first.
    sign_order INT NOT NULL,
    -- 'manual' or a registered external provider name (e.g. 'signyu').
    provider VARCHAR(32) NOT NULL DEFAULT 'manual',
    status signature_status NOT NULL DEFAULT 'PENDING',
    signed_by INT REFERENCES users(user_id) ON DELETE SET NULL,
    signed_at TIMESTAMP WITH TIME ZONE,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    CONSTRAINT chk_doc_sig_req_completed_fields CHECK (
        status <> 'COMPLETED' OR (signed_document_id IS NOT NULL AND signed_by IS NOT NULL AND signed_at IS NOT NULL)
    )
);

-- One requirement per (document, signer), ever — attaching the same signer twice is
-- idempotent (see the ON CONFLICT DO NOTHING in signature_add_signers_to_document).
CREATE UNIQUE INDEX IF NOT EXISTS idx_doc_sig_req_one_per_document_signer
    ON document_signature_requirements(document_id, signer_id);

CREATE INDEX IF NOT EXISTS idx_doc_sig_req_status ON document_signature_requirements(status);

CREATE INDEX IF NOT EXISTS idx_doc_sig_req_document_sign_order ON document_signature_requirements(document_id, sign_order);

-- Every request/response exchanged with an external signature provider, keyed by provider.
CREATE TABLE IF NOT EXISTS signature_provider_requests (
    log_id SERIAL PRIMARY KEY,
    provider VARCHAR(32) NOT NULL,
    requirement_id INT REFERENCES document_signature_requirements(requirement_id) ON DELETE SET NULL,
    direction VARCHAR(16) NOT NULL, -- 'outbound' (we called them) or 'inbound' (webhook)
    endpoint VARCHAR(255) NOT NULL,
    method VARCHAR(8) NOT NULL,
    request_body JSONB,
    response_status INT,
    response_body JSONB,
    error TEXT,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_sig_provider_requests_requirement ON signature_provider_requests(requirement_id);

-- SignYu-specific correlation data — a second provider gets its own table, not new columns here.
CREATE TABLE IF NOT EXISTS signyu_requirement_links (
    requirement_id INT PRIMARY KEY REFERENCES document_signature_requirements(requirement_id) ON DELETE CASCADE,
    external_document_id VARCHAR(128) NOT NULL,
    external_signer_id VARCHAR(128) NOT NULL,
    sign_url TEXT
);

CREATE INDEX IF NOT EXISTS idx_signyu_links_external_document ON signyu_requirement_links(external_document_id);
