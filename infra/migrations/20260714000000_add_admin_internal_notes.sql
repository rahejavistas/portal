-- Add standalone admin-only internal notes to the process review system.
--
-- `process_admin_comments` — standalone notes admins can drop on any
-- step at any time, without needing to approve or reject.
CREATE TABLE IF NOT EXISTS process_admin_comments (
    comment_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    process_id UUID NOT NULL REFERENCES processes(process_id) ON DELETE CASCADE,
    template_step_id INT NOT NULL,
    admin_id INT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    comment TEXT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_admin_comments_process
    ON process_admin_comments(process_id);
