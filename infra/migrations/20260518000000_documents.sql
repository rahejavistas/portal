CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- Categories are admin-defined except PUBLIC/RESIDENT_SPECIFIC (is_system, seeded separately).
CREATE TABLE IF NOT EXISTS document_categories (
    code VARCHAR(64) PRIMARY KEY,
    label VARCHAR(255) NOT NULL,
    is_public_read BOOLEAN NOT NULL DEFAULT FALSE, -- visible to every user, no designation needed
    is_flat_scoped BOOLEAN NOT NULL DEFAULT FALSE, -- visible only via flat ownership (document_flats)
    is_system BOOLEAN NOT NULL DEFAULT FALSE, -- protected seed row, can't be created/edited via API
    created_by INT REFERENCES users(user_id) ON DELETE SET NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

CREATE TABLE IF NOT EXISTS documents (
    document_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    title VARCHAR(255) NOT NULL,
    description TEXT,
    category VARCHAR(64) NOT NULL REFERENCES document_categories(code),
    uploaded_date TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    is_deleted BOOLEAN DEFAULT FALSE NOT NULL,
    is_user_uploaded BOOLEAN DEFAULT FALSE NOT NULL,
    is_upload_confirmed BOOLEAN NOT NULL DEFAULT TRUE,
    uploaded_by INT REFERENCES users(user_id) ON DELETE SET NULL,
    content_type VARCHAR(255),
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

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

CREATE TABLE IF NOT EXISTS generic_document_metadata (
    document_id UUID PRIMARY KEY REFERENCES documents(document_id) ON DELETE CASCADE,
    subcategory_id INT REFERENCES document_subcategories(subcategory_id) ON DELETE SET NULL,
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
    category VARCHAR(64) NOT NULL REFERENCES document_categories(code) ON DELETE CASCADE,
    designation_id INT NOT NULL REFERENCES designations(designation_id) ON DELETE CASCADE,
    can_read BOOLEAN NOT NULL DEFAULT FALSE,
    can_manage BOOLEAN NOT NULL DEFAULT FALSE,
    updated_by INT REFERENCES users(user_id) ON DELETE SET NULL,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    PRIMARY KEY (category, designation_id)
);

CREATE INDEX IF NOT EXISTS idx_documents_category ON documents(category);
