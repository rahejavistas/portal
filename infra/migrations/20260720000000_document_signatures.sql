-- E-signature tracking: which documents need signature is declared per-field in the
-- process template (`requires_signature`), and a requirement is auto-provisioned the
-- moment such a field's document is saved — no admin action needed. Signing = uploading
-- the signed file as a NEW document; the original is never touched, and this table's
-- document_id -> signed_document_id pair is the old-to-new map.

CREATE TYPE signature_status AS ENUM ('PENDING', 'COMPLETED', 'CANCELLED');
CREATE TYPE signature_event_source AS ENUM ('internal', 'external');

CREATE TABLE IF NOT EXISTS document_signature_requirements (
    requirement_id SERIAL PRIMARY KEY,
    document_id UUID NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
    signed_document_id UUID REFERENCES documents(document_id) ON DELETE SET NULL,
    process_id UUID NOT NULL REFERENCES processes(process_id) ON DELETE CASCADE,
    template_step_id INT NOT NULL,
    template_field_id INT NOT NULL,
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

-- One active (PENDING or COMPLETED) requirement per document — auto-provisioning is
-- idempotent, and cancelling frees the document up to be re-flagged.
CREATE UNIQUE INDEX IF NOT EXISTS idx_doc_sig_req_one_active_per_document
    ON document_signature_requirements(document_id)
    WHERE (status <> 'CANCELLED');

CREATE INDEX IF NOT EXISTS idx_doc_sig_req_status ON document_signature_requirements(status);

CREATE TABLE IF NOT EXISTS document_signature_events (
    event_id SERIAL PRIMARY KEY,
    requirement_id INT NOT NULL REFERENCES document_signature_requirements(requirement_id) ON DELETE CASCADE,
    source signature_event_source NOT NULL,
    event_type VARCHAR(64) NOT NULL,
    raw_payload JSONB,
    actor_user_id INT REFERENCES users(user_id) ON DELETE SET NULL,
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_sig_events_requirement ON document_signature_events(requirement_id);
