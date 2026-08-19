mod cli;
mod deploy;
mod error;
mod idl;
mod keys;

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
    let cli = Cli::parse();
    if cli.rpc_url.trim().is_empty() {
        return Err(CliError::InvalidArgument(
            "--rpc-url/ARCH_RPC_URL must not be empty".to_string(),
        ));
    }

    let config = Config {
        arch_node_url: cli.rpc_url,
        network: cli.bitcoin_network.into(),
        node_endpoint: String::new(),
        node_username: String::new(),
        node_password: String::new(),
        titan_url: String::new(),
    };

    match cli.command {
        Command::Deploy(args) => deploy::run(&config, args),
    }
}
