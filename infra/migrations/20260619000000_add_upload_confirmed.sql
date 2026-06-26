-- Add is_upload_confirmed column to documents table.
-- Default TRUE so existing documents (already uploaded via server proxy) are not affected.
-- New presigned-upload documents will be created with is_upload_confirmed = FALSE,
-- and flipped to TRUE after the client confirms the S3 upload succeeded.
ALTER TABLE documents ADD COLUMN IF NOT EXISTS is_upload_confirmed BOOLEAN NOT NULL DEFAULT TRUE;
