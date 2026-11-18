-- owner: a2m-ledger
-- The evidence for promoting a binding in shadow says what it covers
-- (decision 0251). A comparison now names the call's input, by the
-- workspace's keyed digest of it, never the input itself and never shown;
-- and what the shadow read cost the system, in the rate budget's units, one
-- per step. A promotion names the verified binding its matches were
-- compared against, and, for each environment its evidence had to come
-- from, the distinct inputs and the matches it was promoted on.
--
-- A comparison kept before this migration names no input. It is still
-- counted as a match, a mismatch, or not compared, but no distinct input:
-- it can never make a binding promotable.
ALTER TABLE shadow_comparisons ADD COLUMN input_digest TEXT;
ALTER TABLE shadow_comparisons ADD COLUMN requests INTEGER NOT NULL DEFAULT 0
    CHECK (requests >= 0);

-- What the count of distinct inputs reads: by binding, environment, and the
-- verified binding compared against.
CREATE INDEX shadow_comparisons_by_evidence
    ON shadow_comparisons (workspace_id, target, operation, binding_digest, environment,
                           against_digest, seq);

-- From here on every comparison names its input.
CREATE TRIGGER shadow_comparisons_name_their_input
BEFORE INSERT ON shadow_comparisons
WHEN NEW.input_digest IS NULL
BEGIN
    SELECT RAISE(ABORT, 'a2m: a shadow comparison names the input it was made on');
END;

-- `against_digest`: the verified binding the counted matches were compared
-- against. `evidence`: canonical JSON, a list of {environment,
-- distinct_inputs, matches}, one per environment the evidence had to come
-- from. Both are empty for a promotion made before this migration.
ALTER TABLE shadow_promotions ADD COLUMN against_digest TEXT;
ALTER TABLE shadow_promotions ADD COLUMN evidence TEXT;
