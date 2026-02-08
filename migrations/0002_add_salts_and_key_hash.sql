-- Salts table: 128-bit random salts with creation time.
CREATE TABLE IF NOT EXISTS salts (
  id BIGSERIAL PRIMARY KEY,
  salt BYTEA NOT NULL,
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_salts_created_at ON salts (created_at DESC);

-- Entries: store only a hash identifier (no plaintext key).
DO $$
BEGIN
  IF EXISTS (
    SELECT 1
    FROM information_schema.columns
    WHERE table_name = 'entries'
      AND column_name = 'key'
  ) THEN
    ALTER TABLE entries RENAME COLUMN key TO key_hash;
  END IF;
END $$;

-- Rebuild indexes to match new column name (idempotent).
DROP INDEX IF EXISTS idx_entries_key_created_at;
CREATE INDEX IF NOT EXISTS idx_entries_key_hash_created_at ON entries (key_hash, created_at DESC);
