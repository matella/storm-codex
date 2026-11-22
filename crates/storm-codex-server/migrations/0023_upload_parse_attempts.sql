-- Parse attempts of an upload (security review C1, defence in depth): incremented right before
-- each parse started by the upload path or the startup resume (`upload::run_parse`). A file that
-- kills the whole process mid-parse leaves its row `pending`; `upload::resume_pending` gives up
-- once `parse_attempts` reaches MAX_PARSE_ATTEMPTS (3) and marks the row `parse_failed`
-- (`error_class = 'too_many_attempts'`) instead of re-parsing it at every startup (crash loop).
-- `POST /api/admin/reprocess` can still retry such a row by hand.
ALTER TABLE uploads ADD COLUMN parse_attempts INT NOT NULL DEFAULT 0;
