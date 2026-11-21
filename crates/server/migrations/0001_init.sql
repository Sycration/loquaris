-- Timestamps are milliseconds since the Unix epoch. Secrets (session tokens,
-- invite and reset tokens) are stored as their SHA-256 only.

CREATE TABLE users (
    id            TEXT PRIMARY KEY,
    email         TEXT NOT NULL UNIQUE,      -- trimmed and lowercased
    display_name  TEXT NOT NULL,
    password_hash TEXT,                      -- argon2id PHC string; NULL until set
    is_admin      INTEGER NOT NULL DEFAULT 0,
    disabled      INTEGER NOT NULL DEFAULT 0,
    settings      TEXT NOT NULL DEFAULT '{}', -- JSON object
    created_at    INTEGER NOT NULL
);

CREATE TABLE sessions (
    id           TEXT PRIMARY KEY,           -- public ID for listing and revoking
    token_hash   BLOB NOT NULL UNIQUE,
    user_id      TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at   INTEGER NOT NULL,
    last_seen_at INTEGER NOT NULL,
    expires_at   INTEGER NOT NULL,
    user_agent   TEXT
);
CREATE INDEX sessions_user ON sessions(user_id);

-- One-time links to set a password (admin-generated resets and new accounts).
CREATE TABLE password_resets (
    token_hash BLOB PRIMARY KEY,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    used_at    INTEGER
);

CREATE TABLE projects (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,                -- mirrors meta.name for listings
    owner_id   TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    deleted_at INTEGER
);

CREATE TABLE members (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id    TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role       TEXT NOT NULL CHECK (role IN ('owner', 'editor', 'viewer')),
    added_at   INTEGER NOT NULL,
    PRIMARY KEY (project_id, user_id)
);
CREATE INDEX members_user ON members(user_id);

CREATE TABLE invites (
    id         TEXT PRIMARY KEY,
    token_hash BLOB NOT NULL UNIQUE,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    role       TEXT NOT NULL CHECK (role IN ('editor', 'viewer')),
    created_by TEXT NOT NULL REFERENCES users(id),
    created_at INTEGER NOT NULL,
    expires_at INTEGER,
    max_uses   INTEGER,
    uses       INTEGER NOT NULL DEFAULT 0,
    revoked_at INTEGER
);
CREATE INDEX invites_project ON invites(project_id);

-- The Yjs update log; a snapshot covers every update with seq <= its seq.
CREATE TABLE doc_updates (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL,
    data       BLOB NOT NULL,
    user_id    TEXT,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (project_id, seq)
);

CREATE TABLE doc_snapshots (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    seq        INTEGER NOT NULL,
    data       BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (project_id, seq)
);

CREATE TABLE versions (
    id         TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    label      TEXT,
    auto       INTEGER NOT NULL DEFAULT 0,
    data       BLOB NOT NULL,                -- encoded document state
    created_by TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX versions_project ON versions(project_id, created_at);

CREATE TABLE blobs (
    sha256      TEXT PRIMARY KEY,
    size        INTEGER NOT NULL,
    mime        TEXT NOT NULL,
    sample_rate INTEGER NOT NULL,
    frames      INTEGER NOT NULL,
    channels    INTEGER NOT NULL,
    created_by  TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at  INTEGER NOT NULL
);

-- Which projects may read a blob.
CREATE TABLE project_blobs (
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    sha256     TEXT NOT NULL REFERENCES blobs(sha256),
    PRIMARY KEY (project_id, sha256)
);
CREATE INDEX project_blobs_sha ON project_blobs(sha256);
