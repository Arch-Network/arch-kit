use std::path::PathBuf;

use thiserror::Error;

pub(crate) type Result<T> = std::result::Result<T, CliError>;

#[derive(Debug, Error)]
pub(crate) enum CliError {
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    #[error("{label} is not a readable file: {path}")]
    InputNotFile { label: &'static str, path: PathBuf },

    #[error("{label} path is not valid UTF-8: {path}")]
    NonUtf8Path { label: &'static str, path: PathBuf },

    #[error("failed to read {label} {path}: {source}")]
    ReadInput {
        label: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to load {label} {path}: {source}")]
    LoadKey {
        label: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to create {label} {path}: {source}")]
    CreateKey {
        label: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to initialize program project at {path}: {source}")]
    InitializeProgram {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("program template is not valid UTF-8: {path}")]
    InvalidProgramTemplate { path: PathBuf },

    #[error("failed to render program template {path}: {source}")]
    RenderProgramTemplate {
        path: PathBuf,
        #[source]
        source: minijinja::Error,
    },

    #[error("vanity key search failed: {0}")]
    VanitySearch(String),

    #[error("account {address} is owned by {actual}, expected APL token program {expected}")]
    TokenProgramOwnerMismatch {
        address: String,
        expected: String,
        actual: String,
    },

    #[error("failed to decode token account {address}: {detail}")]
    InvalidTokenAccount { address: String, detail: String },

    #[error("failed to decode token mint {address}: {detail}")]
    InvalidTokenMint { address: String, detail: String },

    #[error("token transfer failed: {0}")]
    TokenTransfer(String),

    #[error(
        "token account {address} has mint {actual_mint} and owner {actual_owner}, expected mint {expected_mint} and owner {expected_owner}"
    )]
    TokenAccountIdentityMismatch {
        address: String,
        expected_mint: String,
        actual_mint: String,
        expected_owner: String,
        actual_owner: String,
    },

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

    #[error("IDL JSON is invalid: {0}")]
    InvalidIdl(String),

    #[error(
        "requested IDL account size {requested} bytes is smaller than the required {required} bytes"
    )]
    IdlSizeTooSmall { requested: usize, required: usize },

    #[error("IDL payload is too large for the protocol length field: {bytes} bytes")]
    IdlPayloadTooLarge { bytes: usize },

    #[error("--fund-authority cannot be used with the mainnet signing network")]
    MainnetFaucetUnsupported,

    #[error("program deployment failed: {0}")]
    ProgramDeploy(#[from] arch_sdk::ProgramDeployerError),

    #[error("Arch RPC operation failed: {0}")]
    ArchRpc(#[from] arch_sdk::ArchError),

    #[error("JSON operation failed: {0}")]
    Json(#[from] serde_json::Error),

    #[error("IDL error: {0}")]
    Idl(String),

    #[error("{action} transaction finished with status {status}")]
    TransactionFailed { action: String, status: String },

    #[error(
        "program {program_base58} ({program_hex}) was deployed, but IDL publication failed: {source}"
    )]
    IdlAfterDeployment {
        program_base58: String,
        program_hex: String,
        #[source]
        source: Box<CliError>,
    },
}
