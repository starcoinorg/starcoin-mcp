use std::{env, path::PathBuf, time::Duration};

use starcoin_node_core::AppContext;
use starcoin_node_types::{GetBlockInput, Mode, RuntimeConfig, VmProfile};
use url::Url;

#[tokio::test]
#[ignore = "requires STARCOIN_NODE_E2E_RPC_URL"]
async fn read_only_smoke_against_live_rpc_endpoint() {
    let rpc_url = env::var("STARCOIN_NODE_E2E_RPC_URL")
        .expect("STARCOIN_NODE_E2E_RPC_URL must be set for live smoke tests");
    let app = AppContext::bootstrap(RuntimeConfig {
        rpc_endpoint_url: Url::parse(&rpc_url).expect("rpc url should parse"),
        mode: Mode::ReadOnly,
        vm_profile: VmProfile::Auto,
        expected_chain_id: None,
        expected_network: None,
        expected_genesis_hash: None,
        require_genesis_hash_match: false,
        connect_timeout: Duration::from_secs(3),
        request_timeout: Duration::from_secs(10),
        startup_probe_timeout: Duration::from_secs(10),
        rpc_auth_token: None,
        rpc_headers: Vec::new(),
        tls_server_name: None,
        allowed_rpc_hosts: Vec::new(),
        tls_pinned_spki_sha256: Vec::new(),
        allow_insecure_remote_transport: true,
        allow_read_only_chain_autodetect: true,
        default_expiration_ttl: Duration::from_secs(600),
        max_expiration_ttl: Duration::from_secs(3_600),
        watch_poll_interval: Duration::from_secs(3),
        watch_timeout: Duration::from_secs(120),
        max_head_lag: Duration::from_secs(60),
        warn_head_lag: Duration::from_secs(15),
        allow_submit_without_prior_simulation: true,
        chain_status_cache_ttl: Duration::from_secs(3),
        abi_cache_ttl: Duration::from_secs(300),
        module_cache_max_entries: 128,
        disable_disk_cache: true,
        max_submit_blocking_timeout: Duration::from_secs(60),
        max_watch_timeout: Duration::from_secs(300),
        min_watch_poll_interval: Duration::from_secs(2),
        max_list_blocks_count: 100,
        max_events_limit: 200,
        max_account_resource_limit: 100,
        max_account_module_limit: 50,
        max_list_resources_size: 100,
        max_list_modules_size: 100,
        max_publish_package_bytes: 524_288,
        max_concurrent_watch_requests: 8,
        max_inflight_expensive_requests: 16,
        config_path: Some(PathBuf::from("/tmp/node-mcp-live.toml")),
        log_level: "info".to_owned(),
    })
    .await
    .expect("live read_only bootstrap should succeed");

    let chain_status = app
        .chain_status()
        .await
        .expect("chain_status should succeed against the live endpoint");
    assert!(chain_status.chain_id > 0);
    assert!(!chain_status.network.is_empty());

    let block = app
        .get_block(GetBlockInput {
            block_hash: None,
            block_number: Some(chain_status.head_block_number),
            decode: true,
            include_raw: false,
        })
        .await
        .expect("head block lookup should succeed against the live endpoint");
    assert!(block.block.is_some());
}
