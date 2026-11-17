-- owner: a2m-ledger
-- Findings and the unknowns ledger (spec section 218), published versions
-- and their plans, and the audit chain.

-- A finding is named by the digest of its content, so it is never rewritten.
-- The level is checked here as well as by the type: nothing above `discovered`.
CREATE TABLE findings (
    workspace_id TEXT NOT NULL,
    target TEXT NOT NULL,
    id TEXT NOT NULL,
    format INTEGER NOT NULL,
    level TEXT NOT NULL CHECK (level IN ('inferred', 'discovered')),
    kind TEXT NOT NULL,
    source TEXT NOT NULL,
    body TEXT NOT NULL,
    observed_at INTEGER NOT NULL,
    stored_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, target, id)
) STRICT;

CREATE TRIGGER findings_never_change
BEFORE UPDATE ON findings
BEGIN
    SELECT RAISE(ABORT, 'a2m: a stored finding is never rewritten');
END;

CREATE TABLE unknowns (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    target TEXT NOT NULL,
    question TEXT NOT NULL,
    scope TEXT NOT NULL,
    why_it_matters TEXT NOT NULL,
    settled_by_what TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('open', 'looking', 'blocked', 'settled', 'out_of_scope')),
    settlement TEXT,
    opened_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK ((status = 'settled') = (settlement IS NOT NULL))
) STRICT;

CREATE INDEX unknowns_by_target ON unknowns (workspace_id, target, status);

-- A published version is its plan's bytes, named by their hash.
CREATE TABLE target_versions (
    workspace_id TEXT NOT NULL,
    target TEXT NOT NULL,
    plan_hash TEXT NOT NULL,
    version TEXT NOT NULL,
    plan BLOB NOT NULL,
    published_by TEXT NOT NULL,
    published_at INTEGER NOT NULL,
    is_current INTEGER NOT NULL DEFAULT 0 CHECK (is_current IN (0, 1)),
    PRIMARY KEY (workspace_id, target, plan_hash)
) STRICT;

CREATE UNIQUE INDEX target_versions_one_current ON target_versions (workspace_id, target)
WHERE is_current = 1;

CREATE TRIGGER target_versions_keep_their_plan
BEFORE UPDATE ON target_versions
WHEN NEW.plan IS NOT OLD.plan
    OR NEW.plan_hash IS NOT OLD.plan_hash
    OR NEW.version IS NOT OLD.version
    OR NEW.target IS NOT OLD.target
    OR NEW.workspace_id IS NOT OLD.workspace_id
BEGIN
    SELECT RAISE(ABORT, 'a2m: a published version never changes');
END;

CREATE TRIGGER target_versions_are_kept
BEFORE DELETE ON target_versions
BEGIN
    SELECT RAISE(ABORT, 'a2m: a published version is never removed');
END;

-- The audit chain, one per workspace: each entry carries the hash of the one
-- before it.
CREATE TABLE audit (
    workspace_id TEXT NOT NULL,
    seq INTEGER NOT NULL,
    at INTEGER NOT NULL,
    actor TEXT NOT NULL,
    action TEXT NOT NULL,
    subject TEXT NOT NULL,
    detail TEXT NOT NULL,
    previous_hash TEXT NOT NULL,
    hash TEXT NOT NULL,
    PRIMARY KEY (workspace_id, seq)
) STRICT;

CREATE TRIGGER audit_never_changes
BEFORE UPDATE ON audit
BEGIN
    SELECT RAISE(ABORT, 'a2m: the audit chain is append-only');
END;

CREATE TRIGGER audit_is_kept
BEFORE DELETE ON audit
BEGIN
    SELECT RAISE(ABORT, 'a2m: the audit chain is append-only');
END;
