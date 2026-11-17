-- owner: a2m-ledger
-- Runs, their events, the ledger of commit points, and record locks (spec
-- section 208). The triggers hold the nine rules where the data is: they
-- refuse what the rules forbid whoever sends the statement.

CREATE TABLE runs (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    principal_id TEXT NOT NULL,
    client_label TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    tool_kind TEXT NOT NULL CHECK (tool_kind IN ('operation', 'pipeline')),
    target TEXT NOT NULL,
    target_version_hash TEXT NOT NULL,
    environment TEXT NOT NULL,
    identity_ref TEXT,
    effect TEXT NOT NULL CHECK (effect IN ('read', 'write', 'irreversible')),
    input_hash TEXT NOT NULL,
    input_ref TEXT REFERENCES blobs (hash),
    dedupe_key TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN (
        'queued', 'validating', 'rejected', 'needs_approval', 'expired', 'running',
        'needs_input', 'needs_login', 'stopped', 'completed', 'failed', 'cancelled')),
    state_class TEXT,
    state_code TEXT,
    attempt INTEGER NOT NULL,
    lease_epoch INTEGER NOT NULL,
    diff_hash TEXT,
    diff_ref TEXT REFERENCES blobs (hash),
    approval_id TEXT,
    decision_id TEXT,
    result_ref TEXT REFERENCES blobs (hash),
    trace_ref TEXT REFERENCES blobs (hash),
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    finished_at INTEGER,
    retention_class TEXT NOT NULL CHECK (retention_class IN ('short', 'long', 'longest')),
    retention_state TEXT NOT NULL DEFAULT 'kept' CHECK (retention_state IN ('kept', 'purging')),
    legal_hold INTEGER NOT NULL DEFAULT 0 CHECK (legal_hold IN (0, 1))
) STRICT;

-- Rule 6: at most one unfinished run for one caller, tool version, and input.
CREATE UNIQUE INDEX runs_one_unfinished ON runs (dedupe_key)
WHERE state NOT IN ('rejected', 'expired', 'completed', 'failed', 'cancelled');
CREATE INDEX runs_by_workspace_state ON runs (workspace_id, state);
CREATE INDEX runs_by_principal ON runs (principal_id, created_at);

-- Rule 8: a run in a final state never changes again. Its retention, its
-- legal hold, and a trace that arrives late are not part of what it was.
CREATE TRIGGER runs_final_means_final
BEFORE UPDATE ON runs
WHEN OLD.state IN ('rejected', 'expired', 'completed', 'failed', 'cancelled')
    AND (NEW.id IS NOT OLD.id
        OR NEW.state IS NOT OLD.state
        OR NEW.state_class IS NOT OLD.state_class
        OR NEW.state_code IS NOT OLD.state_code
        OR NEW.attempt IS NOT OLD.attempt
        OR NEW.lease_epoch IS NOT OLD.lease_epoch
        OR NEW.input_hash IS NOT OLD.input_hash
        OR NEW.dedupe_key IS NOT OLD.dedupe_key
        OR NEW.diff_hash IS NOT OLD.diff_hash
        OR NEW.diff_ref IS NOT OLD.diff_ref
        OR NEW.approval_id IS NOT OLD.approval_id
        OR NEW.decision_id IS NOT OLD.decision_id
        OR NEW.result_ref IS NOT OLD.result_ref
        OR NEW.finished_at IS NOT OLD.finished_at)
BEGIN
    SELECT RAISE(ABORT, 'a2m: a run in a final state never changes');
END;

CREATE TRIGGER runs_lease_only_grows
BEFORE UPDATE OF lease_epoch ON runs
WHEN NEW.lease_epoch < OLD.lease_epoch
BEGIN
    SELECT RAISE(ABORT, 'a2m: a lease number only grows');
END;

CREATE TRIGGER runs_leave_by_retention_only
BEFORE DELETE ON runs
WHEN OLD.retention_state IS NOT 'purging'
BEGIN
    SELECT RAISE(ABORT, 'a2m: a run is removed only by retention');
END;

CREATE TABLE run_events (
    run_id TEXT NOT NULL REFERENCES runs (id),
    seq INTEGER NOT NULL,
    epoch INTEGER NOT NULL,
    at INTEGER NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN (
        'state_changed', 'step_started', 'binding_chosen', 'intent', 'acted', 'verified',
        'failed', 'reconcile_read', 'approval_bound', 'lease_granted', 'lease_lost', 'need',
        'late_evidence', 'planned', 'diff_stored', 'lock')),
    payload TEXT NOT NULL,
    evidence_ref TEXT REFERENCES blobs (hash),
    PRIMARY KEY (run_id, seq)
) STRICT;

-- Rule 9: history is append-only, except that retention removes whole runs.
CREATE TRIGGER run_events_never_change
BEFORE UPDATE ON run_events
BEGIN
    SELECT RAISE(ABORT, 'a2m: history is append-only');
END;

CREATE TRIGGER run_events_leave_by_retention_only
BEFORE DELETE ON run_events
WHEN (SELECT retention_state FROM runs WHERE id = OLD.run_id) IS NOT 'purging'
BEGIN
    SELECT RAISE(ABORT, 'a2m: history is append-only');
END;

CREATE TRIGGER run_events_take_the_next_number
BEFORE INSERT ON run_events
WHEN NEW.seq IS NOT (SELECT COALESCE(MAX(seq), 0) + 1 FROM run_events WHERE run_id = NEW.run_id)
BEGIN
    SELECT RAISE(ABORT, 'a2m: an event takes the next sequence number of its run');
END;

-- Rule 4, for events: nothing from an older lease is recorded, except
-- evidence that arrives late.
CREATE TRIGGER run_events_are_fenced
BEFORE INSERT ON run_events
WHEN NEW.kind IS NOT 'late_evidence'
    AND NEW.epoch IS NOT (SELECT lease_epoch FROM runs WHERE id = NEW.run_id)
BEGIN
    SELECT RAISE(ABORT, 'a2m: an event from another lease is refused');
END;

CREATE TRIGGER run_events_stop_at_the_final_state
BEFORE INSERT ON run_events
WHEN NEW.kind IS NOT 'late_evidence'
    AND (SELECT state FROM runs WHERE id = NEW.run_id)
        IN ('rejected', 'expired', 'completed', 'failed', 'cancelled')
BEGIN
    SELECT RAISE(ABORT, 'a2m: a run in a final state never changes');
END;

-- One row per commit point in the system.
CREATE TABLE run_records (
    run_id TEXT NOT NULL REFERENCES runs (id),
    step_seq INTEGER NOT NULL,
    record_seq INTEGER NOT NULL,
    record_key TEXT NOT NULL,
    record_key_hash TEXT NOT NULL,
    operation TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('planned', 'unchanged', 'intent', 'acted', 'verified', 'failed')),
    intended_hash TEXT NOT NULL,
    intended_ref TEXT REFERENCES blobs (hash),
    before_ref TEXT REFERENCES blobs (hash),
    after_ref TEXT REFERENCES blobs (hash),
    read_back_hash TEXT,
    idempotency_token TEXT NOT NULL,
    assigned_key TEXT,
    outputs_ref TEXT REFERENCES blobs (hash),
    skip_reason TEXT,
    binding_mode TEXT,
    intent_epoch INTEGER,
    intent_seq INTEGER,
    attempts INTEGER NOT NULL DEFAULT 0,
    failure_class TEXT,
    failure_code TEXT,
    evidence_ref TEXT REFERENCES blobs (hash),
    intent_at INTEGER,
    acted_at INTEGER,
    verified_at INTEGER,
    PRIMARY KEY (run_id, step_seq, record_seq),
    UNIQUE (run_id, idempotency_token)
) STRICT;

CREATE INDEX run_records_by_state ON run_records (run_id, state);
CREATE INDEX run_records_by_key ON run_records (record_key_hash);

CREATE TRIGGER run_records_start_planned
BEFORE INSERT ON run_records
WHEN NEW.state NOT IN ('planned', 'unchanged')
BEGIN
    SELECT RAISE(ABORT, 'a2m: a record starts planned or unchanged');
END;

-- The moves a record may make. Anything else skips a state.
CREATE TRIGGER run_records_move_by_the_rules
BEFORE UPDATE OF state ON run_records
WHEN NEW.state IS NOT OLD.state AND NOT (
    (OLD.state = 'planned' AND NEW.state IN ('intent', 'failed'))
    OR (OLD.state = 'intent' AND NEW.state IN ('acted', 'verified', 'planned', 'failed'))
    OR (OLD.state = 'acted' AND NEW.state IN ('verified', 'planned', 'failed'))
    OR (OLD.state = 'failed' AND NEW.state IN ('verified', 'planned')))
BEGIN
    SELECT RAISE(ABORT, 'a2m: a record cannot make that move');
END;

-- Rule 1: acted only after an intent that was stored under a lease.
CREATE TRIGGER run_records_intent_before_action
BEFORE UPDATE OF state ON run_records
WHEN NEW.state = 'acted'
    AND (OLD.state IS NOT 'intent' OR OLD.intent_epoch IS NULL OR OLD.intent_seq IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'a2m: a record is acted only after its intent is stored');
END;

-- Rule 2: verified only with a read-back that equals the intent, and evidence.
CREATE TRIGGER run_records_proof_before_verified
BEFORE UPDATE OF state ON run_records
WHEN NEW.state = 'verified'
    AND (NEW.read_back_hash IS NOT NEW.intended_hash OR NEW.evidence_ref IS NULL)
BEGIN
    SELECT RAISE(ABORT, 'a2m: a record is verified only with a matching read-back and its evidence');
END;

-- Rule 4, for records: an intent is stored under the run's current lease.
CREATE TRIGGER run_records_intent_under_the_lease
BEFORE UPDATE OF state ON run_records
WHEN NEW.state = 'intent'
    AND NEW.intent_epoch IS NOT (SELECT lease_epoch FROM runs WHERE id = NEW.run_id)
BEGIN
    SELECT RAISE(ABORT, 'a2m: an intent is stored under the current lease');
END;

-- Rule 8, for records.
CREATE TRIGGER run_records_stop_at_the_final_state
BEFORE UPDATE ON run_records
WHEN (SELECT state FROM runs WHERE id = NEW.run_id)
    IN ('rejected', 'expired', 'completed', 'failed', 'cancelled')
BEGIN
    SELECT RAISE(ABORT, 'a2m: a run in a final state never changes');
END;

CREATE TRIGGER run_records_leave_by_retention_only
BEFORE DELETE ON run_records
WHEN (SELECT retention_state FROM runs WHERE id = OLD.run_id) IS NOT 'purging'
BEGIN
    SELECT RAISE(ABORT, 'a2m: a record is removed only by retention');
END;

-- Rule 7: at most one run holds the lock for a record key in a target and
-- environment. The primary key is the rule.
CREATE TABLE record_locks (
    workspace_id TEXT NOT NULL,
    target TEXT NOT NULL,
    environment TEXT NOT NULL,
    record_key_hash TEXT NOT NULL,
    run_id TEXT NOT NULL REFERENCES runs (id),
    epoch INTEGER NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('held', 'quarantined')),
    acquired_at INTEGER NOT NULL,
    PRIMARY KEY (workspace_id, target, environment, record_key_hash)
) STRICT;

CREATE INDEX record_locks_by_run ON record_locks (run_id);
