CREATE TABLE IF NOT EXISTS processes (
    process_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id INT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    template_version_id INT NOT NULL REFERENCES process_template_versions(version_id),
    ownership_id INT REFERENCES flat_ownerships(ownership_id) ON DELETE CASCADE,
    current_turn VARCHAR(10),
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

-- Each process can have multiple steps. Each step can have multiple substeps.
-- parent_step_instance_id links to parent step instance. Null for root step.
CREATE TABLE IF NOT EXISTS process_steps (
    step_instance_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    process_id UUID NOT NULL REFERENCES processes(process_id) ON DELETE CASCADE,
    template_step_id INT NOT NULL,
    parent_step_instance_id UUID REFERENCES process_steps(step_instance_id) ON DELETE CASCADE,

    -- is_completed:
    -- TRUE  => User has filled all required fields for this step, or all child substeps
    --          are completed (which propagates completion upwards to the parent).
    -- FALSE => Step is still open/pending user input, OR it was rejected by a reviewer,
    --          OR it was manually reopened by a reviewer (which sets is_completed = FALSE
    --          for this step and parent steps up the chain).
    is_completed BOOLEAN DEFAULT FALSE NOT NULL,

    started_at TIMESTAMP WITH TIME ZONE,
    completed_at TIMESTAMP WITH TIME ZONE,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    UNIQUE (process_id, template_step_id)
);

-- NOTE: We explicitly omit a UNIQUE constraint on (step_instance_id, template_field_id).
-- This allows the table to act as an append-only audit log, storing full submission
-- and edit history. The application retrieves only the latest value by querying with
-- DISTINCT ON (template_field_id) ordered by updated_at DESC.
CREATE TABLE IF NOT EXISTS process_fields (
    field_instance_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    step_instance_id UUID NOT NULL REFERENCES process_steps(step_instance_id) ON DELETE CASCADE,
    template_field_id INT NOT NULL,
    text_value TEXT,
    document_id UUID REFERENCES documents(document_id) ON DELETE SET NULL,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

-- Reviewer history for a step.
CREATE TABLE IF NOT EXISTS process_step_reviews (
    review_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    step_instance_id UUID NOT NULL REFERENCES process_steps(step_instance_id) ON DELETE CASCADE,
    reviewer_user_id INT REFERENCES users(user_id) ON DELETE SET NULL,

    -- is_approved:
    -- TRUE  => Reviewer approved the step submission. It transitions the step to Completed.
    -- FALSE => Reviewer rejected/reopened the step. It resets is_completed to FALSE for this
    --          step and its parent chain (transitioning them back to Open so users can correct them).
    --          Importantly, sibling substeps remain completed (is_completed = TRUE) to preserve
    --          other unrelated, valid documents/entries from corruption.
    is_approved BOOLEAN NOT NULL,

    notes TEXT,
    -- Reviewer-annotated list of template_step_ids flagged during this review. Purely a
    -- display hint for the resident UI — has no effect on the state machine.
    flagged_step_ids INT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

-- Standalone notes reviewers can drop on any step at any time, without needing to
-- approve or reject. Never shown to residents.
CREATE TABLE IF NOT EXISTS process_reviewer_comments (
    comment_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    process_id UUID NOT NULL REFERENCES processes(process_id) ON DELETE CASCADE,
    template_step_id INT NOT NULL,
    reviewer_user_id INT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    comment TEXT NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_processes_user ON processes(user_id);
CREATE INDEX IF NOT EXISTS idx_process_steps_process ON process_steps(process_id);
CREATE INDEX IF NOT EXISTS idx_process_fields_step ON process_fields(step_instance_id);
CREATE INDEX IF NOT EXISTS idx_process_reviews_step ON process_step_reviews(step_instance_id);
CREATE INDEX IF NOT EXISTS idx_reviewer_comments_process ON process_reviewer_comments(process_id);
