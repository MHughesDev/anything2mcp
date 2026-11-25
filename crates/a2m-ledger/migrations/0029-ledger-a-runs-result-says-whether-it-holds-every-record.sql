-- owner: a2m-ledger
-- A run's result says how much of what its read could return it holds
-- (decision 0472): `complete` when the driver that read it stated that the
-- system holds no record the read did not return, `partial` when it stated
-- that the system holds more, and `unknown` when it stated nothing, which is
-- every run before this migration and every run of a driver that cannot
-- tell. A watch reports a record removed only between two checks whose
-- runs' results are `complete`. The engine states it with the result, in
-- the same write, and it never changes after: a run in a final state keeps
-- it, and no run states it without a result.
ALTER TABLE runs ADD COLUMN result_coverage TEXT NOT NULL DEFAULT 'unknown'
    CHECK (result_coverage IN ('complete', 'partial', 'unknown'));

CREATE TRIGGER runs_result_coverage_is_final
BEFORE UPDATE OF result_coverage ON runs
WHEN NEW.result_coverage IS NOT OLD.result_coverage
    AND (OLD.state IN ('rejected', 'expired', 'completed', 'failed', 'cancelled')
        OR (NEW.result_coverage IS NOT 'unknown' AND NEW.result_ref IS NULL))
BEGIN
    SELECT RAISE(ABORT, 'a2m: a run states its coverage with its result, and never after it ends');
END;
