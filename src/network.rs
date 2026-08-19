use arch_sdk::Config;
use bitcoin::Network;
use clap::ValueEnum;

use crate::error::{CliError, Result};

pub(crate) const DEFAULT_RPC_URL: &str = "https://rpc.testnet.arch.network";

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

pub(crate) fn config(rpc_url: String, bitcoin_network: BitcoinNetwork) -> Result<Config> {
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

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use clap::CommandFactory;

    use crate::cli::Cli;

    use super::*;

    #[test]
    fn declares_environment_fallbacks_and_network_mapping() {
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
    fn rejects_an_empty_rpc_url() {
        assert!(config("  ".to_string(), BitcoinNetwork::Testnet).is_err());
    }
}
