use std::path::PathBuf;

use arch_sdk::{ArchError, Config, arch_program::pubkey::Pubkey, blocking::ArchRpcClient};
use serde_json::Value;

use crate::{
    error::{CliError, Result},
    idl::{self, PublishMode, PublishOptions},
    keys::{load_existing_key, pubkey_hex},
    token::parse_pubkey,
};

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    #[command(subcommand)]
    pub(crate) command: IdlCommand,
}

#[derive(Debug, clap::Subcommand)]
pub(crate) enum IdlCommand {
    /// Initialize or upgrade the canonical IDL of an already-deployed program.
    Publish(PublishArgs),

    /// Print the canonical on-chain IDL of a program as JSON.
    Fetch(FetchArgs),

    /// Fail unless the canonical on-chain IDL matches a local IDL JSON file.
    Check(CheckArgs),
}

#[derive(Debug, clap::Args)]
pub(crate) struct PublishArgs {
    /// Deployed program ID (Base58 or 64-character hex).
    #[arg(long, value_name = "PUBKEY")]
    pub(crate) program_id: String,

    /// IDL JSON to publish.
    #[arg(long, value_name = "PATH")]
    pub(crate) idl: PathBuf,

    /// IDL authority keypair; pays for and signs every IDL transaction.
    #[arg(long, value_name = "PATH")]
    pub(crate) authority: PathBuf,

    /// `init` requires a missing IDL, `upgrade` an existing one; `auto` accepts either.
    #[arg(long, value_enum, default_value = "auto", value_name = "MODE")]
    pub(crate) mode: PublishMode,

    /// Print the target IDL address and every transaction; send nothing.
    #[arg(long)]
    pub(crate) dry_run: bool,

    /// Minimum canonical IDL account size in bytes, including its 44-byte header.
    /// Growing a populated account also requires --allow-idl-resize.
    #[arg(long, value_name = "BYTES")]
    pub(crate) idl_size: Option<usize>,

    /// Allow clearing, growing, and republishing a populated canonical IDL.
    #[arg(long)]
    pub(crate) allow_idl_resize: bool,
}

#[derive(Debug, clap::Args)]
pub(crate) struct FetchArgs {
    /// Program ID (Base58 or 64-character hex).
    #[arg(long, value_name = "PUBKEY")]
    pub(crate) program_id: String,
}

#[derive(Debug, clap::Args)]
pub(crate) struct CheckArgs {
    /// Expected IDL JSON.
    #[arg(long, value_name = "PATH")]
    pub(crate) idl: PathBuf,

    /// Program ID (Base58 or 64-character hex).
    #[arg(long, value_name = "PUBKEY")]
    pub(crate) program_id: String,
}

pub(crate) fn run(config: &Config, args: Args) -> Result<()> {
    match args.command {
        IdlCommand::Publish(args) => publish(config, args),
        IdlCommand::Fetch(args) => fetch(config, args),
        IdlCommand::Check(args) => check(config, args),
    }
}

fn publish(config: &Config, args: PublishArgs) -> Result<()> {
    let program = parse_pubkey(&args.program_id, "--program-id")?;
    let (authority_keypair, authority) = load_existing_key(&args.authority, "authority key")?;
    let prepared = idl::prepare(&args.idl, program, args.idl_size)?;

    println!("Arch program IDL publication");
    println!("  RPC: {}", config.arch_node_url);
    println!("  Bitcoin network: {}", config.network);
    println!("  Program: {program}");
    println!("  Program (hex): {}", pubkey_hex(&program));
    ensure_deployed(config, program)?;

    idl::publish(
        config,
        program,
        authority,
        authority_keypair,
        prepared,
        PublishOptions {
            mode: args.mode,
            allow_resize: args.allow_idl_resize,
            dry_run: args.dry_run,
        },
    )
}

/// Catch a wrong --program-id before any IDL transaction.
fn ensure_deployed(config: &Config, program: Pubkey) -> Result<()> {
    match ArchRpcClient::new(config).read_account_info(program) {
        Ok(account) if account.is_executable => Ok(()),
        Ok(_) => Err(CliError::InvalidArgument(format!(
            "--program-id {program} is not an executable program account"
        ))),
        Err(ArchError::NotFound(_)) => Err(CliError::InvalidArgument(format!(
            "--program-id {program} does not exist on {}",
            config.arch_node_url
        ))),
        Err(error) => Err(error.into()),
    }
}

fn fetch(config: &Config, args: FetchArgs) -> Result<()> {
    let program = parse_pubkey(&args.program_id, "--program-id")?;
    let fetched = idl::fetch(config, program)?;
    println!("{}", serde_json::to_string_pretty(&fetched.json)?);
    Ok(())
}

fn check(config: &Config, args: CheckArgs) -> Result<()> {
    let program = parse_pubkey(&args.program_id, "--program-id")?;
    let prepared = idl::prepare(&args.idl, program, None)?;
    let fetched = idl::fetch(config, program)?;
    let differing = differing_fields(prepared.expected(), &fetched.json);
    if !differing.is_empty() {
        return Err(CliError::Idl(format!(
            "on-chain IDL at {} differs from {} in: {}",
            fetched.address,
            args.idl.display(),
            differing.join(", ")
        )));
    }
    println!("On-chain IDL matches {}", args.idl.display());
    println!("  IDL account: {}", fetched.address);
    println!("  IDL account (hex): {}", pubkey_hex(&fetched.address));
    println!("  Authority: {}", fetched.authority);
    println!("  Capacity: {} bytes", fetched.capacity);
    Ok(())
}

/// Top-level IDL fields whose values differ, for a readable check failure.
fn differing_fields(expected: &Value, actual: &Value) -> Vec<String> {
    let (Some(expected), Some(actual)) = (expected.as_object(), actual.as_object()) else {
        return if expected == actual {
            vec![]
        } else {
            vec!["<root>".to_string()]
        };
    };
    let mut keys: Vec<&String> = expected.keys().chain(actual.keys()).collect();
    keys.sort();
    keys.dedup();
    keys.into_iter()
        .filter(|key| expected.get(*key) != actual.get(*key))
        .map(ToString::to_string)
        .collect()
}

#[cfg(test)]
#[path = "idl_tests.rs"]
mod command_tests;

#[cfg(test)]
mod tests {
    use clap::Parser;
    use serde_json::json;

    use crate::cli::{Cli, Command};

    use super::*;

    fn parse(args: &[&str]) -> IdlCommand {
        let cli = Cli::try_parse_from([&["arch-kit", "idl"], args].concat()).unwrap();
        let Command::Idl(args) = cli.command else {
            panic!("expected idl command");
        };
        args.command
    }

    #[test]
    fn parses_publish_with_defaults_and_every_option() {
        let base = [
            "publish",
            "--program-id",
            "4022dec7e57f757682712b45ddf13a8f324710f4637cefc0ebacc6343caab24c",
            "--idl",
            "idl.json",
            "--authority",
            "authority.key",
        ];
        let IdlCommand::Publish(args) = parse(&base) else {
            panic!("expected publish");
        };
        assert_eq!(args.mode, PublishMode::Auto);
        assert!(!args.dry_run && !args.allow_idl_resize);
        assert_eq!(args.idl_size, None);

        let full = [
            &base[..],
            &[
                "--mode",
                "upgrade",
                "--dry-run",
                "--idl-size",
                "20000",
                "--allow-idl-resize",
            ],
        ]
        .concat();
        let IdlCommand::Publish(args) = parse(&full) else {
            panic!("expected publish");
        };
        assert_eq!(args.mode, PublishMode::Upgrade);
        assert!(args.dry_run && args.allow_idl_resize);
        assert_eq!(args.idl_size, Some(20_000));
        assert_eq!(args.idl, PathBuf::from("idl.json"));
        assert_eq!(args.authority, PathBuf::from("authority.key"));
    }

    #[test]
    fn publish_requires_program_idl_and_authority() {
        for missing in 0..3 {
            let mut args = vec![
                "arch-kit",
                "idl",
                "publish",
                "--program-id",
                "p",
                "--idl",
                "i",
                "--authority",
                "a",
            ];
            args.drain(3 + missing * 2..5 + missing * 2);
            assert!(Cli::try_parse_from(args).is_err());
        }
        assert!(
            Cli::try_parse_from([
                "arch-kit",
                "idl",
                "publish",
                "--program-id",
                "p",
                "--idl",
                "i",
                "--authority",
                "a",
                "--mode",
                "replace",
            ])
            .is_err()
        );
    }

    #[test]
    fn parses_fetch_and_check_with_network_flags() {
        let cli = Cli::try_parse_from([
            "arch-kit",
            "--rpc-url",
            "https://rpc.mainnet.arch.network",
            "--bitcoin-network",
            "mainnet",
            "idl",
            "check",
            "--idl",
            "idl.json",
            "--program-id",
            "abc",
        ])
        .unwrap();
        assert_eq!(cli.rpc_url, "https://rpc.mainnet.arch.network");
        let Command::Idl(Args {
            command: IdlCommand::Check(args),
        }) = cli.command
        else {
            panic!("expected idl check");
        };
        assert_eq!(args.idl, PathBuf::from("idl.json"));
        assert_eq!(args.program_id, "abc");

        let IdlCommand::Fetch(args) = parse(&["fetch", "--program-id", "abc"]) else {
            panic!("expected fetch");
        };
        assert_eq!(args.program_id, "abc");
    }

    #[test]
    fn reports_each_differing_top_level_field() {
        let expected = json!({"address": "a", "errors": [1], "instructions": [], "metadata": {}});
        let actual = json!({"address": "a", "errors": [2], "events": [], "metadata": {}});
        assert_eq!(
            differing_fields(&expected, &actual),
            ["errors", "events", "instructions"]
        );
        assert!(differing_fields(&expected, &expected).is_empty());
    }
}
