use std::path::PathBuf;

use bitcoin::Network;
use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "arch-kit")]
#[command(about = "Program interaction toolkit for Arch Network")]
#[command(version)]
pub(crate) struct Cli {
    /// Arch JSON-RPC endpoint.
    #[arg(long, env = "ARCH_RPC_URL", value_name = "URL")]
    pub(crate) rpc_url: String,

    /// Bitcoin network used for BIP-322 transaction signatures.
    #[arg(long, env = "ARCH_BITCOIN_NETWORK", value_enum, value_name = "NETWORK")]
    pub(crate) bitcoin_network: BitcoinNetwork,

    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Deploy or update a program and optionally publish its IDL.
    Deploy(DeployArgs),
}

#[derive(Debug, Args)]
pub(crate) struct DeployArgs {
    /// Compiled Arch program ELF.
    #[arg(long, value_name = "PATH")]
    pub(crate) elf: PathBuf,

    /// Existing program identity keypair.
    #[arg(long, value_name = "PATH")]
    pub(crate) program_key: PathBuf,

    /// Existing deployment and IDL authority keypair.
    #[arg(long, value_name = "PATH")]
    pub(crate) authority: PathBuf,

    /// Fund the authority through the configured Arch RPC faucet before deployment.
    #[arg(long)]
    pub(crate) fund_authority: bool,

    /// IDL JSON to initialize or upgrade after deployment.
    #[arg(long, value_name = "PATH")]
    pub(crate) idl: Option<PathBuf>,

    /// Minimum initial canonical IDL account size in bytes, including its 44-byte header.
    #[arg(long, requires = "idl", value_name = "BYTES")]
    pub(crate) idl_size: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum BitcoinNetwork {
    Mainnet,
    Testnet,
    Testnet4,
    Signet,
    Regtest,
}

impl From<BitcoinNetwork> for Network {
    fn from(value: BitcoinNetwork) -> Self {
        match value {
            BitcoinNetwork::Mainnet => Self::Bitcoin,
            BitcoinNetwork::Testnet => Self::Testnet,
            BitcoinNetwork::Testnet4 => Self::Testnet4,
            BitcoinNetwork::Signet => Self::Signet,
            BitcoinNetwork::Regtest => Self::Regtest,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use clap::CommandFactory;

    use super::*;

    #[test]
    fn parses_deploy_with_optional_operational_flags() {
        let cli = Cli::try_parse_from([
            "arch-kit",
            "--rpc-url",
            "http://127.0.0.1:9002",
            "--bitcoin-network",
            "regtest",
            "deploy",
            "--elf",
            "program.so",
            "--program-key",
            "program.json",
            "--authority",
            "authority.json",
            "--fund-authority",
            "--idl",
            "program.idl.json",
            "--idl-size",
            "20000",
        ])
        .unwrap();

        let Command::Deploy(args) = cli.command;
        assert!(args.fund_authority);
        assert_eq!(args.idl_size, Some(20_000));
        assert_eq!(args.idl, Some(PathBuf::from("program.idl.json")));
    }

    #[test]
    fn rejects_idl_size_without_idl() {
        let parsed = Cli::try_parse_from([
            "arch-kit",
            "--rpc-url",
            "http://127.0.0.1:9002",
            "--bitcoin-network",
            "regtest",
            "deploy",
            "--elf",
            "program.so",
            "--program-key",
            "program.json",
            "--authority",
            "authority.json",
            "--idl-size",
            "10000",
        ]);

        assert!(parsed.is_err());
    }

    #[test]
    fn declares_connection_environment_fallbacks_and_network_mapping() {
        let command = Cli::command();
        let rpc_url = command
            .get_arguments()
            .find(|argument| argument.get_id() == "rpc_url")
            .unwrap();
        let bitcoin_network = command
            .get_arguments()
            .find(|argument| argument.get_id() == "bitcoin_network")
            .unwrap();

        assert_eq!(rpc_url.get_env(), Some(OsStr::new("ARCH_RPC_URL")));
        assert_eq!(
            bitcoin_network.get_env(),
            Some(OsStr::new("ARCH_BITCOIN_NETWORK"))
        );
        assert_eq!(Network::from(BitcoinNetwork::Mainnet), Network::Bitcoin);
        assert_eq!(Network::from(BitcoinNetwork::Testnet4), Network::Testnet4);
    }
}
