-- Append-only audit trail for permission/role/document-category-access changes.
-- Column-level created_by/granted_by/updated_by only ever show the latest state;
-- this table keeps the full history for future UI exposure.
CREATE TABLE IF NOT EXISTS permission_log (
    log_id SERIAL PRIMARY KEY,
    description TEXT NOT NULL,
    done_by INT REFERENCES users(user_id) ON DELETE SET NULL,
    done_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_permission_log_done_at ON permission_log(done_at DESC);
