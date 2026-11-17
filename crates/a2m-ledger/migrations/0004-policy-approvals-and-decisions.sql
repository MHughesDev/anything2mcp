-- owner: a2m-policy
-- Approvals and recorded decisions (spec section 212). The policy engine owns
-- these tables and is the only writer. The ledger reads them, and only when
-- writes begin (rule 3 of spec section 208): of an approval it reads `id`,
-- `scope`, `run_id`, `diff_hash`, `used_at`, and `revoked_at`; of a decision
-- it reads `id`, `run_id`, `question`, `allowed`, and `diff_hash`. Those
-- columns keep their meaning; the rest is the policy engine's to change.

CREATE TABLE approvals (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    scope TEXT NOT NULL CHECK (scope IN ('run', 'standing')),
    run_id TEXT,
    diff_hash TEXT,
    tool_version_hash TEXT,
    approver_id TEXT NOT NULL,
    channel TEXT NOT NULL,
    presence_check TEXT NOT NULL,
    granted_at INTEGER NOT NULL,
    expires_at INTEGER,
    used_at INTEGER,
    revoked_at INTEGER
) STRICT;

CREATE INDEX approvals_by_run ON approvals (run_id);

-- One row per answer of the policy engine. `question` is the name of what was
-- asked; `write_without_person` with `allowed` 1 is what lets a run write
-- with no approval.
CREATE TABLE decisions (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    run_id TEXT,
    principal_id TEXT NOT NULL,
    question TEXT NOT NULL,
    allowed INTEGER NOT NULL CHECK (allowed IN (0, 1)),
    diff_hash TEXT,
    reasons TEXT NOT NULL,
    policy_version TEXT,
    decided_at INTEGER NOT NULL
) STRICT;

CREATE INDEX decisions_by_run ON decisions (run_id);
