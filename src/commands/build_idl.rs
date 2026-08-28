use std::{fs, path::PathBuf};

use arch_satellite_lang_idl::build::IdlBuilder;

use crate::error::{CliError, Result};

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Satellite program directory containing Cargo.toml.
    #[arg(value_name = "PROGRAM_PATH")]
    pub(crate) program_path: PathBuf,

    /// Destination for the generated IDL JSON.
    #[arg(value_name = "OUTPUT")]
    pub(crate) output: PathBuf,
}

pub(crate) fn run(args: Args) -> Result<()> {
    if !args.program_path.join("Cargo.toml").is_file() {
        return Err(CliError::Idl(format!(
            "program directory does not contain Cargo.toml: {}",
            args.program_path.display()
        )));
    }

    let idl = IdlBuilder::new()
        .program_path(args.program_path.clone())
        .build()
        .map_err(|error| CliError::Idl(format!("IDL build failed: {error}")))?;
    let json = serde_json::to_vec_pretty(&idl)?;

    if let Some(parent) = args.output.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent).map_err(|error| {
            CliError::Idl(format!(
                "cannot create IDL output directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    fs::write(&args.output, json).map_err(|error| {
        CliError::Idl(format!(
            "cannot write IDL output {}: {error}",
            args.output.display()
        ))
    })?;

    println!("Satellite IDL built successfully.");
    println!("  Program: {}", args.program_path.display());
    println!("  Output: {}", args.output.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;
    use crate::cli::{Cli, Command};

    #[test]
    fn parses_program_and_output_paths() {
        let cli = Cli::try_parse_from([
            "arch-kit",
            "build-idl",
            "program",
            "target/idl/program.json",
        ])
        .unwrap();
        let Command::BuildIdl(args) = cli.command else {
            panic!("expected build-idl command");
        };
        assert_eq!(args.program_path, PathBuf::from("program"));
        assert_eq!(args.output, PathBuf::from("target/idl/program.json"));
    }
}
