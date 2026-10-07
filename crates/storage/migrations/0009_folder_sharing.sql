-- Application-owned file work is journalled before audio is made visible.
-- UUID identities deliberately do not participate in OpenWhispr's empty-table rule.
CREATE TABLE folder_share_operations (
    id TEXT PRIMARY KEY,
    direction TEXT NOT NULL CHECK(direction IN ('export', 'import')),
    stage TEXT NOT NULL,
    staging_path TEXT NOT NULL,
    recording_keys TEXT NOT NULL DEFAULT '[]',
    result_folder_id INTEGER REFERENCES folders(id) ON DELETE SET NULL,
    result_path TEXT,
    result_digest TEXT,
    error TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
