-- owner: a2m-auth
-- Whether a session's last sign-in was declared by a published version or by
-- the target's drafts (decision 0374). A session made under drafts is never
-- renewed without its person, under the drafts or under a published version,
-- and the mark is kept with the session so that a restart does not forget
-- it. `published` or `drafts`. Null in a row kept before this migration,
-- whose sign-in nobody can tell: the keeper reads null, and any word it does
-- not know, as `drafts`, so such a session waits for its person.
ALTER TABLE sessions ADD COLUMN signed_in_under TEXT
    CHECK (signed_in_under IS NULL OR signed_in_under IN ('published', 'drafts'));
