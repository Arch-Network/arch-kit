mod cli;
mod commands;
mod error;
mod idl;
mod keys;
mod network;
mod vanity;

use clap::Parser;

use crate::{
    cli::{Cli, Command},
    error::Result,
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
        Command::Init(args) => commands::init::run(args),
        Command::Keygen(args) => commands::keygen::run(args),
        Command::Pubkey(args) => commands::pubkey::run(args),
        Command::Health => commands::health::run(&network::config(rpc_url, bitcoin_network)?),
        Command::Deploy(args) => {
            commands::deploy::run(&network::config(rpc_url, bitcoin_network)?, args)
        }
    }
}
