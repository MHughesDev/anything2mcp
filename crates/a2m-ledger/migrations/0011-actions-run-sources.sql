-- owner: a2m-actions
-- The source a run was called with (spec section 210): a file of the
-- project read once by the dispatcher, or rows the caller gave inline.
-- Written by `a2m-actions` before the run is admitted, beside the row of
-- `actions_run_calls` with the same key, and read by the runner that claims
-- the run's job, so that every lease of the run reads the same bytes.

-- `source_ref` names the blob that holds the bytes; a blob's name is a
-- keyed digest of its content, so it is the digest the run is bound to.
-- `kind` is `file` or `rows`. `name` is the path inside the project the
-- file was read from, as the caller wrote it, or empty for rows; it is kept
-- for the record and never read from again.
CREATE TABLE actions_run_sources (
    workspace_id TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    target_version TEXT NOT NULL,
    input_hash TEXT NOT NULL,
    source_ref TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('file', 'rows')),
    name TEXT NOT NULL,
    size_bytes INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, principal_id, tool_name, target_version, input_hash)
) STRICT;
