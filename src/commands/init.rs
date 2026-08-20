use std::{
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use include_dir::{Dir, include_dir};
use minijinja::{AutoEscape, Environment, UndefinedBehavior, Value, context};

use crate::{
    error::{CliError, Result},
    keys::{load_existing_key, pubkey_hex},
};

const SATELLITE_VERSION: &str = "0.31.5";
static SIMPLE_PROGRAM_TEMPLATE: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/templates/simple_program");

#[derive(Debug, clap::Args)]
pub(crate) struct Args {
    /// Destination for the new program project. The path must not exist.
    #[arg(value_name = "PATH")]
    pub(crate) path: PathBuf,

    /// Existing program identity key used to declare the program ID.
    #[arg(long, value_name = "PATH")]
    pub(crate) program_key: PathBuf,
}

pub(crate) fn run(args: Args) -> Result<()> {
    let (_, program_id) = load_existing_key(&args.program_key, "program key")?;
    let names = ProjectNames::from_path(&args.path)?;
    let program_id_hex = pubkey_hex(&program_id);
    let program_id_base58 = program_id.to_string();

    ensure_destination_is_available(&args.path)?;
    create_directory(&args.path)?;
    render_project(&args.path, &names, &program_id_hex, &program_id_base58)?;

    println!("Satellite program initialized.");
    println!("  Path: {}", args.path.display());
    println!("  Package: {}", names.package);
    println!("  Program ID: {program_id}");

    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
struct ProjectNames {
    package: String,
    crate_name: String,
    module: String,
}

impl ProjectNames {
    fn from_path(path: &Path) -> Result<Self> {
        let raw_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                CliError::InvalidArgument(format!(
                    "program path must end with a valid UTF-8 directory name: {}",
                    path.display()
                ))
            })?;
        let package = normalized_package_name(raw_name);
        let mut crate_name = package.replace('-', "_");
        if crate_name
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_digit)
        {
            crate_name.insert_str(0, "arch_");
        }
        if is_rust_keyword(&crate_name) {
            crate_name.insert_str(0, "arch_");
        }

        Ok(Self {
            package,
            module: crate_name.clone(),
            crate_name,
        })
    }
}

fn normalized_package_name(name: &str) -> String {
    let mut normalized = String::new();
    let mut needs_separator = false;

    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            if needs_separator && !normalized.is_empty() {
                normalized.push('-');
            }
            normalized.push(character.to_ascii_lowercase());
            needs_separator = false;
        } else {
            needs_separator = true;
        }
    }

    if normalized.is_empty() {
        "arch-program".to_string()
    } else {
        normalized
    }
}

fn is_rust_keyword(name: &str) -> bool {
    matches!(
        name,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "try"
    )
}

fn ensure_destination_is_available(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => Err(CliError::InvalidArgument(format!(
            "program destination already exists: {}",
            path.display()
        ))),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(initialization_error(path, source)),
    }
}

fn create_directory(path: &Path) -> Result<()> {
    fs::create_dir_all(path).map_err(|source| initialization_error(path, source))
}

fn write_new_file(path: &Path, contents: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|source| initialization_error(path, source))?;
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|source| initialization_error(path, source))
}

fn initialization_error(path: &Path, source: std::io::Error) -> CliError {
    CliError::InitializeProgram {
        path: PathBuf::from(path),
        source,
    }
}

fn render_project(
    destination: &Path,
    names: &ProjectNames,
    program_id_hex: &str,
    program_id_base58: &str,
) -> Result<()> {
    let environment = template_environment();
    let template_context = context! {
        package_name => names.package.as_str(),
        crate_name => names.crate_name.as_str(),
        module_name => names.module.as_str(),
        program_id_hex => program_id_hex,
        program_id_base58 => program_id_base58,
        satellite_version => SATELLITE_VERSION,
    };

    render_directory(
        &SIMPLE_PROGRAM_TEMPLATE,
        destination,
        &environment,
        &template_context,
    )
}

fn template_environment() -> Environment<'static> {
    let mut environment = Environment::empty();
    environment.set_undefined_behavior(UndefinedBehavior::Strict);
    environment.set_auto_escape_callback(|_| AutoEscape::None);
    environment.set_keep_trailing_newline(true);
    environment
}

fn render_directory(
    template_directory: &Dir<'_>,
    destination: &Path,
    environment: &Environment<'_>,
    context: &Value,
) -> Result<()> {
    for directory in template_directory.dirs() {
        create_directory(&destination.join(directory.path()))?;
        render_directory(directory, destination, environment, context)?;
    }

    for file in template_directory.files() {
        let template_path = file.path();
        let is_template = template_path.extension() == Some(OsStr::new("j2"));
        let output_relative_path = if is_template {
            template_path.with_extension("")
        } else {
            template_path.to_path_buf()
        };
        let output_path = destination.join(output_relative_path);

        if is_template {
            let source = file
                .contents_utf8()
                .ok_or_else(|| CliError::InvalidProgramTemplate {
                    path: template_path.to_path_buf(),
                })?;
            let name = template_path
                .to_str()
                .ok_or_else(|| CliError::InvalidProgramTemplate {
                    path: template_path.to_path_buf(),
                })?;
            let rendered = environment
                .render_named_str(name, source, context)
                .map_err(|source| CliError::RenderProgramTemplate {
                    path: template_path.to_path_buf(),
                    source,
                })?;
            write_new_file(&output_path, rendered.as_bytes())?;
        } else {
            write_new_file(&output_path, file.contents())?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Command};

    use super::*;

    fn write_test_key(path: &Path) {
        fs::write(path, "01".repeat(32)).unwrap();
    }

    #[test]
    fn parses_a_destination_and_program_key() {
        let cli = Cli::try_parse_from([
            "arch-kit",
            "init",
            "hello-world",
            "--program-key",
            "program.key",
        ])
        .unwrap();

        let Command::Init(args) = cli.command else {
            panic!("expected init command");
        };
        assert_eq!(args.path, PathBuf::from("hello-world"));
        assert_eq!(args.program_key, PathBuf::from("program.key"));
    }

    #[test]
    fn requires_a_program_key() {
        assert!(Cli::try_parse_from(["arch-kit", "init", "hello-world"]).is_err());
    }

    #[test]
    fn derives_safe_cargo_names_from_the_destination() {
        assert_eq!(
            ProjectNames::from_path(Path::new("My Hello Program")).unwrap(),
            ProjectNames {
                package: "my-hello-program".to_string(),
                crate_name: "my_hello_program".to_string(),
                module: "my_hello_program".to_string(),
            }
        );
        assert_eq!(
            ProjectNames::from_path(Path::new("123"))
                .unwrap()
                .crate_name,
            "arch_123"
        );
        assert_eq!(
            ProjectNames::from_path(Path::new("mod"))
                .unwrap()
                .crate_name,
            "arch_mod"
        );
    }

    #[test]
    fn initializes_a_satellite_program_with_the_supplied_program_id() {
        let directory = tempfile::tempdir().unwrap();
        let key_path = directory.path().join("program.key");
        let project_path = directory.path().join("hello-world");
        write_test_key(&key_path);
        let (_, expected_program_id) = load_existing_key(&key_path, "test key").unwrap();

        run(Args {
            path: project_path.clone(),
            program_key: key_path,
        })
        .unwrap();

        let manifest = fs::read_to_string(project_path.join("Cargo.toml")).unwrap();
        let source = fs::read_to_string(project_path.join("src/lib.rs")).unwrap();
        let errors = fs::read_to_string(project_path.join("src/error.rs")).unwrap();
        let readme = fs::read_to_string(project_path.join("README.md")).unwrap();

        assert!(manifest.contains("name = \"hello-world\""));
        assert!(manifest.contains("[workspace]"));
        assert!(manifest.contains("arch-satellite-lang = \"=0.31.5\""));
        assert!(manifest.contains("unicode-segmentation = \"=1.12.0\""));
        assert!(source.contains(
            "declare_id!(\"1b84c5567b126440995d3ed5aaba0565d71e1834604819ff9c17f5e9d5dd078f\")"
        ));
        assert!(source.contains("msg!(\"Hello {}\", ctx.accounts.user.key())"));
        assert!(source.contains("HelloWorldError::UserMustSign"));
        assert!(errors.contains("pub enum HelloWorldError"));
        assert!(readme.contains(&format!("Program ID: `{expected_program_id}`")));
        assert_eq!(
            fs::read(project_path.join(".gitignore")).unwrap(),
            b"/target\n"
        );
        assert!(!project_path.join("Cargo.toml.j2").exists());
    }

    #[test]
    fn template_rendering_is_strict_and_preserves_trailing_newlines() {
        let environment = template_environment();

        assert!(
            environment
                .render_str("{{ missing }}", context! {})
                .is_err()
        );
        assert_eq!(
            environment.render_str("content\n", context! {}).unwrap(),
            "content\n"
        );
    }

    #[test]
    fn refuses_to_write_into_an_existing_destination() {
        let directory = tempfile::tempdir().unwrap();
        let key_path = directory.path().join("program.key");
        let project_path = directory.path().join("existing");
        write_test_key(&key_path);
        fs::create_dir(&project_path).unwrap();
        fs::write(project_path.join("keep.txt"), "untouched").unwrap();

        let result = run(Args {
            path: project_path.clone(),
            program_key: key_path,
        });

        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(project_path.join("keep.txt")).unwrap(),
            "untouched"
        );
        assert!(!project_path.join("Cargo.toml").exists());
    }
}
