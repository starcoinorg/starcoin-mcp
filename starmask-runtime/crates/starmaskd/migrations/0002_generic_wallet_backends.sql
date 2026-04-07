ALTER TABLE wallet_instances
  ADD COLUMN backend_kind TEXT NOT NULL DEFAULT 'starmask_extension';

ALTER TABLE wallet_instances
  ADD COLUMN transport_kind TEXT NOT NULL DEFAULT 'native_messaging';

ALTER TABLE wallet_instances
  ADD COLUMN approval_surface TEXT NOT NULL DEFAULT 'browser_ui';

ALTER TABLE wallet_instances
  ADD COLUMN instance_label TEXT NOT NULL DEFAULT '';

ALTER TABLE wallet_instances
  ADD COLUMN capabilities_json TEXT NOT NULL DEFAULT '[]';

ALTER TABLE wallet_instances
  ADD COLUMN backend_metadata_json TEXT NOT NULL DEFAULT '{}';

ALTER TABLE wallet_accounts
  ADD COLUMN is_read_only INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_wallet_instances_backend_kind_connected
  ON wallet_instances(backend_kind, connected);

CREATE INDEX IF NOT EXISTS idx_wallet_instances_last_seen_at
  ON wallet_instances(last_seen_at);
