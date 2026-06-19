-- Migration: Decouple documents from processes
-- Documents become independent entities. Process linkage is only via process_fields.document_id.

-- Drop the process_id foreign key and column from documents
ALTER TABLE documents DROP COLUMN IF EXISTS process_id;

-- Drop the now-unused index
DROP INDEX IF EXISTS idx_documents_process;
