-- owner: a2m-sched
-- Canaries: one published read with a fixed input, sent on a schedule its
-- person confirmed, to find whether the knowledge it goes through still
-- fits the system (decision 0482). A canary keeps nothing of what it read:
-- what it leaves are drift marks, kept by its drift and lifted by its pass
-- (decision 0380), and here its last outcome. Apart from a watch: it
-- compares no result, and it goes on at its interval after a drift.
--
-- One row per canary. What it reads, with which input, on which schedule,
-- and for whom never change once it is asked for. Its state moves `asked`
-- to `confirmed` by its person's act alone, `confirmed` to `running` while
-- one check is under way and back, and anything to `ended`, which is final.
-- A canary that is not confirmed is never checked. `next_due_index` is the
-- index of the next due time not yet handled: due time `n` is the moment of
-- confirmation, plus the phase, plus `n` intervals; it only grows. `missed`
-- counts the due times that passed with no check. `running_run` is the run
-- of the check under way, and `opened_run` whether that check opened it.
-- `outcome` is the last check's: `passed`, `drifted`, `failed`, or
-- `rested` (its run waited for a person), with the failure it gave. Never a
-- value. Moments are milliseconds since 1970, from the store's clock.
CREATE TABLE canaries (
    id TEXT PRIMARY KEY,
    workspace_id TEXT NOT NULL,
    target TEXT NOT NULL,
    environment TEXT NOT NULL,
    operation TEXT NOT NULL,
    target_version_hash TEXT NOT NULL,
    input_hash TEXT NOT NULL,
    input_ref TEXT,
    surface TEXT NOT NULL,
    requests_per_check INTEGER NOT NULL CHECK (requests_per_check > 0),
    interval_seconds INTEGER NOT NULL CHECK (interval_seconds > 0),
    ends_at INTEGER NOT NULL,
    asked_by TEXT NOT NULL,
    acts_for TEXT NOT NULL,
    asked_at INTEGER NOT NULL,
    confirmed_by TEXT,
    confirmed_at INTEGER,
    phase_ms INTEGER,
    state TEXT NOT NULL CHECK (state IN ('asked', 'confirmed', 'running', 'ended')),
    ended_reason TEXT CHECK (ended_reason IS NULL OR ended_reason IN (
        'end_reached', 'ended_by_person', 'ended_by_caller', 'version_changed')),
    next_due_index INTEGER NOT NULL DEFAULT 0 CHECK (next_due_index >= 0),
    not_before INTEGER,
    running_run TEXT,
    opened_run INTEGER NOT NULL DEFAULT 0 CHECK (opened_run IN (0, 1)),
    checks INTEGER NOT NULL DEFAULT 0 CHECK (checks >= 0),
    missed INTEGER NOT NULL DEFAULT 0 CHECK (missed >= 0),
    outcome TEXT CHECK (outcome IS NULL OR outcome IN ('passed', 'drifted', 'failed', 'rested')),
    outcome_class TEXT,
    outcome_code TEXT,
    outcome_at INTEGER,
    CHECK (ends_at > asked_at),
    CHECK ((confirmed_by IS NULL) = (confirmed_at IS NULL)),
    CHECK ((confirmed_by IS NULL) = (phase_ms IS NULL)),
    CHECK (confirmed_by IS NULL OR confirmed_by = acts_for),
    CHECK (state IN ('asked', 'ended') OR confirmed_by IS NOT NULL),
    CHECK ((state = 'ended') = (ended_reason IS NOT NULL)),
    CHECK ((state = 'running') = (running_run IS NOT NULL)),
    CHECK ((outcome_class IS NULL) = (outcome_code IS NULL))
) STRICT;

CREATE INDEX canaries_by_person ON canaries (workspace_id, acts_for, state);
CREATE INDEX canaries_by_run ON canaries (running_run) WHERE running_run IS NOT NULL;
