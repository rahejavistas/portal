-- Latest dues snapshot per flat, from an admin-uploaded file. One row per flat,
-- overwritten by each new upload; not derived from the accounting module at all.
CREATE TABLE IF NOT EXISTS flat_dues_from_file (
    flat_id INT PRIMARY KEY REFERENCES flats(flat_id) ON DELETE CASCADE,
    total_due NUMERIC(12,2) NOT NULL,
    overdue_amount NUMERIC(12,2) NOT NULL DEFAULT 0,
    as_of DATE NOT NULL,
    uploaded_by INT REFERENCES users(user_id) ON DELETE SET NULL,
    uploaded_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);
