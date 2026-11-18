-- owner: a2m-auth
-- Which stored credential the count of attempts is about (decision 0269).
-- `attempted_with` names the credential as it was stored when an attempt
-- with it was counted: a digest of the sealed envelopes the auth entry
-- names, never of a value, so it confirms no guess of one. A count kept
-- with another digest than the credential now stored is about a credential
-- the person has since stored again, and is not counted. Null when no
-- attempt is counted, and in a row kept before this migration, whose count
-- holds whatever credential is stored until a person signs in themself.
ALTER TABLE sessions ADD COLUMN attempted_with TEXT;
