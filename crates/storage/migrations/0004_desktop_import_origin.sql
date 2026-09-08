-- Older jobs cannot reliably be attributed to the desktop picker.
ALTER TABLE import_jobs ADD COLUMN origin TEXT NOT NULL DEFAULT 'automation';
