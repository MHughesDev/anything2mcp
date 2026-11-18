-- owner: a2m-ledger
-- A record whose only possible evidence is the system's own answer: a queue
-- take, a publish with no key, a call to an irreversible tool (decisions
-- 0161 and 0172). Such a record ends `acted`, on the acknowledgement, and is
-- never called verified and never counts as read back. It is settled, not in
-- doubt: its lock is released, a resume does not read it, and its run may
-- complete. What the ledger checks in code and no trigger can see: that the
-- run's own stored diff says the record's write is `acknowledged`, because
-- the diff is a sealed blob.

-- When the record ended on the system's acknowledgement. Set once, from
-- `acted`, with the evidence of the answer; never cleared.
ALTER TABLE run_records ADD COLUMN acknowledged_at INTEGER;

-- A record starts with no acknowledgement.
CREATE TRIGGER run_records_start_unacknowledged
BEFORE INSERT ON run_records
WHEN NEW.acknowledged_at IS NOT NULL
BEGIN
    SELECT RAISE(ABORT, 'a2m: a record starts with no acknowledgement');
END;

-- Rules 1, 3, and 4, for an acknowledgement: the record's intent was stored
-- and its write was sent under the run's current lease; the run is
-- `running`, cleared to write, irreversible, and has a stored diff; the
-- record stays `acted`; and the evidence of the answer is kept.
CREATE TRIGGER run_records_acknowledged_only_after_a_sent_write
BEFORE UPDATE OF acknowledged_at ON run_records
WHEN NEW.acknowledged_at IS NOT NULL AND (
    OLD.acknowledged_at IS NOT NULL
    OR OLD.state IS NOT 'acted' OR NEW.state IS NOT 'acted'
    OR OLD.intent_epoch IS NULL OR OLD.intent_seq IS NULL OR OLD.acted_at IS NULL
    OR NEW.evidence_ref IS NULL
    OR NOT EXISTS (
        SELECT 1 FROM runs
        WHERE id = NEW.run_id AND state = 'running' AND effect = 'irreversible'
          AND lease_epoch = OLD.intent_epoch AND diff_ref IS NOT NULL
          AND (approval_id IS NOT NULL OR decision_id IS NOT NULL)))
BEGIN
    SELECT RAISE(ABORT, 'a2m: a record ends on an acknowledgement only from acted, sent under the lease of a cleared irreversible run, with its evidence');
END;

-- Rule 2, kept: a record that ended on an acknowledgement is settled. It is
-- never verified, never failed, never planned again, and what it ended with
-- never changes.
CREATE TRIGGER run_records_acknowledged_is_settled
BEFORE UPDATE ON run_records
WHEN OLD.acknowledged_at IS NOT NULL AND (
    NEW.state IS NOT OLD.state
    OR NEW.acknowledged_at IS NOT OLD.acknowledged_at
    OR NEW.evidence_ref IS NOT OLD.evidence_ref
    OR NEW.read_back_hash IS NOT OLD.read_back_hash
    OR NEW.verified_at IS NOT OLD.verified_at
    OR NEW.intent_epoch IS NOT OLD.intent_epoch
    OR NEW.intent_seq IS NOT OLD.intent_seq
    OR NEW.acted_at IS NOT OLD.acted_at
    OR NEW.failure_class IS NOT OLD.failure_class
    OR NEW.failure_code IS NOT OLD.failure_code)
BEGIN
    SELECT RAISE(ABORT, 'a2m: a record that ended on an acknowledgement never changes');
END;

-- Rule 5, as migration 0012 held it, with one change: a record that ended
-- on an acknowledgement is not in doubt.
DROP TRIGGER run_records_no_intent_while_in_doubt;

CREATE TRIGGER run_records_no_intent_while_in_doubt
BEFORE UPDATE OF state ON run_records
WHEN NEW.state = 'intent' AND EXISTS (
    SELECT 1 FROM run_records AS other
    WHERE other.run_id = NEW.run_id AND other.intent_epoch < NEW.intent_epoch
      AND (other.state = 'intent'
           OR (other.state = 'acted' AND other.acknowledged_at IS NULL)
           OR (other.state = 'failed' AND other.reconciled_at IS NULL
               AND (other.acted_at IS NOT NULL
                    OR other.failure_class NOT IN ('policy', 'stale', 'input')))))
BEGIN
    SELECT RAISE(ABORT, 'a2m: a record of an earlier lease is in doubt and must be read back first');
END;

-- A run completes only when every record is verified, unchanged, or ended
-- on an acknowledgement. Until now the ledger's code alone held this.
CREATE TRIGGER runs_complete_only_when_every_record_is_settled
BEFORE UPDATE OF state ON runs
WHEN NEW.state = 'completed' AND OLD.state IS NOT 'completed' AND EXISTS (
    SELECT 1 FROM run_records
    WHERE run_id = NEW.id
      AND NOT (state IN ('verified', 'unchanged')
               OR (state = 'acted' AND acknowledged_at IS NOT NULL)))
BEGIN
    SELECT RAISE(ABORT, 'a2m: a run completes only when every record is verified, unchanged, or acknowledged');
END;
