-- Migration: Add flagged_step_ids to process_step_reviews
--
-- PURPOSE
-- -------
-- Stores an admin-annotated list of substep identifiers on a review record.
-- This is purely a display hint for the resident UI — it has NO effect on
-- step completion logic or state machine transitions.
--
-- WHAT VALUES ARE STORED
-- ----------------------
-- Each integer in flagged_step_ids is a `template_step_id` — the numeric ID
-- assigned to a step in the process template YAML files (e.g. step_id: 3 for
-- "Owner Documents").  These are the SAME integers stored in
-- process_steps.template_step_id for this process.
--
-- EXAMPLE
-- -------
-- flagged_step_ids = '{3, 5}' means the admin flagged substeps "Owner Documents"
-- (template_step_id=3) and "Undertaking" (template_step_id=5) during this review.

ALTER TABLE process_step_reviews
    ADD COLUMN IF NOT EXISTS flagged_step_ids INT[] NOT NULL DEFAULT '{}';
