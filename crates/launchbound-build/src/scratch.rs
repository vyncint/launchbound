//! Specialization scratch copies: the repo copy of a kernel crate is never
//! touched; candidates are rendered into a scratch copy by rewriting
//! `src/params.rs`. Dimension `tile` maps to `pub const TILE`.

use crate::BuildError;
use launchbound_space::{Config, DimRole, KernelSpec, Value};
use std::path::{Path, PathBuf};

/// Copy the kernel crate into scratch, rewriting relative path dependencies
/// to absolute so the copy resolves them from anywhere.
pub fn prepare_scratch(spec: &KernelSpec, scratch_root: &Path) -> Result<PathBuf, BuildError> {
    let kernel_dir = spec
        .dir
        .canonicalize()
        .map_err(|e| BuildError::Scratch(format!("{}: {e}", spec.dir.display())))?;
    let scratch = scratch_root.join(&spec.name);
    std::fs::create_dir_all(scratch.join("src"))
        .map_err(|e| BuildError::Scratch(format!("{}: {e}", scratch.display())))?;

    let manifest = std::fs::read_to_string(kernel_dir.join("Cargo.toml"))
        .map_err(|e| BuildError::Scratch(format!("Cargo.toml: {e}")))?;
    std::fs::write(
        scratch.join("Cargo.toml"),
        rewrite_path_deps(&manifest, &kernel_dir),
    )
    .map_err(|e| BuildError::Scratch(e.to_string()))?;
    if let Ok(lock) = std::fs::read_to_string(kernel_dir.join("Cargo.lock")) {
        let _ = std::fs::write(scratch.join("Cargo.lock"), lock);
    }

    let src = kernel_dir.join("src");
    for entry in std::fs::read_dir(&src).map_err(|e| BuildError::Scratch(e.to_string()))? {
        let entry = entry.map_err(|e| BuildError::Scratch(e.to_string()))?;
        if entry.path().is_file() {
            std::fs::copy(entry.path(), scratch.join("src").join(entry.file_name()))
                .map_err(|e| BuildError::Scratch(e.to_string()))?;
        }
    }
    Ok(scratch)
}

/// Default scratch root for a kernel: inside its own target dir
/// (gitignored). Absolute, because compile executors may run with a
/// different working directory (or inside a container).
pub fn default_scratch_root(spec: &KernelSpec) -> PathBuf {
    let dir = spec.dir.canonicalize().unwrap_or_else(|_| spec.dir.clone());
    dir.join("target").join("launchbound-scratch")
}

/// Rewrite `path = "relative"` dependency entries against `base`.
fn rewrite_path_deps(manifest: &str, base: &Path) -> String {
    let mut out = String::with_capacity(manifest.len());
    for line in manifest.lines() {
        if let Some(start) = line.find("path = \"") {
            let rest = &line[start + 8..];
            if let Some(end) = rest.find('"') {
                let rel = &rest[..end];
                if !Path::new(rel).is_absolute() {
                    let abs = base.join(rel);
                    let abs = abs.canonicalize().unwrap_or(abs);
                    out.push_str(&line[..start + 8]);
                    out.push_str(&abs.display().to_string());
                    out.push_str(&rest[end..]);
                    out.push('\n');
                    continue;
                }
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Rewrite `src/params.rs` in the scratch copy for this candidate's
/// spec-role dimensions.
pub fn write_params(spec: &KernelSpec, config: &Config, scratch: &Path) -> Result<(), BuildError> {
    let path = scratch.join("src").join("params.rs");
    let mut text = std::fs::read_to_string(&path)
        .map_err(|e| BuildError::Params(format!("{}: {e}", path.display())))?;

    for (name, value) in config.values() {
        if spec.dim(name).map(|d| d.role) != Some(DimRole::Spec) {
            continue;
        }
        let konst = name.to_uppercase();
        let Value::Int(n) = value else {
            return Err(BuildError::Params(format!(
                "dimension `{name}` is a string; string spec dims are not supported yet"
            )));
        };
        let needle = format!("pub const {konst}:");
        let Some(line_start) = text.find(&needle) else {
            return Err(BuildError::Params(format!(
                "src/params.rs has no `pub const {konst}:` for dimension `{name}` — \
                 kernel.toml and params.rs have drifted"
            )));
        };
        let line_end = text[line_start..]
            .find(';')
            .map(|i| line_start + i)
            .ok_or_else(|| BuildError::Params(format!("unterminated const {konst}")))?;
        let eq = text[line_start..line_end]
            .find('=')
            .map(|i| line_start + i)
            .ok_or_else(|| BuildError::Params(format!("const {konst} has no `=`")))?;
        text.replace_range(eq..line_end, &format!("= {n}"));
    }
    std::fs::write(&path, text).map_err(|e| BuildError::Params(e.to_string()))
}

/// Content hash of the scratch source (Cargo.toml + src/*), for cache keys.
pub fn source_hash(scratch: &Path) -> Result<String, BuildError> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"launchbound.source.v1\0");
    let mut files = vec![scratch.join("Cargo.toml")];
    let src = scratch.join("src");
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&src)
        .map_err(|e| BuildError::Scratch(e.to_string()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    entries.sort();
    files.extend(entries);
    for file in files {
        hasher.update(file.file_name().unwrap_or_default().as_encoded_bytes());
        hasher.update(b"\0");
        let bytes =
            std::fs::read(&file).map_err(|e| BuildError::Scratch(format!("{file:?}: {e}")))?;
        hasher.update(&bytes);
        hasher.update(b"\0");
    }
    let digest = hasher.finalize();
    Ok(digest[..12].iter().map(|b| format!("{b:02x}")).collect())
}
