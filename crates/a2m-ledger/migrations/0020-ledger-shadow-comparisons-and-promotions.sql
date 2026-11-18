-- owner: a2m-ledger
-- A binding in shadow, compared with the binding that did the real work of
-- a live call, and its promotion by a person (spec section 201, decision
-- 0207). Both tables are append-only: a count is a query over the rows, so
-- it can never drift from the evidence it counts.
--
-- One row per comparison. `binding_digest` names the shadow binding by what
-- it does (decision 0200): a binding whose steps change has another digest
-- and starts again from nothing. `against_digest` names the binding it was
-- compared with. `outcome` is `match`, `mismatch`, or `not_compared` (the
-- shadow failed, or passed its time bound); `differing` holds, for a
-- mismatch, the names of the outputs that differed and how, as canonical
-- JSON, never a value; `why_class` and `why_code` hold, for `not_compared`,
-- what stopped it. Workspace, target, and environment are the run's own.
CREATE TABLE shadow_comparisons (
    seq INTEGER PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    target TEXT NOT NULL,
    environment TEXT NOT NULL,
    operation TEXT NOT NULL,
    binding_digest TEXT NOT NULL,
    mode TEXT NOT NULL,
    position INTEGER NOT NULL CHECK (position >= 0),
    against_digest TEXT NOT NULL,
    run_id TEXT NOT NULL REFERENCES runs (id),
    writes INTEGER NOT NULL CHECK (writes IN (0, 1)),
    outcome TEXT NOT NULL CHECK (outcome IN ('match', 'mismatch', 'not_compared')),
    differing TEXT,
    why_class TEXT,
    why_code TEXT,
    at INTEGER NOT NULL,
    CHECK ((outcome = 'mismatch') = (differing IS NOT NULL)),
    CHECK ((outcome = 'not_compared') = (why_code IS NOT NULL AND why_class IS NOT NULL))
) STRICT;

CREATE INDEX shadow_comparisons_by_binding
    ON shadow_comparisons (workspace_id, target, operation, binding_digest, seq);

-- One row per promotion: the binding, by digest, that a person promoted,
-- and the evidence it was promoted on: the matches in a row, the threshold
-- in force, and the last comparison counted.
CREATE TABLE shadow_promotions (
    workspace_id TEXT NOT NULL,
    target TEXT NOT NULL,
    operation TEXT NOT NULL,
    binding_digest TEXT NOT NULL,
    mode TEXT NOT NULL,
    promoted_by TEXT NOT NULL,
    promoted_at INTEGER NOT NULL,
    matches INTEGER NOT NULL CHECK (matches > 0),
    threshold INTEGER NOT NULL CHECK (threshold > 0 AND matches >= threshold),
    through_seq INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, target, operation, binding_digest)
) STRICT;

CREATE TRIGGER shadow_comparisons_never_change
BEFORE UPDATE ON shadow_comparisons
BEGIN
    SELECT RAISE(ABORT, 'a2m: a shadow comparison never changes');
END;

CREATE TRIGGER shadow_comparisons_leave_only_with_their_run
BEFORE DELETE ON shadow_comparisons
WHEN EXISTS (SELECT 1 FROM runs WHERE id = OLD.run_id AND retention_state IS NOT 'purging')
BEGIN
    SELECT RAISE(ABORT, 'a2m: a shadow comparison leaves only with its run');
END;

CREATE TRIGGER shadow_promotions_never_change
BEFORE UPDATE ON shadow_promotions
BEGIN
    SELECT RAISE(ABORT, 'a2m: a promotion never changes');
END;

CREATE TRIGGER shadow_promotions_never_leave
BEFORE DELETE ON shadow_promotions
BEGIN
    SELECT RAISE(ABORT, 'a2m: a promotion is never removed');
END;

-- A comparison is counted only for a run that is under way: the call it was
-- made beside is live.
CREATE TRIGGER shadow_comparisons_only_beside_a_running_run
BEFORE INSERT ON shadow_comparisons
WHEN NOT EXISTS (SELECT 1 FROM runs WHERE id = NEW.run_id AND state = 'running')
BEGIN
    SELECT RAISE(ABORT, 'a2m: a shadow comparison is made only beside a running run');
END;
