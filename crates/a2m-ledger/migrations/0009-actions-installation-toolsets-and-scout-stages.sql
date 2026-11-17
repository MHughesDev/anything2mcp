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

-- The targets that have a published version, so that a tool can be found by
-- its name without knowing its target first. Written by `version.publish`
-- in the transaction that stores the version.
CREATE TABLE actions_targets (
    workspace_id TEXT NOT NULL,
    target TEXT NOT NULL,
    first_published_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, target)
) STRICT;

-- What a run was called with, kept so that the runner that claims its job
-- can fill the job: the stored input, and the verified facts about the
-- caller that policy is asked with at guard. Keyed by what makes a run the
-- same run (ledger rule 6), so that it is written before the run is
-- admitted. `caller` is JSON of names and flags, never a credential.
CREATE TABLE actions_run_calls (
    workspace_id TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    target_version TEXT NOT NULL,
    input_hash TEXT NOT NULL,
    input_ref TEXT NOT NULL,
    caller TEXT NOT NULL,
    PRIMARY KEY (workspace_id, principal_id, tool_name, target_version, input_hash)
) STRICT;
