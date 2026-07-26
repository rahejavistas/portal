-- E-signature tracking. Signers are a curated directory (`document_signers`), each row
-- mapped to one user account with the contact details an e-signature provider needs
-- (name/phone/email) — kept separate from `users` since not every user is a signer, and
-- a user's signer contact info can differ from their login profile.
--
-- A signature requirement is NOT declared in the process template — a process reviewer
-- attaches one or more signers to a specific document from the process review screen
-- (see `POST /api/v1/admin/process/documents/add-signers`), in the exact order they
-- should sign (`sign_order`) — mirrors e-signature providers like SignYu, which sign
-- documents in the order signers were added. A requirement only becomes actionable
-- once every lower-`sign_order` requirement for the same document is COMPLETED. The
-- field stays not-filled until every attached signer's requirement is COMPLETED.
-- Signing = uploading the signed file as a NEW document; the original is never
-- touched, and this table's document_id -> signed_document_id pair is the old-to-new
-- map. There is deliberately no CANCELLED status and no way to remove a requirement
-- once attached — a required signature is required, full stop.

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
    -- Position in the signing chain for this document (1-based, assigned in the order
    -- signers were attached). Lower orders must COMPLETE before a higher order becomes
    -- actionable.
    sign_order INT NOT NULL,
    -- Which provider is handling this requirement ('manual', or a registered external
    -- provider name like 'signyu'). Provider-specific correlation data (e.g. SignYu's
    -- own document/signer ids) lives in that provider's own table, not here — see
    -- `signyu_requirement_links` below — so this table stays provider-agnostic.
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

-- No separate "requirement history" table — `document_signature_requirements` itself
-- always holds the current (and only) state a requirement can be in, and every
-- external provider interaction is already fully logged in `signature_provider_requests`
-- below. A dedicated event-log table would just duplicate one or the other.

-- Full audit trail of every request/response exchanged with an external signature
-- provider (e.g. SignYu) — "plug and play" providers all log through this same table,
-- keyed by `provider`, so adding a new provider tomorrow needs no schema change here.
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
CREATE INDEX IF NOT EXISTS idx_sig_provider_requests_provider ON signature_provider_requests(provider);

-- SignYu-specific correlation data for a requirement handled by that provider — kept
-- out of `document_signature_requirements` so the shared table stays provider-agnostic.
-- A second external provider gets its own equivalent table, not new columns here.
CREATE TABLE IF NOT EXISTS signyu_requirement_links (
    requirement_id INT PRIMARY KEY REFERENCES document_signature_requirements(requirement_id) ON DELETE CASCADE,
    external_document_id VARCHAR(128) NOT NULL,
    external_signer_id VARCHAR(128) NOT NULL,
    -- SignYu's own signing-page link for this signer, so our UI can offer "open signing
    -- page" directly instead of the signer having to dig through SignYu's email.
    sign_url TEXT
);

CREATE INDEX IF NOT EXISTS idx_signyu_links_external_document ON signyu_requirement_links(external_document_id);
