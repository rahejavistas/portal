-- Enable UUID extension
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

-- 1. Create proper ENUMs safely
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'document_category') THEN
        CREATE TYPE document_category AS ENUM (
            'RESIDENT_SPECIFIC',
            'GENERIC',
            'MEETING_MINUTES',
            'OPERATIONAL'
        );
    END IF;

    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'document_access_level') THEN
        CREATE TYPE document_access_level AS ENUM (
            'PUBLIC',
            'COMMITTEE_ONLY',
            'PRIVATE'
        );
    END IF;

    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'document_type') THEN
        CREATE TYPE document_type AS ENUM (
            'RESIDENT_PROPERTY',
            'RESIDENT_KYC',
            'RESIDENT_RENTAL',
            'RESIDENT_GENERAL',
            'GENERIC_RULEBOOK',
            'GENERIC_BYLAWS',
            'MINUTES_AGM',
            'MINUTES_GBM',
            'MINUTES_MGMT',
            'OPERATIONAL_VENDOR_CONTRACT',
            'OPERATIONAL_PAYMENT',
            'OPERATIONAL_SLA'
        );
    END IF;
END
$$;

-- 2. Core Documents Table (Holds universal/common properties)
CREATE TABLE IF NOT EXISTS documents (
    document_id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    title VARCHAR(255) NOT NULL,
    description TEXT,
    category document_category NOT NULL,
    document_type document_type NOT NULL,
    access_level document_access_level NOT NULL,
    
    -- Access control for committee
    restricted_committee_roles user_role[] DEFAULT '{}'::user_role[] NOT NULL,
    
    -- Shared physical document metadata
    uploaded_date TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    
    -- Soft-delete column
    is_deleted BOOLEAN DEFAULT FALSE NOT NULL,
    
    -- Flag for user uploads
    is_user_uploaded BOOLEAN DEFAULT FALSE NOT NULL,
    
    -- Ensure document category matches its type
    CONSTRAINT chk_document_type_category CHECK (
        (category = 'RESIDENT_SPECIFIC' AND document_type IN ('RESIDENT_PROPERTY', 'RESIDENT_KYC', 'RESIDENT_RENTAL', 'RESIDENT_GENERAL')) OR
        (category = 'GENERIC' AND document_type IN ('GENERIC_RULEBOOK', 'GENERIC_BYLAWS')) OR
        (category = 'MEETING_MINUTES' AND document_type IN ('MINUTES_AGM', 'MINUTES_GBM', 'MINUTES_MGMT')) OR
        (category = 'OPERATIONAL' AND document_type IN ('OPERATIONAL_VENDOR_CONTRACT', 'OPERATIONAL_PAYMENT', 'OPERATIONAL_SLA'))
    )
);

-- 6. Generic Document Metadata Table
CREATE TABLE IF NOT EXISTS generic_document_metadata (
    document_id UUID PRIMARY KEY REFERENCES documents(document_id) ON DELETE CASCADE,
    
    -- Classification
    subcategory VARCHAR(255),
    tags TEXT[] DEFAULT '{}'::TEXT[] NOT NULL,
    
    -- Temporal
    document_date DATE NOT NULL,
    valid_from DATE,
    valid_until DATE,
    
    CONSTRAINT chk_generic_valid_date_range CHECK (
        valid_from IS NULL OR valid_until IS NULL OR valid_from <= valid_until
    )
);

-- 7. Join Table for Enforcing FK Constraints on flat_ids
CREATE TABLE IF NOT EXISTS document_flats (
    document_id UUID NOT NULL REFERENCES documents(document_id) ON DELETE CASCADE,
    flat_id INT NOT NULL REFERENCES flats(flat_id) ON DELETE CASCADE,
    PRIMARY KEY (document_id, flat_id)
);

-- 8. Optimized Indexes for High-Performance Queries & Search
-- Core Indexes
CREATE INDEX IF NOT EXISTS idx_documents_category_type ON documents(category, document_type);
CREATE INDEX IF NOT EXISTS idx_documents_access_level ON documents(access_level);

-- Metadata Specific Indexes
CREATE INDEX IF NOT EXISTS idx_generic_meta_date ON generic_document_metadata(document_date);

-- GIN Indexes for Array Lookups
CREATE INDEX IF NOT EXISTS idx_generic_meta_tags ON generic_document_metadata USING gin(tags);

