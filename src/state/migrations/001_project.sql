-- Paths are stored as bytes because Git paths need not be valid UTF-8.

CREATE TABLE projects (
    id TEXT PRIMARY KEY NOT NULL,
    repository BLOB NOT NULL UNIQUE
) STRICT;

CREATE TABLE tasks (
    project_id TEXT NOT NULL REFERENCES projects (id),
    id TEXT NOT NULL,
    branch TEXT NOT NULL,
    worktree BLOB NOT NULL UNIQUE,
    PRIMARY KEY (project_id, id),
    UNIQUE (project_id, branch)
) STRICT;
