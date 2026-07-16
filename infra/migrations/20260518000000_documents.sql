CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'document_category') THEN
        CREATE TYPE document_category AS ENUM (
            'PUBLIC',
            'RESIDENT_SPECIFIC',
            'COMMITTEE',
            'FINANCE',
            'GOVERNANCE',
            'OPERATIONAL'
        );
    END IF;
END
$$;

CREATE TABLE IF NOT EXISTS documents (
    document_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    title VARCHAR(255) NOT NULL,
    description TEXT,
    category document_category NOT NULL,
    uploaded_date TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    is_deleted BOOLEAN DEFAULT FALSE NOT NULL,
    is_user_uploaded BOOLEAN DEFAULT FALSE NOT NULL,
    is_upload_confirmed BOOLEAN NOT NULL DEFAULT TRUE,
    uploaded_by INT REFERENCES users(user_id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS generic_document_metadata (
    document_id UUID PRIMARY KEY REFERENCES documents(document_id) ON DELETE CASCADE,
    subcategory VARCHAR(255),
    tags TEXT[] DEFAULT '{}'::TEXT[] NOT NULL,
    document_date DATE NOT NULL,
    valid_from DATE,
    valid_until DATE,

    CONSTRAINT chk_generic_valid_date_range CHECK (
        valid_from IS NULL OR valid_until IS NULL OR valid_from <= valid_until
    )
);

CREATE TABLE IF NOT EXISTS document_flats (
    document_id UUID NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
    flat_id INT NOT NULL REFERENCES flats(flat_id) ON DELETE CASCADE,
    PRIMARY KEY (document_id, flat_id)
);

CREATE TABLE IF NOT EXISTS document_category_designations (
    category document_category NOT NULL,
    designation_id INT NOT NULL REFERENCES designations(designation_id) ON DELETE CASCADE,
    PRIMARY KEY (category, designation_id)
);

INSERT INTO document_category_designations (category, designation_id)
SELECT seed.category::document_category, d.designation_id
FROM (VALUES
    ('COMMITTEE', 'committee'),
    ('FINANCE', 'treasurer'),
    ('GOVERNANCE', 'secretary'),
    ('OPERATIONAL', 'staff')
) AS seed(category, designation_code)
JOIN designations d ON d.code = seed.designation_code
ON CONFLICT DO NOTHING;

CREATE INDEX IF NOT EXISTS idx_documents_category ON documents(category);

CREATE INDEX IF NOT EXISTS idx_generic_meta_date ON generic_document_metadata(document_date);
CREATE INDEX IF NOT EXISTS idx_generic_meta_tags ON generic_document_metadata USING gin(tags);
