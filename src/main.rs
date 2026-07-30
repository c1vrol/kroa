//! Kroa compiler CLI.

use clap::{Parser, Subcommand, ValueEnum};
use kroa::{compile_file, run_executable, CompileOptions, MessageFormat};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser, Debug)]
#[command(name = "kroa", version, about = "Kroa programming language compiler")]
struct Cli {
    /// Diagnostic output format for humans or AI agent loops
    #[arg(long = "message-format", value_enum, global = true, default_value_t = CliMessageFormat::Human)]
    message_format: CliMessageFormat,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq, Eq)]
enum CliMessageFormat {
    Human,
    Json,
}

impl From<CliMessageFormat> for MessageFormat {
    fn from(value: CliMessageFormat) -> Self {
        match value {
            CliMessageFormat::Human => MessageFormat::Human,
            CliMessageFormat::Json => MessageFormat::Json,
        }
    }
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Compile a Kroa source file to a native executable
    Build {
        /// Input .kroa file
        input: PathBuf,
        /// Output executable path
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Link with a library (repeatable), e.g. --library m
        #[arg(long = "library", short = 'l')]
        libraries: Vec<String>,
        /// Add a library search path (repeatable)
        #[arg(long = "library-path", short = 'L')]
        library_paths: Vec<PathBuf>,
        /// Extra object/source files to link (repeatable)
        #[arg(long = "link")]
        link_files: Vec<PathBuf>,
        /// Keep temporary LLVM IR files
        #[arg(long)]
        keep_temps: bool,
    },
    /// Compile and run a Kroa program
    Run {
        input: PathBuf,
        /// Arguments forwarded to the program
        #[arg(last = true)]
        args: Vec<String>,
        #[arg(long = "library", short = 'l')]
        libraries: Vec<String>,
        #[arg(long = "library-path", short = 'L')]
        library_paths: Vec<PathBuf>,
        #[arg(long = "link")]
        link_files: Vec<PathBuf>,
    },
    /// Emit LLVM IR to stdout
    EmitIr { input: PathBuf },
    /// Emit Kroa IR to stdout
    EmitKir { input: PathBuf },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let format: MessageFormat = cli.message_format.into();
    match cli.command {
        Commands::Build {
            input,
            output,
            libraries,
            library_paths,
            link_files,
            keep_temps,
        } => {
            let options = CompileOptions {
                output,
                libraries,
                library_paths,
                link_files,
                keep_temps,
                message_format: format,
                ..Default::default()
            };
            match compile_file(&input, &options) {
                Ok(result) => {
                    if let Some(path) = result.output_path {
                        eprintln!("ok: wrote {}", path.display());
                    }
                    ExitCode::SUCCESS
                }
                Err(diags) => {
                    kroa::diagnostics::print_diagnostics(&diags, format);
                    ExitCode::from(1)
                }
            }
        }
        Commands::Run {
            input,
            args,
            libraries,
            library_paths,
            link_files,
        } => {
            let out = std::env::temp_dir().join(format!(
                "kroa-run-{}{}",
                std::process::id(),
                exe_suffix()
            ));
            let options = CompileOptions {
                output: Some(out.clone()),
                libraries,
                library_paths,
                link_files,
                message_format: format,
                ..Default::default()
            };
            match compile_file(&input, &options) {
                Ok(_) => match run_executable(&out, &args) {
                    Ok((code, stdout, stderr)) => {
                        print!("{stdout}");
                        eprint!("{stderr}");
                        let _ = std::fs::remove_file(&out);
                        ExitCode::from(code as u8)
                    }
                    Err(err) => {
                        eprintln!("error[E0900]: failed to run executable: {err}");
                        ExitCode::from(1)
                    }
                },
                Err(diags) => {
                    kroa::diagnostics::print_diagnostics(&diags, format);
                    ExitCode::from(1)
                }
            }
        }
        Commands::EmitIr { input } => {
            let options = CompileOptions {
                emit_ir: true,
                message_format: format,
                ..Default::default()
            };
            match compile_file(&input, &options) {
                Ok(result) => {
                    print!("{}", result.llvm_ir);
                    ExitCode::SUCCESS
                }
                Err(diags) => {
                    kroa::diagnostics::print_diagnostics(&diags, format);
                    ExitCode::from(1)
                }
            }
        }
        Commands::EmitKir { input } => {
            let options = CompileOptions {
                emit_kir: true,
                message_format: format,
                ..Default::default()
            };
            match compile_file(&input, &options) {
                Ok(result) => {
                    print!("{}", result.kroa_ir.unwrap_or_default());
                    ExitCode::SUCCESS
                }
                Err(diags) => {
                    kroa::diagnostics::print_diagnostics(&diags, format);
                    ExitCode::from(1)
                }
            }
        }
    }
}

fn exe_suffix() -> &'static str {
    if cfg!(windows) {
        ".exe"
    } else {
        ""
    }
}
