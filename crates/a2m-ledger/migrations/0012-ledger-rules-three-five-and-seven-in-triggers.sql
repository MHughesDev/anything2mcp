-- owner: a2m-ledger
-- The rules of spec section 208 that migration 0002 left to the ledger's code
-- alone, now held where the data is as well, so that a statement that breaks
-- them is refused whoever sends it (decision 0145). With these, every rule
-- the data can show has a trigger: rule 3 (a write run starts only cleared,
-- by an approval or a decision that is the run's), rule 5 (no intent while a
-- record of an earlier lease is in doubt), rule 7 on the side of the intent
-- (the run holds the record's lock under its lease), and a record goes back
-- to `planned` only after a read. What no trigger can hold is said in the
-- ledger's design: whether an approval has expired, which needs the
-- engine's clock, and whether it was marked used in this very transaction,
-- which only the connection knows.

-- When a record that may have been written was read back: what lets it
-- return to `planned`, and what makes a `failed` record that reached
-- `acted` settled. Cleared when a new intent is stored.
ALTER TABLE run_records ADD COLUMN reconciled_at INTEGER;

-- Rule 3: a write run with records to write enters `running` only with an
-- approval or a decision bound to it.
CREATE TRIGGER runs_write_starts_cleared
BEFORE UPDATE OF state ON runs
WHEN NEW.state = 'running' AND OLD.state IS NOT 'running' AND NEW.effect IS NOT 'read'
    AND NEW.approval_id IS NULL AND NEW.decision_id IS NULL
    AND EXISTS (SELECT 1 FROM run_records WHERE run_id = NEW.id AND state = 'planned')
BEGIN
    SELECT RAISE(ABORT, 'a2m: a write run starts only with an approval or a decision');
END;

-- Rule 3: what is bound is set once, and is this run's: an approval for its
-- diff and tool version, used and not revoked; or an allowed decision to
-- write without a person, for its diff.
CREATE TRIGGER runs_clearance_is_the_runs
BEFORE UPDATE OF approval_id, decision_id ON runs
WHEN (NEW.approval_id IS NOT OLD.approval_id
        AND (OLD.approval_id IS NOT NULL OR NOT EXISTS (
            SELECT 1 FROM approvals
            WHERE id = NEW.approval_id AND scope = 'run' AND run_id = NEW.id
              AND workspace_id = NEW.workspace_id
              AND diff_hash = NEW.diff_hash AND tool_version_hash = NEW.target_version_hash
              AND expires_at IS NOT NULL AND used_at IS NOT NULL AND revoked_at IS NULL)))
    OR (NEW.decision_id IS NOT OLD.decision_id
        AND (OLD.decision_id IS NOT NULL OR NOT EXISTS (
            SELECT 1 FROM decisions
            WHERE id = NEW.decision_id AND run_id = NEW.id AND workspace_id = NEW.workspace_id
              AND question = 'write_without_person' AND allowed = 1
              AND diff_hash = NEW.diff_hash AND policy_version IS NOT NULL)))
BEGIN
    SELECT RAISE(ABORT, 'a2m: a run is cleared once, by an approval or a decision of its own');
END;

-- Rules 3, 5, and 7, for an intent: the run is `running` and cleared; no
-- record of it from an earlier lease is in doubt; and it holds the record's
-- lock, held, under the intent's lease.
CREATE TRIGGER run_records_intent_only_when_cleared
BEFORE UPDATE OF state ON run_records
WHEN NEW.state = 'intent' AND NOT EXISTS (
    SELECT 1 FROM runs
    WHERE id = NEW.run_id AND state = 'running'
      AND (approval_id IS NOT NULL OR decision_id IS NOT NULL))
BEGIN
    SELECT RAISE(ABORT, 'a2m: an intent is stored for a running run that was cleared to write');
END;

CREATE TRIGGER run_records_no_intent_while_in_doubt
BEFORE UPDATE OF state ON run_records
WHEN NEW.state = 'intent' AND EXISTS (
    SELECT 1 FROM run_records AS other
    WHERE other.run_id = NEW.run_id AND other.intent_epoch < NEW.intent_epoch
      AND (other.state IN ('intent', 'acted')
           OR (other.state = 'failed' AND other.reconciled_at IS NULL
               AND (other.acted_at IS NOT NULL
                    OR other.failure_class NOT IN ('policy', 'stale', 'input')))))
BEGIN
    SELECT RAISE(ABORT, 'a2m: a record of an earlier lease is in doubt and must be read back first');
END;

CREATE TRIGGER run_records_intent_holds_the_lock
BEFORE UPDATE OF state ON run_records
WHEN NEW.state = 'intent' AND NOT EXISTS (
    SELECT 1 FROM record_locks AS lock, runs AS run
    WHERE run.id = NEW.run_id
      AND lock.workspace_id = run.workspace_id AND lock.target = run.target
      AND lock.environment = run.environment AND lock.record_key_hash = NEW.record_key_hash
      AND lock.run_id = NEW.run_id AND lock.state = 'held' AND lock.epoch = NEW.intent_epoch)
BEGIN
    SELECT RAISE(ABORT, 'a2m: an intent is stored only under the record lock of its lease');
END;

-- Rule 5: a record that may have been written goes back to `planned` only
-- after it was read back. A `failed` record for which no intent was stored
-- since it was last planned was never sent, and needs no read.
CREATE TRIGGER run_records_planned_again_only_after_a_read
BEFORE UPDATE OF state ON run_records
WHEN NEW.state = 'planned' AND OLD.state IN ('intent', 'acted', 'failed')
    AND NEW.reconciled_at IS NULL
    AND NOT (OLD.state = 'failed' AND OLD.intent_epoch IS NULL AND OLD.acted_at IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'a2m: a record returns to planned only after it was read back');
END;
