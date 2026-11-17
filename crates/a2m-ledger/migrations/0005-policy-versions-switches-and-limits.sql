-- owner: a2m-policy
-- The rest of what the policy engine keeps (spec section 212): what a
-- recorded decision was about, declined approvals, published policy sets,
-- kill switches, and quota counters. Written by `a2m-policy`, which is the
-- only crate that reads or writes these.

-- What a decision was about, for `policy.explain`: the resource's short
-- name, the whole request as canonical JSON, and the code a refused caller
-- was given.
ALTER TABLE decisions ADD COLUMN resource TEXT;
ALTER TABLE decisions ADD COLUMN request TEXT;
ALTER TABLE decisions ADD COLUMN refusal_code TEXT;

-- A person's refusal of a run's diff, or an approval window that ended with
-- no answer: then `declined_by` holds no value, a decline by nobody.
CREATE TABLE approval_declines (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    declined_by TEXT,
    channel TEXT NOT NULL,
    declined_at INTEGER NOT NULL
) STRICT;

CREATE INDEX approval_declines_by_run ON approval_declines (run_id);

-- Published policy sets. A row is immutable; the newest `seq` is the one in
-- force. `layers` and `tests` are canonical JSON, and `version` is the
-- digest over the format, the guardrails' digest, and both texts. A set
-- published again after another took its place is a new row with the same
-- version.
CREATE TABLE policy_versions (
    seq INTEGER PRIMARY KEY,
    version TEXT NOT NULL,
    format INTEGER NOT NULL,
    guardrails TEXT NOT NULL,
    layers TEXT NOT NULL,
    tests TEXT NOT NULL,
    published_by TEXT,
    presence_check TEXT,
    published_at INTEGER NOT NULL
) STRICT;

CREATE INDEX policy_versions_by_version ON policy_versions (version);

CREATE TRIGGER policy_versions_never_change
BEFORE UPDATE ON policy_versions
BEGIN
    SELECT RAISE(ABORT, 'a2m: a published policy version never changes');
END;

CREATE TRIGGER policy_versions_are_kept
BEFORE DELETE ON policy_versions
BEGIN
    SELECT RAISE(ABORT, 'a2m: a published policy version is never removed');
END;

-- Kill switches. `scope` is `everything`, `workspace`, `target`, or
-- `operation`; `name` is what the scope names, and is empty for everything.
-- `epoch` rises with every change, so a runner can tell that one happened.
CREATE TABLE kill_switches (
    scope TEXT NOT NULL CHECK (scope IN ('everything', 'workspace', 'target', 'operation')),
    name TEXT NOT NULL,
    is_on INTEGER NOT NULL CHECK (is_on IN (0, 1)),
    epoch INTEGER NOT NULL,
    set_by TEXT NOT NULL,
    presence_check TEXT,
    set_at INTEGER NOT NULL,
    PRIMARY KEY (scope, name)
) STRICT;

-- Calls counted against a quota, one row per caller, period length, and
-- period.
CREATE TABLE quota_counters (
    workspace_id TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    period_minutes INTEGER NOT NULL,
    period_start INTEGER NOT NULL,
    calls INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, principal_id, period_minutes, period_start)
) STRICT;
