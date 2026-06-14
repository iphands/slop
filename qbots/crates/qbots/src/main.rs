//! # qbots — external Quake 2 bot client fleet
//!
//! CLI entry point. `connect-one` is the Plan 03 verification harness (a single bot
//! connects and stays alive); `run`/`status` for a multi-bot fleet land in Plan 07.

use std::net::SocketAddr;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "qbots",
    about = "External Quake 2 bot clients that connect to a real server over UDP"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Connect a single bot to a server and keep it alive.
    ConnectOne {
        /// Server address, e.g. `127.0.0.1:27910`.
        #[arg(long)]
        addr: String,
        /// Bot display name (userinfo `name`).
        #[arg(long)]
        name: Option<String>,
        /// Client qport (defaults to a per-process value; must be unique across bots).
        #[arg(long)]
        qport: Option<u16>,
    },
}

/// A per-process default qport (distinct across concurrent bot processes).
fn default_qport() -> u16 {
    (std::process::id() & 0xFFFF) as u16
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::ConnectOne { addr, name, qport } => {
            let name = name.unwrap_or_else(|| "qbots".to_string());
            let qport = qport.unwrap_or_else(default_qport);
            let addr: SocketAddr = match addr.parse() {
                Ok(a) => a,
                Err(e) => {
                    eprintln!("qbots: bad address '{addr}': {e}");
                    return ExitCode::FAILURE;
                }
            };
            println!("qbots: connecting '{name}' to {addr} (qport {qport})…  Ctrl-C to stop.");
            match client::run(addr, &name, qport).await {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("qbots: {e}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}
