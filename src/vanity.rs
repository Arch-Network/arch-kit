use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, RecvTimeoutError},
    },
    thread,
    time::{Duration, Instant},
};

use arch_sdk::arch_program::pubkey::Pubkey;
use bitcoin::{
    key::Keypair,
    secp256k1::{Secp256k1, SecretKey},
};

use crate::error::{CliError, Result};

const BASE58_ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
const MAX_X_ONLY_BASE58_LEN: usize = 44;
const ATTEMPT_BATCH_SIZE: u64 = 4_096;

pub(crate) struct VanitySearch {
    prefix: String,
    thread_count: usize,
}

pub(crate) struct VanityOutcome {
    pub(crate) keypair: Keypair,
    pub(crate) pubkey: Pubkey,
    pub(crate) attempts: u64,
    pub(crate) elapsed: Duration,
}

struct Candidate {
    keypair: Keypair,
    pubkey: Pubkey,
}

impl VanitySearch {
    pub(crate) fn new(prefix: &str, requested_threads: Option<usize>) -> Result<Self> {
        validate_prefix(prefix)?;
        if requested_threads == Some(0) {
            return Err(CliError::InvalidArgument(
                "--threads must be at least 1".to_string(),
            ));
        }

        let available_threads = thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1);
        let thread_count = requested_threads
            .unwrap_or(available_threads)
            .min(available_threads);

        Ok(Self {
            prefix: prefix.to_string(),
            thread_count,
        })
    }

    pub(crate) fn prefix(&self) -> &str {
        &self.prefix
    }

    pub(crate) fn thread_count(&self) -> usize {
        self.thread_count
    }

    pub(crate) fn rough_expected_attempts(&self) -> f64 {
        58_f64.powi(self.prefix.len() as i32)
    }

    pub(crate) fn run(&self) -> Result<VanityOutcome> {
        let started_at = Instant::now();
        let found = Arc::new(AtomicBool::new(false));
        let attempts = Arc::new(AtomicU64::new(0));
        let prefix: Arc<[u8]> = Arc::from(self.prefix.as_bytes());
        let (sender, receiver) = mpsc::sync_channel(1);
        let mut workers = Vec::with_capacity(self.thread_count);

        for _ in 0..self.thread_count {
            let found = Arc::clone(&found);
            let attempts = Arc::clone(&attempts);
            let prefix = Arc::clone(&prefix);
            let sender = sender.clone();
            workers.push(thread::spawn(move || {
                search_worker(&prefix, &found, &attempts, &sender);
            }));
        }
        drop(sender);

        let mut candidate = None;
        let mut previous_attempts = 0;
        let mut previous_update = Instant::now();
        loop {
            match receiver.recv_timeout(Duration::from_secs(1)) {
                Ok(result) => {
                    candidate = Some(result);
                    break;
                }
                Err(RecvTimeoutError::Timeout) => {
                    let current_attempts = attempts.load(Ordering::Relaxed);
                    let elapsed = previous_update.elapsed().as_secs_f64();
                    let rate =
                        (current_attempts.saturating_sub(previous_attempts)) as f64 / elapsed;
                    eprintln!("  Searched {current_attempts} candidates ({rate:.0}/s)...");
                    previous_attempts = current_attempts;
                    previous_update = Instant::now();
                }
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }

        found.store(true, Ordering::Release);
        let mut worker_panicked = false;
        for worker in workers {
            worker_panicked |= worker.join().is_err();
        }
        if worker_panicked {
            return Err(CliError::VanitySearch(
                "a search worker panicked".to_string(),
            ));
        }

        let candidate = candidate.ok_or_else(|| {
            CliError::VanitySearch("all workers stopped without finding a key".to_string())
        })?;
        Ok(VanityOutcome {
            keypair: candidate.keypair,
            pubkey: candidate.pubkey,
            attempts: attempts.load(Ordering::Relaxed),
            elapsed: started_at.elapsed(),
        })
    }
}

fn search_worker(
    prefix: &[u8],
    found: &AtomicBool,
    attempts: &AtomicU64,
    sender: &mpsc::SyncSender<Candidate>,
) {
    let secp = Secp256k1::new();
    let mut rng = rand::thread_rng();
    let mut pending_attempts = 0;

    while !found.load(Ordering::Acquire) {
        let secret_key = SecretKey::new(&mut rng);
        let public_key = secret_key.public_key(&secp);
        let serialized = public_key.serialize();
        let x_only_bytes = &serialized[1..];
        pending_attempts += 1;

        if base58_starts_with(x_only_bytes, prefix)
            && found
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            attempts.fetch_add(pending_attempts, Ordering::Relaxed);
            let keypair = Keypair::from_secret_key(&secp, &secret_key);
            let pubkey = Pubkey::from_slice(x_only_bytes);
            let _ = sender.send(Candidate { keypair, pubkey });
            return;
        }

        if pending_attempts == ATTEMPT_BATCH_SIZE {
            attempts.fetch_add(pending_attempts, Ordering::Relaxed);
            pending_attempts = 0;
        }
    }

    attempts.fetch_add(pending_attempts, Ordering::Relaxed);
}

fn base58_starts_with(bytes: &[u8], prefix: &[u8]) -> bool {
    let mut encoded = [0_u8; MAX_X_ONLY_BASE58_LEN];
    let encoded_len = bs58::encode(bytes)
        .onto(encoded.as_mut_slice())
        .expect("44 bytes can hold the Base58 encoding of a 32-byte public key");
    encoded[..encoded_len].starts_with(prefix)
}

fn validate_prefix(prefix: &str) -> Result<()> {
    if prefix.is_empty() {
        return Err(CliError::InvalidArgument(
            "--prefix must not be empty".to_string(),
        ));
    }
    if prefix.len() > MAX_X_ONLY_BASE58_LEN {
        return Err(CliError::InvalidArgument(format!(
            "--prefix cannot exceed {MAX_X_ONLY_BASE58_LEN} Base58 characters"
        )));
    }
    if let Some(invalid) = prefix
        .bytes()
        .find(|character| !BASE58_ALPHABET.contains(character))
    {
        return Err(CliError::InvalidArgument(format!(
            "--prefix contains non-Base58 character {:?}",
            char::from(invalid)
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_bitcoin_base58_prefixes() {
        assert!(validate_prefix("PAMM").is_ok());
        assert!(validate_prefix("").is_err());
        assert!(validate_prefix("0OIl").is_err());
        assert!(validate_prefix(&"a".repeat(45)).is_err());
    }

    #[test]
    fn x_only_encoding_matches_the_arch_pubkey_representation() {
        let secp = Secp256k1::new();
        let secret_key = SecretKey::from_slice(&[1_u8; 32]).unwrap();
        let public_key = secret_key.public_key(&secp);
        let serialized = public_key.serialize();
        let x_only_bytes = &serialized[1..];
        let expected = Pubkey::from_slice(x_only_bytes).to_string();

        assert!(base58_starts_with(x_only_bytes, expected.as_bytes()));
        assert!(!base58_starts_with(x_only_bytes, b"not-the-prefix"));
    }

    #[test]
    fn thread_count_is_positive_and_capped_to_available_parallelism() {
        let search = VanitySearch::new("A", Some(usize::MAX)).unwrap();
        let available = thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(1);

        assert_eq!(search.thread_count(), available);
        assert!(VanitySearch::new("A", Some(0)).is_err());
    }
}
