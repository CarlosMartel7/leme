-- The Herdr workspace last opened for a task. Herdr IDs are opaque handles
-- that stay valid while the workspace is open; a stale one is replaced.

ALTER TABLE tasks ADD COLUMN workspace TEXT;
