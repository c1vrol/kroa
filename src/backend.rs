//! Native linking: compile LLVM IR with clang and link the Kroa runtime.

use crate::diagnostics::{Diagnostic, Diagnostics};
use crate::CompileOptions;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn link_native(
    llvm_ir: &str,
    output: &Path,
    options: &CompileOptions,
    diagnostics: &mut Diagnostics,
) -> Result<(), Vec<Diagnostic>> {
    let tmp_dir = std::env::temp_dir().join(format!("kroa-{}", std::process::id()));
    let _ = fs::create_dir_all(&tmp_dir);
    let ll_path = tmp_dir.join("out.ll");
    if let Err(err) = fs::write(&ll_path, llvm_ir) {
        diagnostics.error(format!("failed to write LLVM IR: {err}"));
        return Err(diagnostics.clone_vec());
    }

    let runtime = find_runtime_c().ok_or_else(|| {
        diagnostics
            .error("could not find runtime/runtime.c; run the compiler from the Kroa project root");
        diagnostics.clone_vec()
    })?;

    let mut cmd = Command::new("clang");
    cmd.arg(&ll_path)
        .arg(&runtime)
        .arg("-O2")
        .arg("-o")
        .arg(output);

    for path in &options.library_paths {
        cmd.arg(format!("-L{}", path.display()));
    }
    for lib in &options.libraries {
        cmd.arg(format!("-l{lib}"));
    }
    for file in &options.link_files {
        cmd.arg(file);
    }

    // On Windows MSVC target, clang uses the MSVC linker when available.
    let output_result = cmd.output();
    match output_result {
        Ok(out) => {
            if !out.status.success() {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let stdout = String::from_utf8_lossy(&out.stdout);
                diagnostics.error(format!(
                    "clang failed while linking native binary\n{stdout}{stderr}"
                ));
                if !options.keep_temps {
                    let _ = fs::remove_dir_all(&tmp_dir);
                }
                return Err(diagnostics.clone_vec());
            }
        }
        Err(err) => {
            diagnostics.error(format!(
                "failed to run clang: {err}. Install LLVM and ensure `clang` is on PATH"
            ));
            return Err(diagnostics.clone_vec());
        }
    }

    if !options.keep_temps {
        let _ = fs::remove_dir_all(&tmp_dir);
    }
    Ok(())
}

fn find_runtime_c() -> Option<PathBuf> {
    let candidates = [
        PathBuf::from("runtime/runtime.c"),
        PathBuf::from("./runtime/runtime.c"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime/runtime.c"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

trait DiagnosticsExt {
    fn clone_vec(&self) -> Vec<Diagnostic>;
}

impl DiagnosticsExt for Diagnostics {
    fn clone_vec(&self) -> Vec<Diagnostic> {
        self.iter().cloned().collect()
    }
}
