//! `prepare_scratch` copies the crate, not a subset of it.
//!
//! It took only the entries of `src/` that are files, so `mod util;` with
//! `src/util/mod.rs` — how Rust code is organised past one file — produced a
//! scratch crate that could not compile. The gate then reported
//! `error: could not compile` against a crate whose own `cargo check` is
//! clean, and never said that what it compiled was not the reader's crate.
//!
//! The six corpus kernels are single-file because `corpus/README.md` asks
//! them to be, which is why nothing here caught it.

use launchbound_space::KernelSpec;
use std::fs;
use std::path::{Path, PathBuf};

fn write(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

/// A kernel crate with a module directory, a nested module below it, and a
/// build script — the three things the old copy dropped.
fn kernel_with_a_module_directory(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("lb-scratch-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    write(
        &dir.join("Cargo.toml"),
        "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    );
    write(&dir.join("build.rs"), "fn main() {}\n");
    write(&dir.join("src/lib.rs"), "mod params;\nmod util;\n");
    write(&dir.join("src/params.rs"), "pub const TILE: usize = 128;\n");
    write(&dir.join("src/util/mod.rs"), "pub mod inner;\n");
    write(&dir.join("src/util/inner.rs"), "pub const K: u32 = 1;\n");
    write(
        &dir.join("kernel.toml"),
        "[kernel]\nname = \"probe\"\nentry = \"probe\"\nneeds_cc = \"7.5\"\ndomain = 1\n\n\
         [dims.tile]\nrole = \"spec\"\nvalues = [128]\n",
    );
    dir
}

#[test]
fn a_module_directory_reaches_the_scratch_copy() {
    let dir = kernel_with_a_module_directory("moddir");
    let spec = KernelSpec::load(&dir).expect("the probe kernel.toml loads");
    let root = dir.join("target/launchbound-scratch");
    let scratch = launchbound_build::scratch::prepare_scratch(&spec, &root).expect("scratch");

    for rel in [
        "src/lib.rs",
        "src/params.rs",
        "src/util/mod.rs",
        "src/util/inner.rs",
        // One directory up, and the same omission: a build script the
        // manifest would have run.
        "build.rs",
        "Cargo.toml",
    ] {
        assert!(
            scratch.join(rel).is_file(),
            "{rel} must be in the scratch copy — the gate compiles this, not your crate"
        );
    }
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_build_script_named_by_the_manifest_is_copied_from_where_it_lives() {
    let dir = kernel_with_a_module_directory("buildpath");
    let manifest = dir.join("Cargo.toml");
    let text = fs::read_to_string(&manifest).unwrap();
    fs::write(
        &manifest,
        text.replace("edition", "build = \"tools/gen.rs\"\nedition"),
    )
    .unwrap();
    write(&dir.join("tools/gen.rs"), "fn main() {}\n");

    let spec = KernelSpec::load(&dir).unwrap();
    let root = dir.join("target/launchbound-scratch");
    let scratch = launchbound_build::scratch::prepare_scratch(&spec, &root).unwrap();
    assert!(scratch.join("tools/gen.rs").is_file());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_target_directory_under_src_is_not_dragged_along() {
    let dir = kernel_with_a_module_directory("skiptarget");
    write(&dir.join("src/target/huge.bin"), "not source\n");
    let spec = KernelSpec::load(&dir).unwrap();
    let root = dir.join("target/launchbound-scratch");
    let scratch = launchbound_build::scratch::prepare_scratch(&spec, &root).unwrap();
    assert!(scratch.join("src/lib.rs").is_file());
    assert!(
        !scratch.join("src/target").exists(),
        "build output is not source"
    );
    let _ = fs::remove_dir_all(&dir);
}
