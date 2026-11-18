-- owner: a2m-ledger
-- A published version's gate evidence is written once, with the version,
-- and never changed (decision 0196, audit finding LE-1): the rule is held
-- where the data is, as migration 0003 holds the version's plan.
CREATE TRIGGER target_versions_keep_their_gate_evidence
BEFORE UPDATE OF gate_evidence ON target_versions
WHEN NEW.gate_evidence IS NOT OLD.gate_evidence
BEGIN
    SELECT RAISE(ABORT, 'a2m: a published version''s gate evidence never changes');
END;
