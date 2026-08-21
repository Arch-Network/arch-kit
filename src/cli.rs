use clap::{Parser, Subcommand};

use crate::{
    commands::{
        ata, deploy, init, keygen, mint_info, pubkey, token_account, token_accounts, token_balance,
    },
    network::{BitcoinNetwork, DEFAULT_RPC_URL},
};

#[derive(Debug, Parser)]
#[command(name = "arch-kit")]
#[command(about = "Program interaction toolkit for Arch Network")]
#[command(version)]
pub(crate) struct Cli {
    /// Arch JSON-RPC endpoint.
    #[arg(
        long,
        env = "ARCH_RPC_URL",
        default_value = DEFAULT_RPC_URL,
        value_name = "URL"
    )]
    pub(crate) rpc_url: String,

    /// Bitcoin network used for BIP-322 transaction signatures.
    #[arg(
        long,
        env = "ARCH_BITCOIN_NETWORK",
        default_value = "testnet",
        value_enum,
        value_name = "NETWORK"
    )]
    pub(crate) bitcoin_network: BitcoinNetwork,

    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Initialize a new Satellite program from an existing program key.
    Init(init::Args),

    /// Deploy or update a program and optionally publish its IDL.
    Deploy(deploy::Args),

    /// Generate one or more new secp256k1 secret key files.
    Keygen(keygen::Args),

    /// Derive an Arch public key from a secret key file.
    Pubkey(pubkey::Args),

    /// Derive an associated token account address for an owner and mint.
    Ata(ata::Args),

    /// Get an owner's associated token account balance for a mint.
    TokenBalance(token_balance::Args),

    /// Inspect an APL token account.
    TokenAccount(token_account::Args),

    /// List every APL token account owned by an address.
    TokenAccounts(token_accounts::Args),

    /// Inspect an APL token mint.
    MintInfo(mint_info::Args),

    /// Check whether the configured Arch node is ready and its chain is progressing.
    Health,
}
