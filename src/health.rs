use std::{
    thread,
    time::{Duration, Instant},
};

use arch_sdk::{Config, blocking::ArchRpcClient};

use crate::error::{CliError, Result};

const IS_NODE_READY: &str = "is_node_ready";
const BLOCK_PROGRESS_WINDOW: Duration = Duration::from_secs(2);

pub(crate) fn run(config: &Config) -> Result<()> {
    let started_at = Instant::now();
    let client = ArchRpcClient::new(config);
    let latency_started_at = Instant::now();
    let readiness = client
        .call_method::<bool>(IS_NODE_READY)
        .map_err(|source| health_rpc_error(config, source))?;
    let rpc_latency = latency_started_at.elapsed();
    require_ready(readiness, &config.arch_node_url)?;

    let initial_height = client
        .get_block_count()
        .map_err(|source| health_rpc_error(config, source))?;
    thread::sleep(BLOCK_PROGRESS_WINDOW);

    let final_height = client
        .get_block_count()
        .map_err(|source| health_rpc_error(config, source))?;
    require_progress(
        initial_height,
        final_height,
        BLOCK_PROGRESS_WINDOW.as_secs(),
        &config.arch_node_url,
    )?;

    println!("Arch node is healthy (ready and producing blocks).");
    println!("  RPC: {}", config.arch_node_url);
    println!(
        "  Block height: {initial_height} -> {final_height} (+{})",
        final_height - initial_height
    );
    println!("  RPC latency: {}ms", rpc_latency.as_millis());
    println!("  Progress window: {}s", BLOCK_PROGRESS_WINDOW.as_secs());
    println!("  Total check time: {}ms", started_at.elapsed().as_millis());
    Ok(())
}

fn health_rpc_error(config: &Config, source: arch_sdk::ArchError) -> CliError {
    CliError::NodeHealthRpc {
        rpc_url: config.arch_node_url.clone(),
        source,
    }
}

fn require_ready(readiness: Option<bool>, rpc_url: &str) -> Result<()> {
    match readiness {
        Some(true) => Ok(()),
        Some(false) => Err(CliError::NodeNotReady {
            rpc_url: rpc_url.to_string(),
        }),
        None => Err(CliError::NodeHealthUnavailable {
            rpc_url: rpc_url.to_string(),
        }),
    }
}

fn require_progress(
    initial_height: u64,
    final_height: u64,
    observation_seconds: u64,
    rpc_url: &str,
) -> Result<()> {
    if final_height > initial_height {
        Ok(())
    } else {
        Err(CliError::BlocksNotProgressing {
            rpc_url: rpc_url.to_string(),
            initial_height,
            final_height,
            observation_seconds,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_the_validator_readiness_rpc_method() {
        assert_eq!(IS_NODE_READY, "is_node_ready");
        assert_eq!(BLOCK_PROGRESS_WINDOW, Duration::from_secs(2));
    }

    #[test]
    fn accepts_only_an_explicit_ready_response() {
        assert!(require_ready(Some(true), "http://node").is_ok());
        assert!(matches!(
            require_ready(Some(false), "http://node"),
            Err(CliError::NodeNotReady { .. })
        ));
        assert!(matches!(
            require_ready(None, "http://node"),
            Err(CliError::NodeHealthUnavailable { .. })
        ));
    }

    #[test]
    fn requires_the_block_height_to_increase() {
        assert!(require_progress(100, 101, 2, "http://node").is_ok());

        for final_height in [100, 99] {
            assert!(matches!(
                require_progress(100, final_height, 2, "http://node"),
                Err(CliError::BlocksNotProgressing {
                    initial_height: 100,
                    final_height: observed,
                    observation_seconds: 2,
                    ..
                }) if observed == final_height
            ));
        }
    }
}
