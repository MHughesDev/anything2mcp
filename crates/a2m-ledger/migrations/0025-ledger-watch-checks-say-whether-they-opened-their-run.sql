-- owner: a2m-ledger
-- A check says whether it opened its run (decisions 0285 and 0287). Two
-- watches of one read and input for one person share a run under ledger
-- rule 6, as a check shares the run of the person's own identical call:
-- the scheduler's admission then hands back the run that exists, and the
-- check opens none. A check that only joined a run never cancels it, and a
-- run some check opened is cancelled only once no check under way names it.
--
-- `opened_run` is 1 for a check whose admission opened the run, and 0 for
-- one handed a run that already existed. A row from before this migration
-- holds 1: the engine then treated every check as the opener of its run,
-- so that a check under way at the upgrade is ended as it would have been
-- before. Two checks under way on one run were not told apart then either.
ALTER TABLE watch_checks ADD COLUMN opened_run INTEGER NOT NULL DEFAULT 1
    CHECK (opened_run IN (0, 1));

-- One check at most opens a run, and only a run no check names yet: a run
-- already named was opened before, by that check or by someone else.
CREATE TRIGGER watch_checks_open_only_a_new_run
BEFORE INSERT ON watch_checks
WHEN NEW.opened_run = 1
  AND EXISTS (SELECT 1 FROM watch_checks WHERE run_id = NEW.run_id)
BEGIN
    SELECT RAISE(ABORT, 'a2m: a check opens only a run no check names yet');
END;

-- Whether a check opened its run never changes.
CREATE TRIGGER watch_checks_opened_run_never_changes
BEFORE UPDATE ON watch_checks
WHEN NEW.opened_run IS NOT OLD.opened_run
BEGIN
    SELECT RAISE(ABORT, 'a2m: a check ends once and never changes after');
END;
