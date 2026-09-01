use arch_kit::ArchKitClient;
use arch_sdk::Config;

use crate::error::Result;

pub(crate) fn run(config: &Config) -> Result<()> {
    let status = ArchKitClient::new(config.arch_node_url.clone())?.health()?;

    println!("Arch node is healthy (ready and producing blocks).");
    println!("  RPC: {}", status.rpc_url);
    println!(
        "  Block height: {} -> {} (+{})",
        status.initial_block_height,
        status.final_block_height,
        status.block_height_delta()
    );
    println!("  RPC latency: {}ms", status.rpc_latency.as_millis());
    println!(
        "  Progress window: {}s",
        status.observation_window.as_secs()
    );
    println!("  Total check time: {}ms", status.total_elapsed.as_millis());
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::{
        cli::{Cli, Command},
        network::BitcoinNetwork,
    };

    #[test]
    fn parses_with_shared_network_configuration() {
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
    fn does_not_expose_the_progress_window() {
        assert!(Cli::try_parse_from(["arch-kit", "health", "--progress-window", "5"]).is_err());
    }
}
