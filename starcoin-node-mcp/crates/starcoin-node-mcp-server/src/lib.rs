#![forbid(unsafe_code)]

mod error_mapping;
mod runtime;
pub mod server;

pub use error_mapping::AdapterError;
pub use runtime::{serve_stdio, serve_stdio_with_config};
pub use server::StarcoinNodeMcpServer;
