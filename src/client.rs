use std::{
    thread,
    time::{Duration, Instant},
};

use arch_sdk::{Config, blocking::ArchRpcClient};
use bitcoin::Network;
use thiserror::Error;

const IS_NODE_READY: &str = "is_node_ready";
/// A healthy chain still has multi-second gaps between blocks (3.5 s between
/// two consecutive blocks was observed on a chain averaging nine blocks per
/// second), so progress is polled until the height moves, bounded by this
/// timeout, instead of being judged from one fixed window.
const BLOCK_PROGRESS_TIMEOUT: Duration = Duration::from_secs(20);
const BLOCK_PROGRESS_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// The smallest reusable Arch client, currently limited to node health.
#[derive(Clone, Debug)]
pub struct ArchKitClient {
    rpc_url: String,
}

impl ArchKitClient {
    /// Create a client for an Arch JSON-RPC endpoint.
    pub fn new(rpc_url: impl Into<String>) -> Result<Self, ArchKitError> {
        let rpc_url = rpc_url.into();
        if rpc_url.trim().is_empty() {
            return Err(ArchKitError::InvalidRpcUrl);
        }
        Ok(Self { rpc_url })
    }

    /// Check that the node is ready and produces a new block within the
    /// progress timeout. The returned `observation_window` is the time it
    /// took to see the height move.
    pub fn health(&self) -> Result<HealthStatus, ArchKitError> {
        let started_at = Instant::now();
        let config = self.rpc_config();
        let client = ArchRpcClient::new(&config);

        let latency_started_at = Instant::now();
        let readiness = client
            .call_method::<bool>(IS_NODE_READY)
            .map_err(|source| self.rpc_error(source))?;
        let rpc_latency = latency_started_at.elapsed();
        require_ready(readiness, &self.rpc_url)?;

        let initial_block_height = client
            .get_block_count()
            .map_err(|source| self.rpc_error(source))?;
        let observation_started_at = Instant::now();
        let progress = wait_for_progress(
            initial_block_height,
            BLOCK_PROGRESS_TIMEOUT,
            BLOCK_PROGRESS_POLL_INTERVAL,
            || {
                client
                    .get_block_count()
                    .map_err(|source| self.rpc_error(source))
            },
        )?;
        let observation_window = observation_started_at.elapsed();
        let final_block_height = match progress {
            Progress::Advanced(height) => height,
            Progress::Stalled(height) => {
                return Err(ArchKitError::BlocksNotProgressing {
                    rpc_url: self.rpc_url.clone(),
                    initial_height: initial_block_height,
                    final_height: height,
                    observation_seconds: observation_window.as_secs(),
                });
            }
        };

        Ok(HealthStatus {
            rpc_url: self.rpc_url.clone(),
            initial_block_height,
            final_block_height,
            rpc_latency,
            observation_window,
            total_elapsed: started_at.elapsed(),
        })
    }

    fn rpc_config(&self) -> Config {
        Config {
            arch_node_url: self.rpc_url.clone(),
            // Health checks do not create or sign transactions, so the
            // Bitcoin network has no effect on this operation.
            network: Network::Bitcoin,
            node_endpoint: String::new(),
            node_username: String::new(),
            node_password: String::new(),
            titan_url: String::new(),
        }
    }

    fn rpc_error(&self, source: arch_sdk::ArchError) -> ArchKitError {
        ArchKitError::NodeHealthRpc {
            rpc_url: self.rpc_url.clone(),
            source,
        }
    }
}

/// Successful node-health measurements.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HealthStatus {
    pub rpc_url: String,
    pub initial_block_height: u64,
    pub final_block_height: u64,
    pub rpc_latency: Duration,
    pub observation_window: Duration,
    pub total_elapsed: Duration,
}

impl HealthStatus {
    pub fn block_height_delta(&self) -> u64 {
        self.final_block_height - self.initial_block_height
    }
}

/// Errors returned by the reusable Arch client.
#[derive(Debug, Error)]
pub enum ArchKitError {
    #[error("Arch RPC URL must not be empty")]
    InvalidRpcUrl,

    #[error("Arch node is not ready: {rpc_url}")]
    NodeNotReady { rpc_url: String },

    #[error("Arch node returned no readiness result: {rpc_url}")]
    NodeHealthUnavailable { rpc_url: String },

    #[error("failed to check Arch node {rpc_url}: {source}")]
    NodeHealthRpc {
        rpc_url: String,
        #[source]
        source: arch_sdk::ArchError,
    },

    #[error(
        "Arch node blocks are not progressing at {rpc_url}: height changed from {initial_height} to {final_height} over {observation_seconds}s"
    )]
    BlocksNotProgressing {
        rpc_url: String,
        initial_height: u64,
        final_height: u64,
        observation_seconds: u64,
    },
}

fn require_ready(readiness: Option<bool>, rpc_url: &str) -> Result<(), ArchKitError> {
    match readiness {
        Some(true) => Ok(()),
        Some(false) => Err(ArchKitError::NodeNotReady {
            rpc_url: rpc_url.to_string(),
        }),
        None => Err(ArchKitError::NodeHealthUnavailable {
            rpc_url: rpc_url.to_string(),
        }),
    }
}

/// Outcome of watching the block height after an initial sample.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Progress {
    /// The height rose above the initial sample; carries the height seen.
    Advanced(u64),
    /// The timeout passed without the height rising; carries the last height.
    Stalled(u64),
}

/// Poll `sample` every `poll_interval` until it exceeds `initial_height` or
/// `timeout` elapses. A sampling error ends the wait immediately, since a
/// node that stops answering is not healthy either.
fn wait_for_progress<E>(
    initial_height: u64,
    timeout: Duration,
    poll_interval: Duration,
    mut sample: impl FnMut() -> Result<u64, E>,
) -> Result<Progress, E> {
    let started_at = Instant::now();
    loop {
        thread::sleep(poll_interval);
        let height = sample()?;
        if height > initial_height {
            return Ok(Progress::Advanced(height));
        }
        if started_at.elapsed() >= timeout {
            return Ok(Progress::Stalled(height));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_an_empty_rpc_url() {
        assert!(matches!(
            ArchKitClient::new("  "),
            Err(ArchKitError::InvalidRpcUrl)
        ));
    }

    #[test]
    fn accepts_only_an_explicit_ready_response() {
        assert!(require_ready(Some(true), "http://node").is_ok());
        assert!(matches!(
            require_ready(Some(false), "http://node"),
            Err(ArchKitError::NodeNotReady { .. })
        ));
        assert!(matches!(
            require_ready(None, "http://node"),
            Err(ArchKitError::NodeHealthUnavailable { .. })
        ));
    }

    #[test]
    fn progress_is_reported_as_soon_as_the_height_moves() {
        let mut samples = [100, 100, 101].into_iter();
        let progress = wait_for_progress::<()>(
            100,
            Duration::from_secs(10),
            Duration::from_millis(1),
            || Ok(samples.next().expect("sample")),
        );
        assert_eq!(progress, Ok(Progress::Advanced(101)));
    }

    #[test]
    fn a_stalled_height_is_reported_after_the_timeout() {
        let progress = wait_for_progress::<()>(
            100,
            Duration::from_millis(5),
            Duration::from_millis(1),
            || Ok(100),
        );
        assert_eq!(progress, Ok(Progress::Stalled(100)));
    }

    #[test]
    fn a_lower_height_never_counts_as_progress() {
        let progress = wait_for_progress::<()>(
            100,
            Duration::from_millis(5),
            Duration::from_millis(1),
            || Ok(99),
        );
        assert_eq!(progress, Ok(Progress::Stalled(99)));
    }

    #[test]
    fn a_sampling_error_ends_the_wait() {
        let progress = wait_for_progress(
            100,
            Duration::from_secs(10),
            Duration::from_millis(1),
            || Err("rpc down"),
        );
        assert_eq!(progress, Err("rpc down"));
    }

    #[test]
    fn reports_the_block_height_delta() {
        let status = HealthStatus {
            rpc_url: "http://node".to_string(),
            initial_block_height: 100,
            final_block_height: 103,
            rpc_latency: Duration::from_millis(5),
            observation_window: Duration::from_secs(2),
            total_elapsed: Duration::from_millis(2_005),
        };

        assert_eq!(status.block_height_delta(), 3);
    }
}
