-- Versioned process templates: every save appends a new process_template_versions row.
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
