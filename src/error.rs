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
