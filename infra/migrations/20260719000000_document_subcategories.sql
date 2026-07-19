-- UI-only tag scoped to a category; never used for access control.
CREATE TABLE IF NOT EXISTS document_subcategories (
    subcategory_id SERIAL PRIMARY KEY,
    category VARCHAR(64) NOT NULL REFERENCES document_categories(code) ON DELETE CASCADE,
    code VARCHAR(64) NOT NULL,
    label VARCHAR(255) NOT NULL,
    created_by INT REFERENCES users(user_id) ON DELETE SET NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    UNIQUE (category, code)
);

ALTER TABLE generic_document_metadata
    ADD COLUMN subcategory_id INT REFERENCES document_subcategories(subcategory_id) ON DELETE SET NULL;

ALTER TABLE generic_document_metadata DROP COLUMN subcategory;
