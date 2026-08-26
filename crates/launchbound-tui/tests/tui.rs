//! S8 gate tests: golden frames through a real PTY on hermetic fixtures —
//! initial layout, live resize, a long candidate list scrolled, the live
//! search progress view, and the rejection view — plus the 100-iteration
//! stress. No frame contains a clock or an animation.
//!
//! Sync policy: `wait_frame`, and the frame it returns is the one asserted
//! on — never `wait_idle`, never sleep.
//!
//! These used to sync on a 150ms quiet period, which is a guess at how long
//! a repaint takes. On a loaded runner it is the wrong guess: the app pauses
//! mid-repaint, the period elapses, and the screen read is half-painted. It
//! had already cost this suite once — see the comment in
//! `ranking_scrolls_a_long_candidate_list`, where a golden was blessed from
//! a too-early capture and the test then verified nothing while passing.
//! The same shape failed reconverge's `main` on macOS at 2 and 16 threads.
//!
//! The binary brackets every repaint in DEC 2026 synchronized updates, so
//! `wait_frame` observes only whole frames. No duration is involved, so
//! there is no duration to get wrong.
//!
//! Regenerate goldens after an intentional UI change with
//! `LAUNCHBOUND_BLESS=1 cargo test -p launchbound-tui --test tui`.

use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{env, fs};

use termlens::{Key, Terminal};

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
    t.send(Key::Char('q')).expect("send q");
    let status = t.wait_exit().expect("TUI did not exit after q");
    assert!(status.success(), "{context}: exited with {status:?}");
}

/// The overview has painted once its footer is on screen: it is drawn last,
/// so a frame carrying it carries everything above it.
fn ready(screen: &termlens::Screen) -> bool {
    screen.to_string().contains("q quit")
}

#[test]
fn overview_at_80x24() {
    let mut t = spawn((80, 24));
    let frame = t.wait_frame(ready).expect("the first complete frame");
    assert_golden("overview-80x24.txt", &frame.to_string(), "overview");
    quit(t, "overview");
}

/// Live resize: the same session re-laid-out at a new geometry.
#[test]
fn resize_relayouts_the_frame() {
    let mut t = spawn((80, 24));
    // Both waits are one-directional, which is what makes this test
    // deterministic rather than a race. The chosen line has room for its
    // interval at 110 columns and not at 80, so the interval's *absence*
    // identifies a pre-resize frame and its presence a post-resize one.
    //
    // Waiting on `ready` here instead — a predicate true of every frame —
    // is what flaked: it returns the earliest frame nobody has looked at,
    // so on a loaded runner the frame it handed back could already be the
    // one the resize produced, leaving nothing for the second wait and a
    // ten-second timeout. termlens says so in as many words ("has not
    // completed a repaint since the frame this terminal last returned").
    t.wait_frame(|s| {
        let frame = s.to_string();
        frame.contains("q quit") && !frame.contains("[0.0398, 0.0402]")
    })
    .expect("the 80-column frame");
    t.resize(110, 32).expect("resize");
    let frame = t
        .wait_frame(|s| s.to_string().contains("[0.0398, 0.0402]"))
        .expect("the relaid-out frame");
    assert_golden("overview-110x32.txt", &frame.to_string(), "resized");
    quit(t, "resized");
}

/// The long candidate list, scrolled: ranking view plus five steps down.
#[test]
fn ranking_scrolls_a_long_candidate_list() {
    let mut t = spawn((80, 24));
    t.wait_frame(ready).expect("the first complete frame");
    t.send(Key::Char('2')).expect("send 2");
    t.wait_frame(|s| s.to_string().contains("ranking ("))
        .expect("ranking view");
    for _ in 0..5 {
        t.send(Key::Char('j')).expect("send j");
    }
    // Sync on the scroll having APPLIED, not on quiet: at scroll=5 the five
    // pre-scroll top rows (…0a, …09, …01, …02, …03) are gone and …04 leads.
    // The original golden was blessed from a too-early capture and never
    // verified scrolling at all — caught by ubuntu delivering all five keys.
    let frame = t
        .wait_frame(|s| {
            let frame = s.to_string();
            frame.contains("c1-0000000000000004") && !frame.contains("c1-0000000000000003")
        })
        .expect("scroll applied");
    assert_golden("ranking-scrolled-80x24.txt", &frame.to_string(), "ranking");
    quit(t, "ranking");
}

/// The rejection view: rules, spans, and the refused-but-faster headline.
#[test]
fn rejection_view_names_rules_and_spans() {
    let mut t = spawn((80, 24));
    t.wait_frame(ready).expect("the first complete frame");
    t.send(Key::Char('3')).expect("send 3");
    // The help line always contains the word "rejections"; sync on
    // view-body content instead.
    let screen = t
        .wait_frame(|s| s.to_string().contains("all refused configurations:"))
        .expect("rejections view")
        .to_string();
    assert!(screen.contains("RC001"), "rule id visible");
    assert!(screen.contains("src/lib.rs:33:13"), "span visible");
    assert_golden("rejections-80x24.txt", &screen, "rejections");
    quit(t, "rejections");
}

/// Live search progress: measured-of-planned and per-candidate statuses.
#[test]
fn progress_view_shows_measured_of_planned() {
    let mut t = spawn((80, 24));
    t.wait_frame(ready).expect("the first complete frame");
    t.send(Key::Char('4')).expect("send 4");
    let screen = t
        .wait_frame(|s| s.to_string().contains("measured 11 of"))
        .expect("progress view")
        .to_string();
    assert!(screen.contains("measured 11 of"), "progress counter");
    assert_golden("progress-80x24.txt", &screen, "progress");
    quit(t, "progress");
}

/// The S8 stress gate: 100 consecutive spawn → frame → golden → quit cycles.
#[test]
fn stress_100_runs_at_80x24() {
    for run in 0..100 {
        let mut t = spawn((80, 24));
        let frame = t
            .wait_frame(ready)
            .unwrap_or_else(|e| panic!("run {run}: waiting for the first frame: {e}"));
        assert_golden(
            "overview-80x24.txt",
            &frame.to_string(),
            &format!("run {run}"),
        );
        quit(t, &format!("run {run}"));
    }
}
