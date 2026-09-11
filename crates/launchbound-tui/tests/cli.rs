//! `termlens-cli` — the command that ships beside the harness this crate's
//! PTY tests already use.
//!
//! The rest of the suite asks whether the TUI draws the right thing. This
//! asks what a maintainer does when it draws the wrong one: point the tool
//! at the binary without writing a test, read a saved screen back, and diff
//! two of them. launchbound is a good case for that — five saved screens
//! live in `tests/golden/`, one of them carries an attribute (`REVERSED`) no
//! plain-text comparison can see, and the pair from
//! `resize_relayouts_the_frame` is two different geometries of the same
//! session.
//!
//! Ignored by default, and that is not a preference. launchbound-tui is
//! published on crates.io, so `cargo test` is something contributors and
//! downstream consumers run; a `cargo test` that quietly `cargo install`s a
//! binary is a surprise a published crate must not spring on anyone. CI asks
//! for them by name (stress.yml), which is also why they cost the `hunt`
//! shards nothing — `--ignored` tests are skipped by the plain runs.
//!
//! ```sh
//! just termlens-cli     # cargo test -p launchbound-tui --test cli -- --ignored
//! ```
//!
//! `TERMLENS_CLI` points at an existing binary and skips the install.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;

/// The termlens version this crate is tested against, read from the
/// lockfile so the tool and the library can never be two different releases
/// — a `render` from a newer CLI than the `Screen` that wrote the file is
/// exactly the confusion this suite exists to rule out.
fn version_under_test() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        // The workspace lockfile when run from a checkout; the one cargo
        // puts beside the manifest when run from a published .crate.
        let lock = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .map(|dir| dir.join("Cargo.lock"))
            .find(|path| path.exists())
            .expect("Cargo.lock is committed at the workspace root");
        let text =
            std::fs::read_to_string(&lock).unwrap_or_else(|e| panic!("{}: {e}", lock.display()));
        let mut lines = text.lines();
        while let Some(line) = lines.next() {
            if line.trim() == "name = \"termlens\"" {
                for next in lines.by_ref() {
                    if let Some(rest) = next.trim().strip_prefix("version = \"") {
                        return rest.trim_end_matches('"').to_owned();
                    }
                }
            }
        }
        panic!("no termlens version in {}", lock.display());
    })
}

/// The `termlens` binary: `$TERMLENS_CLI` if the environment provides one,
/// otherwise installed once under the target directory at the version under
/// test.
fn cli() -> &'static PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        if let Some(given) = std::env::var_os("TERMLENS_CLI") {
            return PathBuf::from(given);
        }
        // CARGO_TARGET_TMPDIR is inside the workspace target directory, so
        // the install is cached between runs and `cargo clean` removes it.
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("termlens-cli");
        let bin = root
            .join("bin")
            .join(format!("termlens{}", std::env::consts::EXE_SUFFIX));
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let status = Command::new(cargo)
            .args(["install", "termlens-cli", "--version", version_under_test()])
            .args(["--locked", "--root"])
            .arg(&root)
            .status()
            .expect("cargo install termlens-cli");
        assert!(
            status.success(),
            "cargo install termlens-cli --version {} failed. It is published \
             alongside the library; if this version of termlens exists on \
             crates.io and termlens-cli does not, the two releases went out \
             of lockstep.",
            version_under_test()
        );
        bin
    })
}

fn run(args: &[&str]) -> Output {
    Command::new(cli())
        .args(args)
        .output()
        .expect("run termlens")
}

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn path(p: &Path) -> String {
    p.to_str().expect("a UTF-8 path").to_owned()
}

#[test]
#[ignore = "needs termlens-cli; run with --ignored (stress.yml does)"]
fn the_tool_and_the_library_are_one_release() {
    let out = run(&["--version"]);
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("termlens {}", version_under_test()),
        "the installed CLI is not the version this crate tests against"
    );
}

/// `inspect` drives the binary without a test being written, which is the
/// first thing a contributor reaches for — and the first thing a bug report
/// should contain.
///
/// Pointed at the gate=none run, because that is the frame the project most
/// needs to survive every path to a reader: the no-gate banner has to be
/// there when the TUI is driven by something that is not this suite.
#[test]
#[ignore = "needs termlens-cli; run with --ignored (stress.yml does)"]
fn inspect_drives_the_real_tui() {
    let run_dir = path(&fixture("run-metal"));
    let out = run(&[
        "inspect",
        "--size",
        "80x24",
        "--idle",
        "600",
        env!("CARGO_BIN_EXE_launchbound-tui"),
        &run_dir,
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let screen = String::from_utf8_lossy(&out.stdout);
    assert!(screen.contains("size: 80x24"), "{screen}");
    assert!(
        screen.contains("NO convergence gate exists on the Metal path"),
        "the no-gate banner reaches a reader who never wrote a test:\n{screen}"
    );
    assert!(
        screen.contains("1 overview · 2 ranking · 3 rejections · 4 progress"),
        "and a whole frame, not a half-painted one:\n{screen}"
    );
    // The trailer goes to **stderr** since termlens 0.11 (termlens#340), so
    // what stdout carries is a saved screen the tool reads back unedited.
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("still running at the deadline"),
        "launchbound-tui is a TUI, so inspect reports the deadline rather \
         than an exit status: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !screen.contains("--- "),
        "and stdout is the screen alone:\n{screen}"
    );
}

/// `render` on this crate's own saved screens.
///
/// The styled golden is the one worth rendering: `REVERSED` is invisible in
/// every text comparison the suite makes, so this is where the saved screen
/// proves it still carries the banner's emphasis into something a person can
/// paste into an issue.
#[test]
#[ignore = "needs termlens-cli; run with --ignored (stress.yml does)"]
fn render_carries_the_banner_out_of_a_saved_screen() {
    let styled = path(&golden("overview-metal-80x24.styled.txt"));

    let html = run(&["render", "--html", &styled]);
    assert!(
        html.status.success(),
        "{}",
        String::from_utf8_lossy(&html.stderr)
    );
    let body = String::from_utf8_lossy(&html.stdout);
    let banner = body
        .lines()
        .find(|line| line.contains("NO convergence gate exists on the Metal path"))
        .unwrap_or_else(|| panic!("the banner is not in the HTML:\n{body}"));
    assert!(
        banner.contains("font-weight:bold") && banner.contains("background:"),
        "the banner lost its emphasis on the way out of the saved screen — \
         it renders as an ordinary line: {banner}"
    );

    let svg = run(&["render", "--svg", &styled]);
    assert!(
        svg.status.success(),
        "{}",
        String::from_utf8_lossy(&svg.stderr)
    );
    let image = String::from_utf8_lossy(&svg.stdout);
    assert!(
        image.contains("NO convergence gate exists on the Metal path"),
        "the banner is in the image:\n{image}"
    );
    assert!(
        image.matches("<rect").count() >= 2,
        "reverse video is a painted background, so the image needs a rect \
         besides the page's own:\n{image}"
    );

    // And the plain goldens, which are the format the other four tests save.
    let text = run(&["render", "--text", &path(&golden("overview-80x24.txt"))]);
    assert!(text.status.success());
    assert!(
        String::from_utf8_lossy(&text.stdout).contains("CHOSEN  c1-0000000000000009"),
        "a plain golden reads back as a screen too"
    );
}

/// `diff`, and the three exit codes a script reads.
///
/// The pair is the one `resize_relayouts_the_frame` produces: the same
/// session at two geometries. What the tool says about it — compared over
/// the overlap, the rest clipped — is the answer that test's diff assertion
/// is making in-process, so a reader who reaches for the CLI after a golden
/// fails gets the same account of it.
#[test]
#[ignore = "needs termlens-cli; run with --ignored (stress.yml does)"]
fn diff_reports_the_three_exit_codes() {
    let narrow = path(&golden("overview-80x24.txt"));
    let wide = path(&golden("overview-110x32.txt"));

    // 0: a screen is the same picture as itself.
    let same = run(&["diff", "--color", "never", &narrow, &narrow]);
    assert_eq!(same.status.code(), Some(0), "a golden equals itself");
    assert!(String::from_utf8_lossy(&same.stdout).contains("no difference"));

    // 1: the screens differ. This is the signal a script reads.
    let out = run(&["diff", "--color", "never", &narrow, &wide]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "the resize re-laid out the frame"
    );
    let rendered = String::from_utf8_lossy(&out.stdout);
    assert!(
        rendered.contains("size: 80x24 → 110x32"),
        "the size delta:\n{rendered}"
    );
    assert!(
        rendered.contains("compared over the 80x24 overlap"),
        "and what that means for the comparison:\n{rendered}"
    );
    assert!(
        rendered.contains("rows unchanged"),
        "and a count of what did not move:\n{rendered}"
    );

    // 2: the tool itself could not run. A run directory is JSON, and JSON is
    // one of the two formats a saved screen can be in — so this is the
    // mistake that is easy to make, not a contrived one.
    let bad = path(&fixture("run-flip").join("verdicts.json"));
    let broken = run(&["diff", "--color", "never", &narrow, &bad]);
    assert_eq!(
        broken.status.code(),
        Some(2),
        "an unreadable file is the tool failing, not a difference"
    );
    assert!(
        String::from_utf8_lossy(&broken.stderr).contains("not a termlens Screen"),
        "and it says so: {}",
        String::from_utf8_lossy(&broken.stderr)
    );
}
