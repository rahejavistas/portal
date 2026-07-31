ALTER TABLE user_credentials
    ADD COLUMN IF NOT EXISTS mfa_otp_code VARCHAR(6),
    ADD COLUMN IF NOT EXISTS mfa_otp_expires_at TIMESTAMP WITH TIME ZONE,
    ADD COLUMN IF NOT EXISTS mfa_otp_attempts INT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS mfa_pending_token VARCHAR(255),
    ADD COLUMN IF NOT EXISTS mfa_otp_resends INT NOT NULL DEFAULT 0;

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

ALTER TABLE documents ADD COLUMN IF NOT EXISTS content_type VARCHAR(255);
