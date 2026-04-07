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
        .find_map(non_empty_env_path)
}

fn default_home_dir() -> PathBuf {
    non_empty_env_path("HOME").unwrap_or_else(|| PathBuf::from("."))
}

fn non_empty_env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name).and_then(|value| {
        if value.is_empty() {
            None
        } else {
            Some(PathBuf::from(value))
        }
    })
}

#[derive(Clone, Debug)]
struct LinuxSocketDirs {
    state_home: PathBuf,
    runtime_dir: PathBuf,
}

fn linux_socket_dirs(home: &Path) -> LinuxSocketDirs {
    let state_home =
        non_empty_env_path("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local").join("state"));
    let runtime_dir = non_empty_env_path("XDG_RUNTIME_DIR").unwrap_or_else(|| state_home.clone());
    LinuxSocketDirs {
        state_home,
        runtime_dir,
    }
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
        preferred_linux_socket_path(&linux_socket_dirs(home))
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
        legacy_linux_socket_path(&linux_socket_dirs(home))
    }
}

fn preferred_linux_socket_path(dirs: &LinuxSocketDirs) -> PathBuf {
    dirs.runtime_dir
        .join("starmask-runtime")
        .join("starmaskd.sock")
}

fn legacy_linux_socket_path(dirs: &LinuxSocketDirs) -> PathBuf {
    dirs.state_home.join("starcoin-mcp").join("starmaskd.sock")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::{
        LinuxSocketDirs, legacy_linux_socket_path, legacy_socket_path, preferred_linux_socket_path,
        preferred_socket_path, resolve_default_socket_path,
    };

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

    #[test]
    fn linux_preferred_socket_path_uses_xdg_runtime_dir() {
        let dirs = LinuxSocketDirs {
            state_home: PathBuf::from("/tmp/state-home"),
            runtime_dir: PathBuf::from("/tmp/runtime-dir"),
        };

        assert_eq!(
            preferred_linux_socket_path(&dirs),
            PathBuf::from("/tmp/runtime-dir/starmask-runtime/starmaskd.sock")
        );
    }

    #[test]
    fn linux_legacy_socket_path_uses_xdg_state_home() {
        let dirs = LinuxSocketDirs {
            state_home: PathBuf::from("/tmp/state-home"),
            runtime_dir: PathBuf::from("/tmp/runtime-dir"),
        };

        assert_eq!(
            legacy_linux_socket_path(&dirs),
            PathBuf::from("/tmp/state-home/starcoin-mcp/starmaskd.sock")
        );
    }
}
