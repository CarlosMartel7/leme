-- One focus/break timer per project. Times are milliseconds since the Unix
-- epoch on the wall clock; durations are milliseconds.

CREATE TABLE timers (
    project_id TEXT PRIMARY KEY NOT NULL REFERENCES projects (id),
    mode TEXT NOT NULL CHECK (mode IN ('focus', 'break')),
    duration_ms INTEGER NOT NULL CHECK (duration_ms > 0),
    phase TEXT NOT NULL CHECK (phase IN ('running', 'paused', 'elapsed')),
    -- Time left: at running_since while running, or while paused.
    remaining_ms INTEGER CHECK (remaining_ms BETWEEN 0 AND duration_ms),
    running_since_ms INTEGER,
    finished_at_ms INTEGER,
    CHECK (
        (phase = 'running' AND remaining_ms IS NOT NULL
            AND running_since_ms IS NOT NULL AND finished_at_ms IS NULL)
        OR (phase = 'paused' AND remaining_ms IS NOT NULL
            AND running_since_ms IS NULL AND finished_at_ms IS NULL)
        OR (phase = 'elapsed' AND remaining_ms IS NULL
            AND running_since_ms IS NULL AND finished_at_ms IS NOT NULL)
    )
) STRICT;
