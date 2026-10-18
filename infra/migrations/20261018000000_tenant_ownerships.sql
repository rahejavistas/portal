-- A flat's current (and historical) tenant, mapped by the owner. At most one active
-- tenant per ownership at a time; is_active = FALSE rows are history.
CREATE TABLE IF NOT EXISTS tenant_ownerships (
    tenant_ownership_id SERIAL PRIMARY KEY,
    ownership_id INT NOT NULL REFERENCES flat_ownerships(ownership_id) ON DELETE CASCADE,
    tenant_user_id INT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    expires_at DATE NOT NULL,
    rental_agreement_document_id UUID REFERENCES documents(document_id) ON DELETE SET NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_by INT REFERENCES users(user_id) ON DELETE SET NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    ended_at TIMESTAMP WITH TIME ZONE
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_tenant_ownerships_active_per_ownership
ON tenant_ownerships (ownership_id)
WHERE (is_active = TRUE);

CREATE INDEX IF NOT EXISTS idx_tenant_ownerships_tenant_user
ON tenant_ownerships (tenant_user_id) WHERE (is_active = TRUE);
