-- owner: a2m-ledger
-- Watches: one published read with a fixed input, repeated on a schedule a
-- person confirmed, and what changed between its checks (decision 0247,
-- which builds the ledger's half of decision 0234). A watch never writes and
-- keeps no value of what it read: only digests, counts, names of outputs,
-- and times. The values stay in the result of each check's run, kept like
-- any run's.
--
-- One row per watch. What it reads, with which input, on which schedule,
-- and for whom never change once it is asked for (a trigger below). Its
-- state moves `asked` to `confirmed` by a person's act, `confirmed` to
-- `running` while one check is under way and back, `confirmed` to `paused`
-- and back, and anything to `ended`, which is final. `next_due_index` is the
-- index of the next due time not yet handled: due time `n` is the moment of
-- confirmation, plus the phase, plus `n` intervals. It only grows, so no due
-- time is checked twice. `baseline_through` is the check whose digests the
-- baseline last took; `baseline_complete` says some complete check is in it.
CREATE TABLE watches (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    target TEXT NOT NULL,
    environment TEXT NOT NULL,
    operation TEXT NOT NULL,
    target_version_hash TEXT NOT NULL,
    effect TEXT NOT NULL CHECK (effect = 'read'),
    input_hash TEXT NOT NULL,
    input_ref TEXT REFERENCES blobs (hash),
    ignore_outputs TEXT NOT NULL,
    interval_seconds INTEGER NOT NULL CHECK (interval_seconds > 0),
    ends_at INTEGER NOT NULL,
    asked_by TEXT NOT NULL,
    acts_for TEXT NOT NULL,
    client_label TEXT NOT NULL,
    asked_at INTEGER NOT NULL,
    retention_class TEXT NOT NULL CHECK (retention_class IN ('short', 'long', 'longest')),
    confirmed_by TEXT,
    confirmed_at INTEGER,
    phase_ms INTEGER,
    state TEXT NOT NULL CHECK (state IN ('asked', 'confirmed', 'running', 'paused', 'ended')),
    reason_class TEXT,
    reason_code TEXT,
    state_since INTEGER NOT NULL,
    next_due_index INTEGER NOT NULL DEFAULT 0 CHECK (next_due_index >= 0),
    not_before INTEGER,
    running_check INTEGER,
    checks INTEGER NOT NULL DEFAULT 0 CHECK (checks >= 0),
    baseline_through INTEGER,
    baseline_complete INTEGER NOT NULL DEFAULT 0 CHECK (baseline_complete IN (0, 1)),
    retention_state TEXT NOT NULL DEFAULT 'kept' CHECK (retention_state IN ('kept', 'purging')),
    CHECK (ends_at > asked_at),
    CHECK ((confirmed_by IS NULL) = (confirmed_at IS NULL)),
    CHECK ((confirmed_by IS NULL) = (phase_ms IS NULL)),
    CHECK (phase_ms IS NULL OR (phase_ms >= 0 AND phase_ms < interval_seconds * 1000)),
    CHECK (confirmed_by IS NULL OR confirmed_by = acts_for),
    CHECK (state IN ('asked', 'ended') OR confirmed_by IS NOT NULL),
    CHECK ((state IN ('paused', 'ended')) = (reason_code IS NOT NULL)),
    CHECK ((state = 'running') = (running_check IS NOT NULL))
) STRICT;

CREATE INDEX watches_open ON watches (state, id);
CREATE INDEX watches_by_person ON watches (workspace_id, acts_for, state);

-- One row per check: the due time it was for, whether it was the one
-- check made after due times were missed, the run of the read, and how it
-- ended, with counts and never a value. A check is written once when it
-- begins and once when it ends, and never again.
CREATE TABLE watch_checks (
    watch_id TEXT NOT NULL REFERENCES watches (id),
    seq INTEGER NOT NULL CHECK (seq >= 0),
    due_index INTEGER NOT NULL CHECK (due_index >= 0),
    due_at INTEGER NOT NULL,
    catch_up INTEGER NOT NULL CHECK (catch_up IN (0, 1)),
    run_id TEXT NOT NULL REFERENCES runs (id),
    started_at INTEGER NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN (
        'running', 'complete', 'cut_short', 'failed', 'faulted')),
    ended_at INTEGER,
    why_class TEXT,
    why_code TEXT,
    records_seen INTEGER CHECK (records_seen IS NULL OR records_seen >= 0),
    added INTEGER,
    changed INTEGER,
    removed INTEGER,
    unseen INTEGER,
    unchanged INTEGER,
    PRIMARY KEY (watch_id, seq),
    UNIQUE (watch_id, due_index),
    CHECK ((outcome = 'running') = (ended_at IS NULL)),
    CHECK ((outcome IN ('failed', 'faulted')) = (why_code IS NOT NULL)),
    CHECK ((outcome IN ('complete', 'cut_short')) = (records_seen IS NOT NULL))
) STRICT;

CREATE INDEX watch_checks_by_run ON watch_checks (run_id);

-- What the watch tells: one row per change, never changed. `key_digest` is
-- the workspace's keyed digest of the record's key under the purpose
-- `record_key`, as the ledger names a key in a record row and a lock, or
-- `whole` for a read compared whole. `outputs` holds, for `changed`, the
-- names of the outputs that differ, as canonical JSON, never their values.
-- `older_check` is the check that last saw the record. A gap keeps the
-- class and code of what made it and, for due times that passed, the first
-- and last of them and how many.
CREATE TABLE watch_changes (
    seq INTEGER PRIMARY KEY,
    watch_id TEXT NOT NULL REFERENCES watches (id),
    kind TEXT NOT NULL CHECK (kind IN (
        'confirmed', 'added', 'changed', 'removed', 'unseen', 'gap', 'paused', 'resumed',
        'ended')),
    check_seq INTEGER,
    older_check INTEGER,
    key_digest TEXT,
    outputs TEXT,
    why_class TEXT,
    why_code TEXT,
    first_missed INTEGER,
    last_missed INTEGER,
    missed INTEGER CHECK (missed IS NULL OR missed > 0),
    at INTEGER NOT NULL,
    CHECK ((kind = 'changed') = (outputs IS NOT NULL)),
    CHECK (kind NOT IN ('added', 'changed', 'removed', 'unseen')
           OR (check_seq IS NOT NULL AND key_digest IS NOT NULL)),
    CHECK ((first_missed IS NULL) = (last_missed IS NULL)),
    CHECK ((first_missed IS NULL) = (missed IS NULL)),
    CHECK (kind NOT IN ('gap', 'paused', 'ended') OR why_code IS NOT NULL)
) STRICT;

CREATE INDEX watch_changes_by_watch ON watch_changes (watch_id, seq);

-- The baseline: for each record the watch has seen, the digest of its
-- normalized values and of each output, under the purpose of a watch's
-- values, and the check that last saw it. A newer check's digests replace
-- the older ones; a record proven gone is removed; the whole baseline goes
-- when the watch ends. Never a value.
CREATE TABLE watch_baseline (
    watch_id TEXT NOT NULL REFERENCES watches (id),
    key_digest TEXT NOT NULL,
    record_digest TEXT NOT NULL,
    outputs TEXT NOT NULL,
    last_seen INTEGER NOT NULL CHECK (last_seen >= 0),
    PRIMARY KEY (watch_id, key_digest)
) STRICT;

-- What a watch reads, with which input, on which schedule, and for whom,
-- never changes. A new read or a new input is a new watch, confirmed again.
CREATE TRIGGER watches_what_they_read_never_changes
BEFORE UPDATE ON watches
WHEN NEW.id IS NOT OLD.id
  OR NEW.workspace_id IS NOT OLD.workspace_id
  OR NEW.target IS NOT OLD.target
  OR NEW.environment IS NOT OLD.environment
  OR NEW.operation IS NOT OLD.operation
  OR NEW.target_version_hash IS NOT OLD.target_version_hash
  OR NEW.effect IS NOT OLD.effect
  OR NEW.input_hash IS NOT OLD.input_hash
  OR NEW.input_ref IS NOT OLD.input_ref
  OR NEW.ignore_outputs IS NOT OLD.ignore_outputs
  OR NEW.interval_seconds IS NOT OLD.interval_seconds
  OR NEW.ends_at IS NOT OLD.ends_at
  OR NEW.asked_by IS NOT OLD.asked_by
  OR NEW.acts_for IS NOT OLD.acts_for
  OR NEW.client_label IS NOT OLD.client_label
  OR NEW.asked_at IS NOT OLD.asked_at
  OR NEW.retention_class IS NOT OLD.retention_class
BEGIN
    SELECT RAISE(ABORT, 'a2m: what a watch reads, with which input and schedule, never changes');
END;

-- A watch is confirmed once, while it is asked, and the confirmation stays.
CREATE TRIGGER watches_are_confirmed_once
BEFORE UPDATE ON watches
WHEN (OLD.confirmed_by IS NOT NULL
      AND (NEW.confirmed_by IS NOT OLD.confirmed_by
           OR NEW.confirmed_at IS NOT OLD.confirmed_at
           OR NEW.phase_ms IS NOT OLD.phase_ms))
  OR (OLD.confirmed_by IS NULL AND NEW.confirmed_by IS NOT NULL AND OLD.state <> 'asked')
BEGIN
    SELECT RAISE(ABORT, 'a2m: a watch is confirmed once, while it is asked');
END;

-- A check begins only on a confirmed watch: never before the person
-- confirmed it, never while another check of it is under way.
CREATE TRIGGER watches_run_one_check_at_a_time
BEFORE UPDATE ON watches
WHEN NEW.state = 'running'
  AND (OLD.state NOT IN ('confirmed', 'running')
       OR (OLD.state = 'running' AND NEW.running_check IS NOT OLD.running_check))
BEGIN
    SELECT RAISE(ABORT, 'a2m: a check begins only on a confirmed watch with no check under way');
END;

-- No due time is handled twice.
CREATE TRIGGER watches_due_times_only_go_forward
BEFORE UPDATE ON watches
WHEN NEW.next_due_index < OLD.next_due_index
BEGIN
    SELECT RAISE(ABORT, 'a2m: a watch''s due times only go forward');
END;

-- An ended watch never changes, except to be marked for the retention job.
CREATE TRIGGER watches_ended_never_change
BEFORE UPDATE ON watches
WHEN OLD.state = 'ended'
  AND (NEW.state IS NOT OLD.state
       OR NEW.reason_class IS NOT OLD.reason_class
       OR NEW.reason_code IS NOT OLD.reason_code
       OR NEW.state_since IS NOT OLD.state_since
       OR NEW.next_due_index IS NOT OLD.next_due_index
       OR NEW.not_before IS NOT OLD.not_before
       OR NEW.running_check IS NOT OLD.running_check
       OR NEW.checks IS NOT OLD.checks
       OR NEW.baseline_through IS NOT OLD.baseline_through
       OR NEW.baseline_complete IS NOT OLD.baseline_complete
       OR NEW.confirmed_by IS NOT OLD.confirmed_by)
BEGIN
    SELECT RAISE(ABORT, 'a2m: an ended watch never changes');
END;

CREATE TRIGGER watches_leave_only_with_retention
BEFORE DELETE ON watches
WHEN OLD.retention_state IS NOT 'purging'
BEGIN
    SELECT RAISE(ABORT, 'a2m: a watch and its history leave only with the retention job');
END;

-- A check is the watch's own read and nothing else: a read of the watch's
-- operation, version, and input, for the person it acts for, in its
-- workspace, target, and environment, begun while the watch is running it.
CREATE TRIGGER watch_checks_run_only_their_own_read
BEFORE INSERT ON watch_checks
WHEN NOT EXISTS (
    SELECT 1 FROM watches w JOIN runs r ON r.id = NEW.run_id
    WHERE w.id = NEW.watch_id
      AND w.state = 'running' AND w.running_check = NEW.seq
      AND r.effect = 'read'
      AND r.tool_name = w.operation
      AND r.target_version_hash = w.target_version_hash
      AND r.input_hash = w.input_hash
      AND r.principal_id = w.acts_for
      AND r.workspace_id = w.workspace_id
      AND r.target = w.target
      AND r.environment = w.environment)
BEGIN
    SELECT RAISE(ABORT, 'a2m: a check runs only its watch''s own read, while the watch runs it');
END;

-- A check ends once and is never changed after.
CREATE TRIGGER watch_checks_end_once
BEFORE UPDATE ON watch_checks
WHEN OLD.outcome <> 'running'
  OR NEW.watch_id IS NOT OLD.watch_id
  OR NEW.seq IS NOT OLD.seq
  OR NEW.due_index IS NOT OLD.due_index
  OR NEW.due_at IS NOT OLD.due_at
  OR NEW.catch_up IS NOT OLD.catch_up
  OR NEW.run_id IS NOT OLD.run_id
  OR NEW.started_at IS NOT OLD.started_at
BEGIN
    SELECT RAISE(ABORT, 'a2m: a check ends once and never changes after');
END;

CREATE TRIGGER watch_checks_leave_only_with_their_watch
BEFORE DELETE ON watch_checks
WHEN EXISTS (SELECT 1 FROM watches WHERE id = OLD.watch_id AND retention_state IS NOT 'purging')
BEGIN
    SELECT RAISE(ABORT, 'a2m: a check leaves only with its watch');
END;

CREATE TRIGGER watch_changes_never_change
BEFORE UPDATE ON watch_changes
BEGIN
    SELECT RAISE(ABORT, 'a2m: a watch''s change never changes');
END;

CREATE TRIGGER watch_changes_leave_only_with_their_watch
BEFORE DELETE ON watch_changes
WHEN EXISTS (SELECT 1 FROM watches WHERE id = OLD.watch_id AND retention_state IS NOT 'purging')
BEGIN
    SELECT RAISE(ABORT, 'a2m: a watch''s change leaves only with its watch');
END;

-- An ended watch keeps no digests.
CREATE TRIGGER watch_baseline_only_while_open
BEFORE INSERT ON watch_baseline
WHEN NOT EXISTS (SELECT 1 FROM watches WHERE id = NEW.watch_id AND state = 'running')
BEGIN
    SELECT RAISE(ABORT, 'a2m: a baseline is written only by a check under way');
END;

CREATE TRIGGER watch_baseline_changes_only_while_open
BEFORE UPDATE ON watch_baseline
WHEN NOT EXISTS (SELECT 1 FROM watches WHERE id = NEW.watch_id AND state = 'running')
BEGIN
    SELECT RAISE(ABORT, 'a2m: a baseline is written only by a check under way');
END;
