use std::path::{Path, PathBuf};

use arch_sdk::{
    ArchError, Config, Status,
    arch_program::{
        bpf_loader::BPF_LOADER_ID, pubkey::Pubkey, sanitized::ArchMessage, system_instruction,
        system_program,
    },
    blocking::{ArchRpcClient, ProgramDeployer},
    build_and_sign_transaction,
};
use bitcoin::key::Keypair;

use crate::{
    error::{CliError, Result},
    idl,
    keys::{load_or_generate_key, pubkey_hex},
    token::parse_pubkey,
};

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Compiled Arch program ELF.
    #[arg(long, value_name = "PATH")]
    pub(crate) elf: PathBuf,

    /// Existing program identity keypair.
    #[arg(long, value_name = "PATH")]
    pub(crate) program_key: PathBuf,

    /// Require --program-key to derive this program ID (Base58 or 64-character hex).
    #[arg(long, value_name = "PUBKEY")]
    pub(crate) expect_program_id: Option<String>,

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

    /// Minimum canonical IDL account size in bytes, including its 44-byte header.
    /// Growing a populated account also requires --allow-idl-resize.
    #[arg(long, requires = "idl", value_name = "BYTES")]
    pub(crate) idl_size: Option<usize>,

    /// Allow clearing, growing, and republishing a populated canonical IDL.
    #[arg(long, requires = "idl")]
    pub(crate) allow_idl_resize: bool,
}

pub(crate) fn run(config: &Config, args: Args) -> Result<()> {
    let expected_program = args
        .expect_program_id
        .as_deref()
        .map(|value| parse_pubkey(value, "--expect-program-id"))
        .transpose()?;

    ensure_file(&args.elf, "program ELF")?;
    // Read now so invalid permissions or I/O fail before optional faucet use.
    std::fs::read(&args.elf).map_err(|source| CliError::ReadInput {
        label: "program ELF",
        path: args.elf.clone(),
        source,
    })?;

    let (program_keypair, program_pubkey, generated_program_key) = load_or_generate_key(
        &args.program_key,
        "program key",
        config.network,
        args.generate_if_missing,
    )?;
    if let Some(expected) = expected_program
        && expected != program_pubkey
    {
        return Err(CliError::InvalidArgument(format!(
            "program ID mismatch: expected {expected} from --expect-program-id, but --program-key derives {program_pubkey}"
        )));
    }
    let (authority_keypair, authority_pubkey, generated_authority_key) = load_or_generate_key(
        &args.authority,
        "authority key",
        config.network,
        args.generate_if_missing,
    )?;

    if generated_program_key {
        println!(
            "Generated missing program key: {}",
            args.program_key.display()
        );
    }
    if generated_authority_key {
        println!(
            "Generated missing authority key: {}",
            args.authority.display()
        );
    }

    let prepared_idl = args
        .idl
        .as_deref()
        .map(|path| idl::prepare(path, program_pubkey, args.idl_size))
        .transpose()?;

    println!("Arch program deployment");
    println!("  RPC: {}", config.arch_node_url);
    println!("  Bitcoin network: {}", config.network);
    println!("  Program: {}", program_pubkey);
    println!("  Program (hex): {}", pubkey_hex(&program_pubkey));
    println!("  Authority: {}", authority_pubkey);

    if args.fund_authority {
        if config.network == bitcoin::Network::Bitcoin {
            return Err(CliError::MainnetFaucetUnsupported);
        }
        println!("Funding deployment authority through the faucet...");
        ArchRpcClient::new(config)
            .create_and_fund_program_authority_with_faucet(&authority_keypair)?;
        println!("Authority faucet funding completed.");
    }

    let elf_path = path_string(&args.elf, "program ELF")?;
    let program_name = args
        .elf
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Arch Program")
        .to_string();

    println!("Deploying ELF: {}", args.elf.display());
    prepare_program_account(config, program_keypair, authority_keypair)?;
    let deployed_program = ProgramDeployer::new(config).try_deploy_program(
        program_name,
        program_keypair,
        authority_keypair,
        &elf_path,
    )?;
    if deployed_program != program_pubkey {
        return Err(CliError::InvalidArgument(format!(
            "SDK returned program {deployed_program}, expected {program_pubkey} from --program-key"
        )));
    }

    println!("Program deployed successfully.");
    println!("  Program ID: {deployed_program}");
    println!("  Program ID (hex): {}", pubkey_hex(&deployed_program));

    if let Some(prepared) = prepared_idl
        && let Err(source) = idl::publish(
            config,
            deployed_program,
            authority_pubkey,
            authority_keypair,
            prepared,
            args.allow_idl_resize,
        )
    {
        return Err(CliError::IdlAfterDeployment {
            program_base58: deployed_program.to_string(),
            program_hex: pubkey_hex(&deployed_program),
            source: Box::new(source),
        });
    }

    Ok(())
}

// SDK 0.10.0 skips account creation whenever the program account exists, even
// when it is an empty System account created by an earlier funding transfer.
fn prepare_program_account(
    config: &Config,
    program_keypair: Keypair,
    authority_keypair: Keypair,
) -> Result<()> {
    let program = Pubkey::from_slice(&program_keypair.x_only_public_key().0.serialize());
    let authority = Pubkey::from_slice(&authority_keypair.x_only_public_key().0.serialize());
    if program == authority {
        return Err(CliError::InvalidArgument(
            "program and deployment authority must use different keys".to_string(),
        ));
    }

    let client = ArchRpcClient::new(config);
    let account = match client.read_account_info(program) {
        Ok(account) => account,
        Err(ArchError::NotFound(_)) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if account.owner == BPF_LOADER_ID {
        return Ok(());
    }
    if account.owner != system_program::SYSTEM_PROGRAM_ID
        || !account.data.is_empty()
        || account.is_executable
    {
        return Err(CliError::InvalidArgument(format!(
            "program account {program} must be loader-owned or an empty, non-executable System account; owner={}, data_len={}, executable={}",
            account.owner,
            account.data.len(),
            account.is_executable,
        )));
    }

    println!("Assigning existing program account {program} to the BPF loader...");
    let message = ArchMessage::new(
        &[system_instruction::assign(&program, &BPF_LOADER_ID)],
        Some(authority),
        client.get_best_finalized_block_hash()?,
    );
    let transaction = build_and_sign_transaction(
        message,
        vec![authority_keypair, program_keypair],
        config.network,
    )?;
    let txid = client.send_transaction(transaction)?;
    let processed = client.wait_for_processed_transaction(&txid)?;
    if processed.status != Status::Processed || !processed.rollback_status.is_applied() {
        return Err(CliError::TransactionFailed {
            action: format!("program account assignment ({txid})"),
            status: format!(
                "{:?}, rollback={:?}",
                processed.status, processed.rollback_status
            ),
        });
    }
    let assigned = client.read_account_info(program)?;
    if assigned.owner != BPF_LOADER_ID {
        return Err(CliError::InvalidArgument(format!(
            "program account {program} still has owner {} after assignment {txid}; expected {BPF_LOADER_ID}",
            assigned.owner,
        )));
    }
    println!("Program account assigned to the BPF loader: {txid}");
    Ok(())
}

fn ensure_file(path: &Path, label: &'static str) -> Result<()> {
    if path.is_file() {
        Ok(())
    } else {
        Err(CliError::InputNotFile {
            label,
            path: path.to_path_buf(),
        })
    }
}

fn path_string(path: &Path, label: &'static str) -> Result<String> {
    path.to_str()
        .map(ToOwned::to_owned)
        .ok_or_else(|| CliError::NonUtf8Path {
            label,
            path: PathBuf::from(path),
        })
}

#[cfg(test)]
#[path = "deploy_tests.rs"]
mod deployment_tests;

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Command};

    use super::*;

    #[test]
    fn parses_optional_operational_flags() {
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
            "--allow-idl-resize",
            "--expect-program-id",
            "1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f",
        ])
        .unwrap();

        let Command::Deploy(args) = cli.command else {
            panic!("expected deploy command");
        };
        assert!(args.fund_authority);
        assert!(!args.generate_if_missing);
        assert_eq!(args.idl_size, Some(20_000));
        assert!(args.allow_idl_resize);
        assert_eq!(
            args.expect_program_id.as_deref(),
            Some("1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f")
        );
        assert_eq!(args.idl, Some(PathBuf::from("program.idl.json")));
    }

    #[test]
    fn rejects_idl_size_without_idl() {
        let parsed = Cli::try_parse_from([
            "arch-kit",
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
    fn idl_resize_defaults_to_false() {
        let cli = Cli::try_parse_from([
            "arch-kit",
            "deploy",
            "--elf",
            "program.so",
            "--program-key",
            "program.json",
            "--authority",
            "authority.json",
            "--idl",
            "program.idl.json",
        ])
        .unwrap();

        let Command::Deploy(args) = cli.command else {
            panic!("expected deploy command");
        };
        assert!(!args.allow_idl_resize);
        assert!(args.expect_program_id.is_none());
    }

    #[test]
    fn rejects_idl_resize_without_idl() {
        let parsed = Cli::try_parse_from([
            "arch-kit",
            "deploy",
            "--elf",
            "program.so",
            "--program-key",
            "program.json",
            "--authority",
            "authority.json",
            "--allow-idl-resize",
        ]);

        assert!(parsed.is_err());
    }

    #[test]
    fn accepts_generate_if_missing() {
        let cli = Cli::try_parse_from([
            "arch-kit",
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
