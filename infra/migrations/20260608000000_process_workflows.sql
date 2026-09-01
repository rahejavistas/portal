CREATE SEQUENCE IF NOT EXISTS inward_reg_no_seq START 1;

CREATE TABLE IF NOT EXISTS processes (
    process_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    inward_reg_no TEXT NOT NULL UNIQUE,
    user_id INT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    template_version_id INT NOT NULL REFERENCES process_template_versions(version_id),
    ownership_id INT REFERENCES flat_ownerships(ownership_id) ON DELETE CASCADE,
    current_turn VARCHAR(10),
    tat_working_days INT,
    sla_started_at TIMESTAMPTZ,
    sla_due_date DATE,
    sla_paused_at TIMESTAMPTZ,
    sla_paused_working_days INT NOT NULL DEFAULT 0,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

-- parent_step_instance_id is NULL for the root step.
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

-- No UNIQUE on (step_instance_id, template_field_id) — append-only history, latest
-- value read via DISTINCT ON (template_field_id) ORDER BY updated_at DESC.
CREATE TABLE IF NOT EXISTS process_fields (
    field_instance_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    step_instance_id UUID NOT NULL REFERENCES process_steps(step_instance_id) ON DELETE CASCADE,
    template_field_id INT NOT NULL,
    text_value TEXT,
    document_id UUID REFERENCES documents(document_id) ON DELETE SET NULL,
    updated_by INT REFERENCES users(user_id) ON DELETE SET NULL,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    -- Designation that authorized this write, if any.
    acted_as_designation VARCHAR(64)
);

-- Reviewer history for a step. is_approved=FALSE resets is_completed up the chain.
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
    -- Display hint only — flagged steps for resident attention, no state-machine effect.
    flagged_step_ids INT[] NOT NULL DEFAULT '{}',
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    -- Designation that authorized this review, if any.
    acted_as_designation VARCHAR(64)
);

-- Reviewer-only notes, never shown to residents.
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

CREATE TABLE IF NOT EXISTS public_holidays (
    holiday_date DATE PRIMARY KEY,
    description VARCHAR(255) NOT NULL
);

-- Append-only: process status/flag/return history, with actor and timestamp.
CREATE TABLE IF NOT EXISTS process_audit_log (
    audit_id BIGSERIAL PRIMARY KEY,
    process_id UUID NOT NULL REFERENCES processes(process_id) ON DELETE CASCADE,
    template_step_id INT,
    event_type VARCHAR(32) NOT NULL,
    detail TEXT,
    actor_id INT REFERENCES users(user_id) ON DELETE SET NULL,
    -- Designation that authorized this event, if any.
    acted_as_designation VARCHAR(64),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_process_audit_log_process ON process_audit_log(process_id, created_at);

INSERT INTO public_holidays (holiday_date, description) VALUES
    ('2026-01-26', 'Republic Day'),
    ('2026-02-15', 'Mahashivratri'),
    ('2026-02-19', 'Chhatrapati Shivaji Maharaj Jayanti'),
    ('2026-03-03', 'Holi (Second Day)'),
    ('2026-03-19', 'Gudhi Padwa'),
    ('2026-03-21', 'Ramzan-Id (Id-Ul-Fitra)'),
    ('2026-03-26', 'Ram Navami'),
    ('2026-03-31', 'Mahavir Janmakalyanak'),
    ('2026-04-03', 'Good Friday'),
    ('2026-04-14', 'Dr. Babasaheb Ambedkar Jayanti'),
    ('2026-05-01', 'Maharashtra Din / Buddha Pournima'),
    ('2026-05-28', 'Bakri Id (Id-Uz-Zuha)'),
    ('2026-06-26', 'Moharum'),
    ('2026-08-15', 'Independence Day / Parsi New Year'),
    ('2026-08-26', 'Id-E-Milad'),
    ('2026-09-14', 'Ganesh Chaturthi'),
    ('2026-10-02', 'Mahatma Gandhi Jayanti'),
    ('2026-10-20', 'Dasara'),
    ('2026-11-08', 'Diwali Amavasya (Laxmi Pujan)'),
    ('2026-11-10', 'Diwali (Bali Pratipada)'),
    ('2026-11-11', 'Bhaubeej'),
    ('2026-11-24', 'Guru Nanak Jayanti'),
    ('2026-12-25', 'Christmas')
ON CONFLICT (holiday_date) DO UPDATE SET description = EXCLUDED.description;
