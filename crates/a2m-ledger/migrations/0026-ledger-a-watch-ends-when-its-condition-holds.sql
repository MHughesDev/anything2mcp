-- owner: a2m-ledger
-- A watch may carry a condition: an expression over a check's result, in
-- the read's output names, that ends the watch when it holds (decisions
-- 0234 and 0291). The check that finds it holding records its changes as
-- any check does, then `condition_met`, and the watch ends with the reason
-- `condition_met`.
--
-- `condition` is canonical JSON of the plan's form of an expression
-- (`{"library":<n>,"result":<type>,"source":<text>}`), checked when the
-- watch is asked and never changed; null for a watch with none, as every
-- watch asked before this migration is.
--
-- The words a watch's reason and a change's kind may take are column
-- checks of migration 0021, and SQLite cannot widen a check in place, so
-- `watches` and `watch_changes` are built again with the new word and the
-- rows copied across unchanged. Their triggers and indexes go with the old
-- tables and are created again below, as migrations 0021 and 0023 made
-- them, with `condition` among what never changes. The other tables'
-- references to `watches` name it by name and hold across; their foreign
-- keys are checked when this migration's transaction commits.
PRAGMA defer_foreign_keys = ON;

CREATE TEMP TABLE watches_before_0026 AS SELECT * FROM main.watches;
CREATE TEMP TABLE watch_changes_before_0026 AS SELECT * FROM main.watch_changes;

DROP TABLE main.watch_changes;
DROP TABLE main.watches;

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
    reason TEXT CHECK (reason IS NULL OR reason IN (
        'needs_login', 'needs_input', 'drifted', 'version_changed', 'on_hold', 'paused_by_person',
        'ended_by_person', 'ended_by_caller', 'end_reached', 'version_withdrawn', 'declined',
        'condition_met')),
    reason_failure_class TEXT,
    reason_failure_code TEXT,
    state_since INTEGER NOT NULL,
    next_due_index INTEGER NOT NULL DEFAULT 0 CHECK (next_due_index >= 0),
    not_before INTEGER,
    running_check INTEGER,
    checks INTEGER NOT NULL DEFAULT 0 CHECK (checks >= 0),
    baseline_through INTEGER,
    baseline_complete INTEGER NOT NULL DEFAULT 0 CHECK (baseline_complete IN (0, 1)),
    retention_state TEXT NOT NULL DEFAULT 'kept' CHECK (retention_state IN ('kept', 'purging')),
    surface TEXT NOT NULL DEFAULT '',
    requests_per_check INTEGER NOT NULL DEFAULT 0 CHECK (requests_per_check >= 0),
    record_shape TEXT NOT NULL DEFAULT '',
    condition TEXT CHECK (condition IS NULL OR condition <> ''),
    CHECK (ends_at > asked_at),
    CHECK ((confirmed_by IS NULL) = (confirmed_at IS NULL)),
    CHECK ((confirmed_by IS NULL) = (phase_ms IS NULL)),
    CHECK (phase_ms IS NULL OR (phase_ms >= 0 AND phase_ms < interval_seconds * 1000)),
    CHECK (confirmed_by IS NULL OR confirmed_by = acts_for),
    CHECK (state IN ('asked', 'ended') OR confirmed_by IS NOT NULL),
    CHECK ((state IN ('paused', 'ended')) = (reason IS NOT NULL)),
    CHECK ((reason_failure_class IS NULL) = (reason_failure_code IS NULL)),
    CHECK ((state = 'running') = (running_check IS NOT NULL))
) STRICT;

INSERT INTO watches (
    id, workspace_id, target, environment, operation, target_version_hash, effect, input_hash,
    input_ref, ignore_outputs, interval_seconds, ends_at, asked_by, acts_for, client_label,
    asked_at, retention_class, confirmed_by, confirmed_at, phase_ms, state, reason,
    reason_failure_class, reason_failure_code, state_since, next_due_index, not_before,
    running_check, checks, baseline_through, baseline_complete, retention_state, surface,
    requests_per_check, record_shape, condition)
SELECT
    id, workspace_id, target, environment, operation, target_version_hash, effect, input_hash,
    input_ref, ignore_outputs, interval_seconds, ends_at, asked_by, acts_for, client_label,
    asked_at, retention_class, confirmed_by, confirmed_at, phase_ms, state, reason,
    reason_failure_class, reason_failure_code, state_since, next_due_index, not_before,
    running_check, checks, baseline_through, baseline_complete, retention_state, surface,
    requests_per_check, record_shape, NULL
FROM temp.watches_before_0026;

CREATE TABLE watch_changes (
    seq INTEGER PRIMARY KEY,
    watch_id TEXT NOT NULL REFERENCES watches (id),
    kind TEXT NOT NULL CHECK (kind IN (
        'confirmed', 'added', 'changed', 'removed', 'unseen', 'gap', 'paused', 'resumed',
        'ended', 'condition_met')),
    check_seq INTEGER,
    older_check INTEGER,
    key_digest TEXT,
    outputs TEXT,
    why_class TEXT,
    why_code TEXT,
    reason TEXT CHECK (reason IS NULL OR reason IN (
        'needs_login', 'needs_input', 'drifted', 'version_changed', 'on_hold', 'paused_by_person',
        'ended_by_person', 'ended_by_caller', 'end_reached', 'version_withdrawn', 'declined',
        'condition_met')),
    first_missed INTEGER,
    last_missed INTEGER,
    missed INTEGER CHECK (missed IS NULL OR missed > 0),
    at INTEGER NOT NULL,
    CHECK ((kind = 'changed') = (outputs IS NOT NULL)),
    CHECK (kind NOT IN ('added', 'changed', 'removed', 'unseen')
           OR (check_seq IS NOT NULL AND key_digest IS NOT NULL)),
    CHECK (kind <> 'condition_met' OR check_seq IS NOT NULL),
    CHECK ((first_missed IS NULL) = (last_missed IS NULL)),
    CHECK ((first_missed IS NULL) = (missed IS NULL)),
    CHECK ((why_class IS NULL) = (why_code IS NULL)),
    CHECK (kind <> 'gap' OR why_code IS NOT NULL),
    CHECK ((kind IN ('paused', 'ended')) = (reason IS NOT NULL))
) STRICT;

INSERT INTO watch_changes (
    seq, watch_id, kind, check_seq, older_check, key_digest, outputs, why_class, why_code,
    reason, first_missed, last_missed, missed, at)
SELECT
    seq, watch_id, kind, check_seq, older_check, key_digest, outputs, why_class, why_code,
    reason, first_missed, last_missed, missed, at
FROM temp.watch_changes_before_0026;

DROP TABLE temp.watch_changes_before_0026;
DROP TABLE temp.watches_before_0026;

CREATE INDEX watches_open ON watches (state, id);
CREATE INDEX watches_by_person ON watches (workspace_id, acts_for, state);
CREATE INDEX watches_by_surface ON watches (workspace_id, target, environment, surface, state);
CREATE INDEX watch_changes_by_watch ON watch_changes (watch_id, seq);

-- What a watch reads, with which input, on which schedule, for whom, and
-- until which condition, never changes. A new read, input, or condition is
-- a new watch, confirmed again.
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
  OR NEW.condition IS NOT OLD.condition
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
       OR NEW.reason IS NOT OLD.reason
       OR NEW.reason_failure_class IS NOT OLD.reason_failure_class
       OR NEW.reason_failure_code IS NOT OLD.reason_failure_code
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

-- A new watch names its surface, a cost of at least one request, and its
-- shape; and keeps to the floor of 60 seconds and the ceiling of 366 days,
-- whoever sends the statement.
CREATE TRIGGER watches_name_their_surface_cost_and_shape
BEFORE INSERT ON watches
WHEN NEW.surface = '' OR NEW.requests_per_check < 1 OR NEW.record_shape = ''
  OR NEW.interval_seconds < 60
  OR NEW.ends_at - NEW.asked_at > 366 * 86400000
BEGIN
    SELECT RAISE(ABORT, 'a2m: a watch names its surface, its cost, and its shape, and keeps to the floor and the ceiling');
END;

CREATE TRIGGER watches_surface_cost_and_shape_never_change
BEFORE UPDATE ON watches
WHEN NEW.surface IS NOT OLD.surface
  OR NEW.requests_per_check IS NOT OLD.requests_per_check
  OR NEW.record_shape IS NOT OLD.record_shape
BEGIN
    SELECT RAISE(ABORT, 'a2m: what a watch reads, with which input and schedule, never changes');
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

-- A watch's condition holds only on a check of it: the change names the
-- check, and the watch ends with the same word in the same transaction.
CREATE TRIGGER watch_changes_condition_met_names_its_check
BEFORE INSERT ON watch_changes
WHEN NEW.kind = 'condition_met'
  AND NOT EXISTS (SELECT 1 FROM watch_checks
                  WHERE watch_id = NEW.watch_id AND seq = NEW.check_seq)
BEGIN
    SELECT RAISE(ABORT, 'a2m: a condition is met only on a check of its watch');
END;
