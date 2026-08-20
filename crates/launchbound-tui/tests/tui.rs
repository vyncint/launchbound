//! S8 gate tests: golden frames through a real PTY on hermetic fixtures —
//! initial layout, live resize, a long candidate list scrolled, the live
//! search progress view, and the rejection view — plus the 100-iteration
//! stress. Sync policy: wait_idle / wait_until only, never sleep. No frame
//! contains a clock or an animation.
//!
//! Regenerate goldens after an intentional UI change with
//! `LAUNCHBOUND_BLESS=1 cargo test -p launchbound-tui --test tui`.

use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{env, fs};

use termlens::{Key, Terminal};

const QUIET: Duration = Duration::from_millis(150);
const TIMEOUT: Duration = Duration::from_secs(10);

fn fixture_run() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/run-flip")
}

fn golden_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

fn normalize(frame: &str) -> String {
    frame
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

fn assert_golden(name: &str, screen: &str, context: &str) {
    let path = golden_path(name);
    let actual = normalize(screen);
    if env::var_os("LAUNCHBOUND_BLESS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("{actual}\n")).unwrap();
    }
    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing golden {name}; bless with LAUNCHBOUND_BLESS=1"));
    assert_eq!(
        normalize(&expected),
        actual,
        "{context}: frame differs from golden {name}\n--- rendered ---\n{screen}"
    );
}

fn spawn(size: (u16, u16)) -> Terminal {
    let mut t = Terminal::builder()
        .size(size.0, size.1)
        .env_clear()
        .timeout(TIMEOUT)
        .arg(fixture_run())
        .spawn(env!("CARGO_BIN_EXE_launchbound-tui"))
        .expect("failed to spawn the TUI in a PTY");
    // A quiet PTY is not a painted PTY: under parallel-test load the first
    // draw can land after an early idle window. Sync on content first.
    t.wait_until(|s| s.to_string().contains("launchbound"))
        .expect("first frame");
    t
}

fn quit(mut t: Terminal, context: &str) {
    t.send(Key::Char('q'));
    let status = t.wait_exit().expect("TUI did not exit after q");
    assert!(status.success(), "{context}: exited with {status:?}");
}

#[test]
fn overview_at_80x24() {
    let mut t = spawn((80, 24));
    t.wait_idle(QUIET).expect("wait_idle");
    assert_golden("overview-80x24.txt", &t.screen().to_string(), "overview");
    quit(t, "overview");
}

/// Live resize: the same session re-laid-out at a new geometry.
#[test]
fn resize_relayouts_the_frame() {
    let mut t = spawn((80, 24));
    t.wait_idle(QUIET).expect("initial idle");
    t.resize(110, 32).expect("resize");
    t.wait_idle(QUIET).expect("post-resize idle");
    assert_golden("overview-110x32.txt", &t.screen().to_string(), "resized");
    quit(t, "resized");
}

/// The long candidate list, scrolled: ranking view plus five steps down.
#[test]
fn ranking_scrolls_a_long_candidate_list() {
    let mut t = spawn((80, 24));
    t.wait_idle(QUIET).expect("idle");
    t.send(Key::Char('2'));
    t.wait_until(|s| s.to_string().contains("ranking ("))
        .expect("ranking view");
    t.wait_idle(QUIET).expect("view settled");
    for _ in 0..5 {
        t.send(Key::Char('j'));
    }
    t.wait_idle(QUIET).expect("scrolled idle");
    assert_golden(
        "ranking-scrolled-80x24.txt",
        &t.screen().to_string(),
        "ranking",
    );
    quit(t, "ranking");
}

/// The rejection view: rules, spans, and the refused-but-faster headline.
#[test]
fn rejection_view_names_rules_and_spans() {
    let mut t = spawn((80, 24));
    t.wait_idle(QUIET).expect("idle");
    t.send(Key::Char('3'));
    // The help line always contains the word "rejections"; sync on
    // view-body content instead.
    t.wait_until(|s| s.to_string().contains("all refused configurations:"))
        .expect("rejections view");
    t.wait_idle(QUIET).expect("settled");
    let screen = t.screen().to_string();
    assert!(screen.contains("RC001"), "rule id visible");
    assert!(screen.contains("src/lib.rs:33:13"), "span visible");
    assert_golden("rejections-80x24.txt", &screen, "rejections");
    quit(t, "rejections");
}

/// Live search progress: measured-of-planned and per-candidate statuses.
#[test]
fn progress_view_shows_measured_of_planned() {
    let mut t = spawn((80, 24));
    t.wait_idle(QUIET).expect("idle");
    t.send(Key::Char('4'));
    t.wait_until(|s| s.to_string().contains("measured 11 of"))
        .expect("progress view");
    t.wait_idle(QUIET).expect("settled");
    let screen = t.screen().to_string();
    assert!(screen.contains("measured 11 of"), "progress counter");
    assert_golden("progress-80x24.txt", &screen, "progress");
    quit(t, "progress");
}

/// The S8 stress gate: 100 consecutive spawn → idle → golden → quit cycles.
#[test]
fn stress_100_runs_at_80x24() {
    for run in 0..100 {
        let mut t = spawn((80, 24));
        t.wait_idle(QUIET)
            .unwrap_or_else(|e| panic!("run {run}: wait_idle: {e}"));
        assert_golden(
            "overview-80x24.txt",
            &t.screen().to_string(),
            &format!("run {run}"),
        );
        quit(t, &format!("run {run}"));
    }
}
