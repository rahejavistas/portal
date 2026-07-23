-- Versioned, DB-backed process templates. Each logical template (a "process type",
-- e.g. tenant onboarding) is a `process_templates` row; every save produces a new
-- append-only `process_template_versions` row, each with its own plain integer PK
-- (`version_id`). A process instance is permanently pinned to ONE `version_id` for
-- its whole lifetime (`processes.template_version_id`) — `process_templates` itself
-- is only ever consulted by the admin builder (listing/creating/activating templates)
-- and when starting a brand-new resident process (to resolve which version_id is
-- currently active); nothing else needs to look at it again.

CREATE TABLE IF NOT EXISTS process_templates (
    template_id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    active_version_id INT, -- which version_id residents currently start against; NULL until explicitly activated
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS process_template_versions (
    version_id SERIAL PRIMARY KEY, -- the permanent, never-reused key every process actually points to
    template_id INT NOT NULL REFERENCES process_templates(template_id) ON DELETE CASCADE,
    version_number INT NOT NULL, -- human-facing display number only (1, 2, 3...) — never used for lookups/joins
    definition JSONB NOT NULL, -- step_id/field_id only need to be unique WITHIN this definition, nowhere else
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    created_by INT REFERENCES users(user_id) ON DELETE SET NULL,
    UNIQUE (template_id, version_number)
);

ALTER TABLE process_templates
    ADD CONSTRAINT fk_process_templates_active_version
    FOREIGN KEY (active_version_id) REFERENCES process_template_versions(version_id);
