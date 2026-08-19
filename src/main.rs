mod cli;
mod deploy;
mod error;
mod health;
mod idl;
mod keys;
mod vanity;

use arch_sdk::Config;
use clap::Parser;

use crate::{
    cli::{Cli, Command},
    error::{CliError, Result},
};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let Cli {
        rpc_url,
        bitcoin_network,
        command,
    } = Cli::parse();

    match command {
        Command::Keygen(args) => keys::run_keygen(args),
        Command::Pubkey(args) => keys::run_pubkey(args),
        Command::Health => health::run(&network_config(rpc_url, bitcoin_network)?),
        Command::Deploy(args) => deploy::run(&network_config(rpc_url, bitcoin_network)?, args),
    }
}

fn network_config(rpc_url: String, bitcoin_network: cli::BitcoinNetwork) -> Result<Config> {
    if rpc_url.trim().is_empty() {
        return Err(CliError::InvalidArgument(
            "--rpc-url/ARCH_RPC_URL must not be empty".to_string(),
        ));
    }

    Ok(Config {
        arch_node_url: rpc_url,
        network: bitcoin_network.into(),
        node_endpoint: String::new(),
        node_username: String::new(),
        node_password: String::new(),
        titan_url: String::new(),
    })
}
