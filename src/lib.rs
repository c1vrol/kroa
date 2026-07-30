//! Kroa compiler library.
//!
//! Pipeline (Phase 1): source → lexer → parser → typecheck → LLVM IR → native binary
//! Pipeline (Phase 2+): source → … → typecheck → Kroa IR → analyses → LLVM IR → native

pub mod ast;
pub mod backend;
pub mod borrowcheck;
pub mod codegen;
pub mod diagnostics;
pub mod ffi;
pub mod ir;
pub mod lexer;
pub mod lower;
pub mod memory;
pub mod parser;
pub mod span;
pub mod token;
pub mod typecheck;

use std::path::{Path, PathBuf};
use std::process::Command;

pub use diagnostics::{print_diagnostics, DiagnosticCode, MessageFormat};
use diagnostics::{Diagnostic, Diagnostics};
use span::SourceFile;

/// Compilation options shared by CLI commands.
#[derive(Debug, Clone, Default)]
pub struct CompileOptions {
    pub emit_ir: bool,
    pub emit_kir: bool,
    pub output: Option<PathBuf>,
    pub libraries: Vec<String>,
    pub library_paths: Vec<PathBuf>,
    /// Extra files passed directly to the linker (objects, .c sources, etc.).
    pub link_files: Vec<PathBuf>,
    pub keep_temps: bool,
    /// `human` (default) or `json` diagnostics for agent auto-fix loops.
    pub message_format: MessageFormat,
}

/// High-level result of compiling a Kroa source file.
#[derive(Debug)]
pub struct CompileResult {
    pub llvm_ir: String,
    pub kroa_ir: Option<String>,
    pub output_path: Option<PathBuf>,
}

/// Compile Kroa source text into LLVM IR and optionally a native executable.
pub fn compile_source(
    filename: &str,
    source: &str,
    options: &CompileOptions,
) -> Result<CompileResult, Vec<Diagnostic>> {
    let file = SourceFile::new(filename.to_string(), source.to_string());
    let mut diagnostics = Diagnostics::new();

    let tokens = match lexer::lex(&file, &mut diagnostics) {
        Some(tokens) => tokens,
        None => {
            diagnostics.attach_locations(&file);
            return Err(diagnostics.into_vec());
        }
    };

    let program = match parser::parse(&file, &tokens, &mut diagnostics) {
        Some(program) => program,
        None => {
            diagnostics.attach_locations(&file);
            return Err(diagnostics.into_vec());
        }
    };

    let typed = match typecheck::typecheck(&file, &program, &mut diagnostics) {
        Some(typed) => typed,
        None => {
            diagnostics.attach_locations(&file);
            return Err(diagnostics.into_vec());
        }
    };

    // Phase 2+: lower to Kroa IR whenever the program uses features that need it,
    // or always once structs/arenas/references exist. Phase 1 programs still work
    // through the IR path for a single backend.
    let module_ir = lower::lower(&typed, &mut diagnostics);
    if diagnostics.has_errors() {
        diagnostics.attach_locations(&file);
        return Err(diagnostics.into_vec());
    }

    if typed.needs_borrow_check() {
        borrowcheck::borrow_check(&file, &module_ir, &mut diagnostics);
        if diagnostics.has_errors() {
            diagnostics.attach_locations(&file);
            return Err(diagnostics.into_vec());
        }
    }

    let llvm_ir = match codegen::emit_llvm(&module_ir, &mut diagnostics) {
        Some(ir) => ir,
        None => {
            diagnostics.attach_locations(&file);
            return Err(diagnostics.into_vec());
        }
    };

    let kroa_ir = if options.emit_kir {
        Some(ir::format_module(&module_ir))
    } else {
        None
    };

    if options.emit_ir || options.emit_kir {
        return Ok(CompileResult {
            llvm_ir,
            kroa_ir,
            output_path: None,
        });
    }

    let output_path = options
        .output
        .clone()
        .unwrap_or_else(|| default_output_path(filename));

    backend::link_native(&llvm_ir, &output_path, options, &mut diagnostics)?;
    if diagnostics.has_errors() {
        diagnostics.attach_locations(&file);
        return Err(diagnostics.into_vec());
    }

    Ok(CompileResult {
        llvm_ir,
        kroa_ir,
        output_path: Some(output_path),
    })
}

/// Compile a file from disk.
pub fn compile_file(
    path: &Path,
    options: &CompileOptions,
) -> Result<CompileResult, Vec<Diagnostic>> {
    let source = std::fs::read_to_string(path).map_err(|err| {
        vec![Diagnostic::error_code(
            diagnostics::DiagnosticCode::E0900,
            format!("failed to read '{}': {err}", path.display()),
        )]
    })?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("input.kroa");
    compile_source(name, &source, options)
}

fn default_output_path(filename: &str) -> PathBuf {
    let path = Path::new(filename);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("a");
    #[cfg(windows)]
    {
        PathBuf::from(format!("{stem}.exe"))
    }
    #[cfg(not(windows))]
    {
        PathBuf::from(stem)
    }
}

/// Run a compiled executable and return its exit status and captured stdout.
pub fn run_executable(path: &Path, args: &[String]) -> std::io::Result<(i32, String, String)> {
    let output = Command::new(path).args(args).output()?;
    let code = output.status.code().unwrap_or(1);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    Ok((code, stdout, stderr))
}
