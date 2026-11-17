-- owner: a2m-actions
-- What the action layer keeps (spec sections 220, 226, and 402): the facts
-- of this installation, toolsets, and the scout's stage results. Written by
-- `a2m-actions`, which is the only crate that reads or writes these.
-- Moments are milliseconds since 1970, from the store's clock.

-- One fact about the installation, by name: the workspace of one machine,
-- the principal of its user, the digest of the recorded catalog, and the
-- digest of the settings at the last start. Never a secret.
CREATE TABLE actions_installation (
    name TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    set_at INTEGER NOT NULL
) STRICT;

-- A toolset: which tools, of which version, under which identity mode, as
-- JSON its owner wrote through `toolset.create` and `toolset.update`.
CREATE TABLE actions_toolsets (
    workspace_id TEXT NOT NULL,
    name TEXT NOT NULL,
    definition TEXT NOT NULL,
    updated_by TEXT NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, name)
) STRICT;

-- One finished stage of one scout run. A stopped scout continues after the
-- last stage stored here; `input_hash` says what the stage was given, and
-- `result` is its result as JSON, already redacted.
CREATE TABLE actions_scout_stages (
    run_id TEXT NOT NULL,
    stage INTEGER NOT NULL CHECK (stage BETWEEN 0 AND 7),
    input_hash TEXT NOT NULL,
    result TEXT NOT NULL,
    finished_at INTEGER NOT NULL,
    PRIMARY KEY (run_id, stage)
) STRICT;
