CREATE TABLE IF NOT EXISTS requests (
  request_id TEXT PRIMARY KEY,
  client_request_id TEXT NOT NULL,
  kind TEXT NOT NULL,
  status TEXT NOT NULL,
  wallet_instance_id TEXT NOT NULL,
  account_address TEXT NOT NULL,
  payload_hash TEXT NOT NULL,
  payload_json TEXT NOT NULL,
  result_json TEXT,
  created_at INTEGER NOT NULL,
  expires_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  approved_at INTEGER,
  rejected_at INTEGER,
  cancelled_at INTEGER,
  failed_at INTEGER,
  result_expires_at INTEGER,
  last_error_code TEXT,
  last_error_message TEXT,
  reject_reason_code TEXT,
  delivery_lease_id TEXT,
  delivery_lease_expires_at INTEGER,
  presentation_id TEXT,
  presentation_expires_at INTEGER
);

CREATE TABLE IF NOT EXISTS wallet_instances (
  wallet_instance_id TEXT PRIMARY KEY,
  extension_id TEXT NOT NULL,
  extension_version TEXT NOT NULL,
  protocol_version INTEGER NOT NULL,
  profile_hint TEXT,
  lock_state TEXT NOT NULL,
  connected INTEGER NOT NULL,
  last_seen_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS wallet_accounts (
  wallet_instance_id TEXT NOT NULL,
  address TEXT NOT NULL,
  label TEXT,
  public_key TEXT,
  is_default INTEGER NOT NULL,
  is_locked INTEGER NOT NULL,
  last_seen_at INTEGER NOT NULL,
  PRIMARY KEY (wallet_instance_id, address),
  FOREIGN KEY (wallet_instance_id) REFERENCES wallet_instances(wallet_instance_id) ON DELETE CASCADE
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_requests_client_request_id
  ON requests(client_request_id);

CREATE INDEX IF NOT EXISTS idx_requests_status_expires_at
  ON requests(status, expires_at);

CREATE INDEX IF NOT EXISTS idx_requests_wallet_instance_status
  ON requests(wallet_instance_id, status);

CREATE INDEX IF NOT EXISTS idx_requests_result_expires_at
  ON requests(result_expires_at);

CREATE INDEX IF NOT EXISTS idx_wallet_instances_connected_last_seen_at
  ON wallet_instances(connected, last_seen_at);

CREATE INDEX IF NOT EXISTS idx_wallet_accounts_address
  ON wallet_accounts(address);
