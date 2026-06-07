-- Core users table (holds email as primary identity)
CREATE TABLE IF NOT EXISTS users (
    user_id SERIAL PRIMARY KEY,
    email VARCHAR(255) NOT NULL UNIQUE,
    -- User becomes inactive if all flats mapped to this user is revoked.
    -- They can become active again if admin re-adds them mapped to a flat.
    is_active BOOLEAN DEFAULT TRUE NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

-- User credentials table (for authentication)
CREATE TABLE IF NOT EXISTS user_credentials (
    user_id INT PRIMARY KEY REFERENCES users(user_id) ON DELETE CASCADE,
    password_hash VARCHAR(255), -- Nullable for first-time onboarding
    reset_token VARCHAR(255),
    reset_token_expires_at TIMESTAMP WITH TIME ZONE,
    failed_login_attempts INT DEFAULT 0 NOT NULL
);

-- Flats table representing individual flat units with wings
CREATE TABLE IF NOT EXISTS flats (
    flat_id SERIAL PRIMARY KEY,
    wing VARCHAR(255) NOT NULL,
    flat_number VARCHAR(255) NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    UNIQUE (wing, flat_number)
);

-- Historical flat ownership mapping table
CREATE TABLE IF NOT EXISTS flat_ownerships (
    ownership_id SERIAL PRIMARY KEY,
    flat_id INT NOT NULL REFERENCES flats(flat_id) ON DELETE CASCADE,
    user_id INT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    start_date TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    end_date TIMESTAMP WITH TIME ZONE,
    is_active BOOLEAN DEFAULT TRUE NOT NULL
);

-- Partial index to enforce that a flat has at most ONE active owner at any point in time
CREATE UNIQUE INDEX IF NOT EXISTS idx_active_flat_ownership 
ON flat_ownerships (flat_id) 
WHERE (is_active = TRUE);

-- Create user_role enum safely
DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'user_role') THEN
        CREATE TYPE user_role AS ENUM ('admin', 'committee');
    END IF;
END
$$;

-- Mapping table for user roles (admins, committee members) for historical logging
CREATE TABLE IF NOT EXISTS user_roles (
    role_id SERIAL PRIMARY KEY,
    user_id INT NOT NULL REFERENCES users(user_id) ON DELETE CASCADE,
    role user_role NOT NULL,
    is_active BOOLEAN DEFAULT TRUE NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW() NOT NULL
);

-- Index to enforce a user can only have one active role of each type
CREATE UNIQUE INDEX IF NOT EXISTS idx_active_user_role
ON user_roles (user_id, role)
WHERE (is_active = TRUE);
