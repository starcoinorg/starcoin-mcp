#![forbid(unsafe_code)]

mod bridge;
mod client;
mod framing;
mod notify;

use std::{
    env,
    io::{self, BufReader},
    path::PathBuf,
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
    let home = env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    env::var_os("STARMASKD_SOCKET_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
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
        })
}
