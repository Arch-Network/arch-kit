mod arch_signer;
mod cli;
mod commands;
mod error;
mod idl;
mod keys;
mod network;
mod token;
mod transaction;
mod utils;
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
        Command::BuildIdl(args) => commands::build_idl::run(args),
        Command::Keygen(args) => commands::keygen::run(args),
        Command::Pubkey(args) => commands::pubkey::run(args),
        Command::Ata(args) => commands::ata::run(args),
        Command::TokenBalance(args) => {
            commands::token_balance::run(&network::config(rpc_url, bitcoin_network)?, args)
        }
        Command::TokenAccount(args) => {
            commands::token_account::run(&network::config(rpc_url, bitcoin_network)?, args)
        }
        Command::TokenAccounts(args) => {
            commands::token_accounts::run(&network::config(rpc_url, bitcoin_network)?, args)
        }
        Command::MintInfo(args) => {
            commands::mint_info::run(&network::config(rpc_url, bitcoin_network)?, args)
        }
        Command::CreateMint(args) => {
            commands::create_mint::run(&network::config(rpc_url, bitcoin_network)?, args)
        }
        Command::MintTokens(args) => {
            commands::mint_tokens::run(&network::config(rpc_url, bitcoin_network)?, args)
        }
        Command::TokenTransfer(args) => {
            commands::token_transfer::run_to_user(&network::config(rpc_url, bitcoin_network)?, args)
        }
        Command::TokenTransferToAccount(args) => commands::token_transfer::run_to_account(
            &network::config(rpc_url, bitcoin_network)?,
            args,
        ),
        Command::TransferArch(args) => {
            commands::transfer_arch::run(&network::config(rpc_url, bitcoin_network)?, args)
        }
        Command::ArchBalance(args) => {
            commands::arch_balance::run(&network::config(rpc_url, bitcoin_network)?, args)
        }
        Command::Faucet(args) => {
            commands::faucet::run(&network::config(rpc_url, bitcoin_network)?, args)
        }
        Command::Health => commands::health::run(&network::config(rpc_url, bitcoin_network)?),
        Command::Deploy(args) => {
            commands::deploy::run(&network::config(rpc_url, bitcoin_network)?, args)
        }
    }
}
