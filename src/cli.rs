use std::path::PathBuf;

use bitcoin::Network;
use clap::{Args, Parser, Subcommand, ValueEnum};

pub(crate) const DEFAULT_RPC_URL: &str = "https://rpc.testnet.arch.network";

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
    Init(InitArgs),

    /// Deploy or update a program and optionally publish its IDL.
    Deploy(DeployArgs),

    /// Generate one or more new secp256k1 secret key files.
    Keygen(KeygenArgs),

    /// Derive an Arch public key from a secret key file.
    Pubkey(PubkeyArgs),

    /// Check whether the configured Arch node is ready and its chain is progressing.
    Health,
}

#[derive(Debug, Args)]
pub(crate) struct InitArgs {
    /// Destination for the new program project. The path must not exist.
    #[arg(value_name = "PATH")]
    pub(crate) path: PathBuf,

    /// Existing program identity key used to declare the program ID.
    #[arg(long, value_name = "PATH")]
    pub(crate) program_key: PathBuf,
}

#[derive(Debug, Args)]
pub(crate) struct PubkeyArgs {
    /// Secret key file to read.
    #[arg(value_name = "PATH")]
    pub(crate) secret_key: PathBuf,
}

#[derive(Debug, Args)]
pub(crate) struct KeygenArgs {
    /// Require each generated Arch public key to start with this Base58 prefix.
    #[arg(long, value_name = "BASE58_PREFIX")]
    pub(crate) prefix: Option<String>,

    /// Maximum parallel search threads for vanity key generation.
    #[arg(long, requires = "prefix", value_name = "COUNT")]
    pub(crate) threads: Option<usize>,

    /// Destinations for new secret keys. The files must not already exist.
    #[arg(value_name = "PATH", required = true, num_args = 1..)]
    pub(crate) outputs: Vec<PathBuf>,
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

    /// Securely generate any missing program or authority key file.
    #[arg(long)]
    pub(crate) generate_if_missing: bool,

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

        let Command::Deploy(args) = cli.command else {
            panic!("expected deploy command");
        };
        assert!(args.fund_authority);
        assert!(!args.generate_if_missing);
        assert_eq!(args.idl_size, Some(20_000));
        assert_eq!(args.idl, Some(PathBuf::from("program.idl.json")));
    }

    #[test]
    fn parses_init_with_a_destination_and_program_key() {
        let cli = Cli::try_parse_from([
            "arch-kit",
            "init",
            "hello-world",
            "--program-key",
            "program.key",
        ])
        .unwrap();

        let Command::Init(args) = cli.command else {
            panic!("expected init command");
        };
        assert_eq!(args.path, PathBuf::from("hello-world"));
        assert_eq!(args.program_key, PathBuf::from("program.key"));
    }

    #[test]
    fn init_requires_a_program_key() {
        assert!(Cli::try_parse_from(["arch-kit", "init", "hello-world"]).is_err());
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
        assert_eq!(rpc_url.get_default_values(), [OsStr::new(DEFAULT_RPC_URL)]);
        assert_eq!(
            bitcoin_network.get_env(),
            Some(OsStr::new("ARCH_BITCOIN_NETWORK"))
        );
        assert_eq!(
            bitcoin_network.get_default_values(),
            [OsStr::new("testnet")]
        );
        assert_eq!(Network::from(BitcoinNetwork::Mainnet), Network::Bitcoin);
        assert_eq!(Network::from(BitcoinNetwork::Testnet4), Network::Testnet4);
    }

    #[test]
    fn keygen_accepts_one_or_more_positional_paths_without_connection_options() {
        let cli = Cli::try_parse_from(["arch-kit", "keygen", "first.key", "second.key"]).unwrap();

        let Command::Keygen(args) = cli.command else {
            panic!("expected keygen command");
        };
        assert_eq!(
            args.outputs,
            [PathBuf::from("first.key"), PathBuf::from("second.key")]
        );
        assert!(args.prefix.is_none());
        assert!(args.threads.is_none());
    }

    #[test]
    fn keygen_requires_at_least_one_path() {
        assert!(Cli::try_parse_from(["arch-kit", "keygen"]).is_err());
    }

    #[test]
    fn keygen_accepts_a_vanity_prefix_and_thread_limit() {
        let cli = Cli::try_parse_from([
            "arch-kit",
            "keygen",
            "--prefix",
            "PAMM",
            "--threads",
            "4",
            "program.key",
        ])
        .unwrap();

        let Command::Keygen(args) = cli.command else {
            panic!("expected keygen command");
        };
        assert_eq!(args.prefix.as_deref(), Some("PAMM"));
        assert_eq!(args.threads, Some(4));
    }

    #[test]
    fn keygen_rejects_threads_without_a_prefix() {
        assert!(
            Cli::try_parse_from(["arch-kit", "keygen", "--threads", "2", "program.key"]).is_err()
        );
    }

    #[test]
    fn pubkey_accepts_a_secret_key_path() {
        let cli = Cli::try_parse_from(["arch-kit", "pubkey", "authority.key"]).unwrap();

        let Command::Pubkey(args) = cli.command else {
            panic!("expected pubkey command");
        };
        assert_eq!(args.secret_key, PathBuf::from("authority.key"));
    }

    #[test]
    fn pubkey_requires_exactly_one_path() {
        assert!(Cli::try_parse_from(["arch-kit", "pubkey"]).is_err());
        assert!(Cli::try_parse_from(["arch-kit", "pubkey", "first.key", "second.key"]).is_err());
    }

    #[test]
    fn parses_health_with_shared_network_configuration() {
        let cli = Cli::try_parse_from([
            "arch-kit",
            "--rpc-url",
            "http://127.0.0.1:9002",
            "--bitcoin-network",
            "regtest",
            "health",
        ])
        .unwrap();

        assert!(matches!(cli.command, Command::Health));
        assert_eq!(cli.rpc_url, "http://127.0.0.1:9002");
        assert_eq!(cli.bitcoin_network, BitcoinNetwork::Regtest);
    }

    #[test]
    fn health_does_not_expose_the_progress_window() {
        assert!(Cli::try_parse_from(["arch-kit", "health", "--progress-window", "5"]).is_err());
    }

    #[test]
    fn deploy_accepts_generate_if_missing() {
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
            "program.key",
            "--authority",
            "authority.key",
            "--generate-if-missing",
        ])
        .unwrap();

        let Command::Deploy(args) = cli.command else {
            panic!("expected deploy command");
        };
        assert!(args.generate_if_missing);
    }
}
