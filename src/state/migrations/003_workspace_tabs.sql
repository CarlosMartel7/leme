-- The tabs Leme opened in a task's workspace, one per layout role. A tab
-- counts only while its workspace is still the task's workspace and Herdr
-- still lists it. `ready` is set once the tab's command has been started,
-- so an interrupted setup resumes the tab instead of opening another.

CREATE TABLE workspace_tabs (
    project_id TEXT NOT NULL,
    task_id TEXT NOT NULL,
    role TEXT NOT NULL CHECK (role IN ('agent', 'editor', 'tests')),
    workspace TEXT NOT NULL,
    tab TEXT NOT NULL,
    pane TEXT NOT NULL,
    ready INTEGER NOT NULL DEFAULT 0 CHECK (ready IN (0, 1)),
    PRIMARY KEY (project_id, task_id, role),
    FOREIGN KEY (project_id, task_id) REFERENCES tasks (project_id, id)
) STRICT;
