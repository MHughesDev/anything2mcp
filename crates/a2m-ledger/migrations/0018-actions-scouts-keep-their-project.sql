-- owner: a2m-actions
-- A scout is kept by the project it was made in (decision 0196): the digest
-- of the project folder as the dispatcher resolved it, or `none` outside a
-- project, written in the same statement as the scout's row. A scout's
-- lookup filters on it, where it read every scout of the target and the
-- result of each one's first stage before (decision 0189). A scout stored
-- before this has none, is of no project, and is never resumed.
ALTER TABLE actions_scouts ADD COLUMN project TEXT;

CREATE INDEX actions_scouts_by_project
    ON actions_scouts (workspace_id, principal_id, target, project, started_at);
