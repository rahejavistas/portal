-- One optional attachment per internal note, kept out of `documents` so no document list shows it.
-- comment_id stays NULL until the note is saved.
CREATE TABLE IF NOT EXISTS process_comment_attachments (
    attachment_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    process_id UUID NOT NULL REFERENCES processes(process_id) ON DELETE CASCADE,
    template_step_id INT NOT NULL,
    uploaded_by INT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    file_name VARCHAR(255) NOT NULL,
    content_type VARCHAR(255) NOT NULL,
    comment_id UUID UNIQUE REFERENCES process_reviewer_comments(comment_id) ON DELETE CASCADE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_comment_attachments_process ON process_comment_attachments(process_id);
