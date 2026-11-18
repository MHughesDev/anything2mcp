-- owner: a2m-ledger
-- A watch names what the built halves need to hold it to its budget and to
-- read its records from its checks' own results (decision 0262): the
-- surface its read uses, how many requests one check sends, and the shape
-- of its records. All three come from the published plan, never from the
-- caller's input, and never change once the watch is asked for.
--
-- `record_shape` is canonical JSON: `{"whole":true}` for a read compared
-- whole, or `{"key":[<field>, ...],"list":<output>}` for a read whose output
-- `list` is a list of records keyed by those fields, in order.
--
-- No store held a watch before this migration: no action could ask for one.
-- A row from before would hold the defaults, which the trigger below
-- refuses for every new row.
ALTER TABLE watches ADD COLUMN surface TEXT NOT NULL DEFAULT '';
ALTER TABLE watches ADD COLUMN requests_per_check INTEGER NOT NULL DEFAULT 0
    CHECK (requests_per_check >= 0);
ALTER TABLE watches ADD COLUMN record_shape TEXT NOT NULL DEFAULT '';

-- A new watch names its surface, a cost of at least one request, and its
-- shape; and keeps to the floor of 60 seconds and the ceiling of 366 days,
-- whoever sends the statement. The installation's settings narrow both in
-- the scheduler; neither can widen them.
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

CREATE INDEX watches_by_surface ON watches (workspace_id, target, environment, surface, state);

-- A shadow comparison names the purpose its input was digested under. From
-- this migration on every comparison is digested under `shadow_input`; one
-- kept before was digested under `caller_key`, with a label of its own.
-- The two digests of one input differ, so counting both would count one
-- input twice: only comparisons of the purpose in force count as distinct
-- inputs, and older ones count as none, which can only understate the
-- evidence.
ALTER TABLE shadow_comparisons ADD COLUMN input_purpose TEXT NOT NULL DEFAULT 'caller_key'
    CHECK (input_purpose IN ('caller_key', 'shadow_input'));
