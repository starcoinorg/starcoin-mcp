#![forbid(unsafe_code)]

mod bridge;
mod client;
mod framing;
mod notify;

use std::{
    env,
    io::{self, BufReader},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::EnvFilter;

use crate::{
    bridge::handle_request,
    client::LocalDaemonClient,
    framing::{read_frame, write_frame},
    notify::{NotificationState, spawn_notification_loop},
};

#[derive(Debug, Parser)]
#[command(name = "starmask-native-host")]
#[command(about = "Chrome Native Messaging bridge for the Starmask wallet runtime")]
struct Cli {
    #[arg(long)]
    socket_path: Option<PathBuf>,
    #[arg(long)]
    log_level: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let log_level = cli
        .log_level
        .or_else(|| env::var("STARMASK_NATIVE_HOST_LOG_LEVEL").ok())
        .unwrap_or_else(|| "warn".to_owned());

    tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_env_filter(EnvFilter::new(log_level))
        .with_target(false)
        .init();

    let client = LocalDaemonClient::new(cli.socket_path.unwrap_or_else(default_socket_path));
    let stdin = io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let writer = Arc::new(Mutex::new(io::stdout()));
    let state = Arc::new(Mutex::new(NotificationState::default()));
    let running = Arc::new(AtomicBool::new(true));
    let notifier = spawn_notification_loop(
        client.clone(),
        state.clone(),
        writer.clone(),
        running.clone(),
    );

    while let Some(frame) = read_frame(&mut reader)? {
        let request: starmask_types::NativeBridgeRequest = serde_json::from_slice(&frame)?;
        let response = handle_request(&client, request.clone());
        state
            .lock()
            .expect("notification state poisoned")
            .observe(&request, &response);
        let payload = serde_json::to_vec(&response)?;
        let mut stdout = writer.lock().expect("stdout writer poisoned");
        write_frame(&mut *stdout, &payload)?;
    }

    running.store(false, Ordering::Relaxed);
    let _ = notifier.join();

    Ok(())
}

fn default_socket_path() -> PathBuf {
    daemon_socket_env_override().unwrap_or_else(|| resolve_default_socket_path(&default_home_dir()))
}

fn daemon_socket_env_override() -> Option<PathBuf> {
    ["STARMASKD_SOCKET_PATH", "STARMASK_MCP_DAEMON_SOCKET_PATH"]
        .into_iter()
        .find_map(|name| env::var_os(name).map(PathBuf::from))
}

fn default_home_dir() -> PathBuf {
    env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn resolve_default_socket_path(home: &Path) -> PathBuf {
    let preferred = preferred_socket_path(home);
    if preferred.exists() {
        return preferred;
    }

    let legacy = legacy_socket_path(home);
    if legacy.exists() {
        return legacy;
    }

    preferred
}

fn preferred_socket_path(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library")
            .join("Application Support")
            .join("StarmaskRuntime")
            .join("run")
            .join("starmaskd.sock")
    } else {
        home.join(".local")
            .join("state")
            .join("starmask-runtime")
            .join("starmaskd.sock")
    }
}

fn legacy_socket_path(home: &Path) -> PathBuf {
    if cfg!(target_os = "macos") {
        home.join("Library")
            .join("Application Support")
            .join("StarcoinMCP")
            .join("run")
            .join("starmaskd.sock")
    } else {
        home.join(".local")
            .join("state")
            .join("starcoin-mcp")
            .join("starmaskd.sock")
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::{legacy_socket_path, preferred_socket_path, resolve_default_socket_path};

    #[test]
    fn prefers_existing_new_socket_path() {
        let tempdir = tempdir().expect("tempdir");
        let preferred = preferred_socket_path(tempdir.path());
        let legacy = legacy_socket_path(tempdir.path());
        fs::create_dir_all(preferred.parent().expect("preferred parent")).expect("create new path");
        fs::create_dir_all(legacy.parent().expect("legacy parent")).expect("create legacy path");
        fs::write(&preferred, []).expect("write new socket placeholder");
        fs::write(&legacy, []).expect("write legacy socket placeholder");

        assert_eq!(resolve_default_socket_path(tempdir.path()), preferred);
    }

    #[test]
    fn falls_back_to_existing_legacy_socket_path() {
        let tempdir = tempdir().expect("tempdir");
        let legacy = legacy_socket_path(tempdir.path());
        fs::create_dir_all(legacy.parent().expect("legacy parent")).expect("create legacy path");
        fs::write(&legacy, []).expect("write legacy socket placeholder");

        assert_eq!(resolve_default_socket_path(tempdir.path()), legacy);
    }
}
