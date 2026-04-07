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
    daemon_socket_env_override().unwrap_or_else(|| RuntimeSocketLayout::detect().socket_path)
}

fn daemon_socket_env_override() -> Option<PathBuf> {
    non_empty_env_path("STARMASKD_SOCKET_PATH")
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct RuntimeSocketLayout {
    socket_path: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LinuxSocketDirs {
    runtime_dir: PathBuf,
}

impl RuntimeSocketLayout {
    fn detect() -> Self {
        Self::for_home(&default_home_dir())
    }

    fn for_home(home: &Path) -> Self {
        if cfg!(target_os = "macos") {
            Self {
                socket_path: macos_runtime_support_dir(home)
                    .join("run")
                    .join("starmaskd.sock"),
            }
        } else {
            Self::for_linux_dirs(&linux_socket_dirs(home))
        }
    }

    fn for_linux_dirs(dirs: &LinuxSocketDirs) -> Self {
        Self {
            socket_path: linux_socket_path(&dirs.runtime_dir),
        }
    }
}

fn linux_socket_dirs(home: &Path) -> LinuxSocketDirs {
    let state_home =
        non_empty_env_path("XDG_STATE_HOME").unwrap_or_else(|| home.join(".local").join("state"));
    LinuxSocketDirs {
        runtime_dir: non_empty_env_path("XDG_RUNTIME_DIR").unwrap_or(state_home),
    }
}

fn macos_runtime_support_dir(home: &Path) -> PathBuf {
    home.join("Library")
        .join("Application Support")
        .join("StarmaskRuntime")
}

fn linux_socket_path(runtime_dir: &Path) -> PathBuf {
    runtime_dir.join("starmask-runtime").join("starmaskd.sock")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{LinuxSocketDirs, RuntimeSocketLayout, linux_socket_path};

    #[test]
    fn linux_runtime_socket_layout_uses_xdg_runtime_dir() {
        let dirs = LinuxSocketDirs {
            runtime_dir: PathBuf::from("/tmp/runtime-dir"),
        };

        assert_eq!(
            RuntimeSocketLayout::for_linux_dirs(&dirs).socket_path,
            PathBuf::from("/tmp/runtime-dir/starmask-runtime/starmaskd.sock")
        );
    }

    #[test]
    fn linux_socket_path_uses_runtime_dir() {
        assert_eq!(
            linux_socket_path(std::path::Path::new("/tmp/runtime-dir")),
            PathBuf::from("/tmp/runtime-dir/starmask-runtime/starmaskd.sock")
        );
    }
}
