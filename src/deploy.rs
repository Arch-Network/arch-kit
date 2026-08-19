use std::path::{Path, PathBuf};

use arch_sdk::{
    Config,
    blocking::{ArchRpcClient, ProgramDeployer},
};

use crate::{
    cli::DeployArgs,
    error::{CliError, Result},
    idl,
    keys::{load_or_generate_key, pubkey_hex},
};

pub(crate) fn run(config: &Config, args: DeployArgs) -> Result<()> {
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
