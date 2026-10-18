CREATE TABLE IF NOT EXISTS users (
    user_id SERIAL PRIMARY KEY,
    email VARCHAR(255) NOT NULL UNIQUE,
    is_active BOOLEAN DEFAULT TRUE NOT NULL,
    created_by INT REFERENCES users(user_id) ON DELETE SET NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

CREATE TABLE IF NOT EXISTS user_credentials (
    user_id INT PRIMARY KEY REFERENCES users(user_id) ON DELETE CASCADE,
    password_hash VARCHAR(255),
    reset_token VARCHAR(255),
    reset_token_expires_at TIMESTAMP WITH TIME ZONE,
    failed_login_attempts INT DEFAULT 0 NOT NULL,
    mfa_otp_code VARCHAR(6),
    mfa_otp_expires_at TIMESTAMP WITH TIME ZONE,
    mfa_otp_attempts INT NOT NULL DEFAULT 0,
    mfa_pending_token VARCHAR(255),
    mfa_otp_resends INT NOT NULL DEFAULT 0
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_user_credentials_mfa_pending_token
ON user_credentials (mfa_pending_token)
WHERE (mfa_pending_token IS NOT NULL);

CREATE TABLE IF NOT EXISTS user_settings (
    user_id INT PRIMARY KEY REFERENCES users(user_id) ON DELETE CASCADE,
    mfa_enabled BOOLEAN NOT NULL DEFAULT FALSE,
    notify_process_updates BOOLEAN NOT NULL DEFAULT TRUE,
    notify_signature_events BOOLEAN NOT NULL DEFAULT TRUE,
    updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS designations (
    designation_id SERIAL PRIMARY KEY,
    code VARCHAR(64) NOT NULL UNIQUE,
    label VARCHAR(255) NOT NULL,
    is_system BOOLEAN DEFAULT FALSE NOT NULL,
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_by INT REFERENCES users(user_id) ON DELETE SET NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

CREATE TABLE IF NOT EXISTS wings (
    name VARCHAR(255) PRIMARY KEY,
    representative_designation_id INT NOT NULL REFERENCES designations(designation_id),
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

CREATE TABLE IF NOT EXISTS flats (
    flat_id SERIAL PRIMARY KEY,
    wing VARCHAR(255) NOT NULL REFERENCES wings(name),
    flat_number VARCHAR(255) NOT NULL,
    -- Property of the flat, not of whichever billing plan is assigned.
    area_sqft NUMERIC(10,2) CHECK (area_sqft > 0),
    is_fire_flat BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    UNIQUE (wing, flat_number)
);

CREATE TABLE IF NOT EXISTS flat_ownerships (
    ownership_id SERIAL PRIMARY KEY,
    flat_id INT NOT NULL REFERENCES flats(flat_id) ON DELETE CASCADE,
    user_id INT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    start_date TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    end_date TIMESTAMP WITH TIME ZONE,
    is_active BOOLEAN DEFAULT TRUE NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_active_flat_ownership
ON flat_ownerships (flat_id)
WHERE (is_active = TRUE);

CREATE INDEX IF NOT EXISTS idx_flat_ownerships_user ON flat_ownerships(user_id) WHERE is_active = TRUE;

-- Latest dues snapshot per flat, from an admin-uploaded file. One row per flat,
-- overwritten by each new upload; not derived from the accounting module at all.
CREATE TABLE IF NOT EXISTS flat_dues_from_file (
    flat_id INT PRIMARY KEY REFERENCES flats(flat_id) ON DELETE CASCADE,
    total_due NUMERIC(12,2) NOT NULL,
    overdue_amount NUMERIC(12,2) NOT NULL DEFAULT 0,
    as_of DATE NOT NULL,
    uploaded_by INT REFERENCES users(user_id) ON DELETE SET NULL,
    uploaded_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

CREATE TABLE IF NOT EXISTS permissions (
    permission_id SERIAL PRIMARY KEY,
    code VARCHAR(128) NOT NULL UNIQUE,
    label VARCHAR(255) NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

CREATE TABLE IF NOT EXISTS designation_permissions (
    designation_id INT NOT NULL REFERENCES designations(designation_id) ON DELETE CASCADE,
    permission_id INT NOT NULL REFERENCES permissions(permission_id) ON DELETE CASCADE,
    granted_by INT REFERENCES users(user_id) ON DELETE SET NULL,
    granted_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    PRIMARY KEY (designation_id, permission_id)
);

CREATE TABLE IF NOT EXISTS user_designations (
    user_designation_id SERIAL PRIMARY KEY,
    user_id INT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    designation_id INT NOT NULL REFERENCES designations(designation_id) ON DELETE CASCADE,
    is_active BOOLEAN DEFAULT TRUE NOT NULL,
    assigned_by INT REFERENCES users(user_id) ON DELETE SET NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_user_designations_user ON user_designations(user_id);

CREATE UNIQUE INDEX IF NOT EXISTS idx_user_designations_active_unique
ON user_designations (user_id, designation_id)
WHERE (is_active = TRUE);

-- Append-only audit trail for permission/role/document-category-access changes.
CREATE TABLE IF NOT EXISTS permission_log (
    log_id SERIAL PRIMARY KEY,
    description TEXT NOT NULL,
    done_by INT REFERENCES users(user_id) ON DELETE SET NULL,
    done_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);
