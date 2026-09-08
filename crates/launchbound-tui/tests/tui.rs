//! S8 gate tests: golden frames through a real PTY on hermetic fixtures —
//! initial layout, live resize, a long candidate list scrolled, the live
//! search progress view, and the rejection view — plus the 100-iteration
//! stress. No frame contains a clock or an animation.
//!
//! Two sibling files carry the rest: `emulation.rs` asserts that the grid
//! every golden here rests on is one the emulator saw whole, and `cli.rs`
//! drives `termlens-cli` over these goldens and the real binary.
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

/// A run directory shipped *inside* this crate. It has to be inside it:
/// `cargo package -p launchbound-tui --list` ships `src/`, `tests/` and
/// nothing above them, so a test reading `../../runs/...` would be published
/// with its data missing and fail for every downstream consumer — and
/// release.yml publishes with `--no-verify`, so nothing in CI would say so.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn fixture_run() -> PathBuf {
    fixture("run-flip")
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

/// A *styled* golden, compared verbatim.
///
/// `with_styles()` writes three things a plain golden does not have: the
/// `size:`/`cursor:` header, the grid, and a `styles:` block naming the run
/// of columns each attribute covers. It goes through neither `normalize`
/// nor `to_string()`: this file is read back by `Screen::parse` in the same
/// test, and the round trip is only byte-exact against what `with_styles`
/// actually wrote.
///
/// Blessed by the same `LAUNCHBOUND_BLESS=1` that blesses the plain
/// goldens, so the documented regeneration command covers this one too.
fn assert_styled_golden(name: &str, screen: &termlens::Screen, context: &str) -> String {
    let path = golden_path(name);
    let actual = screen.with_styles().to_string();
    if env::var_os("LAUNCHBOUND_BLESS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("{actual}\n")).unwrap();
    }
    let expected = fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing golden {name}; bless with LAUNCHBOUND_BLESS=1"));
    assert_eq!(
        expected.trim_end_matches('\n'),
        actual,
        "{context}: styled frame differs from golden {name}"
    );
    expected
}

fn spawn_in(run_dir: PathBuf, size: (u16, u16)) -> Terminal {
    let mut t = Terminal::builder()
        .size(size.0, size.1)
        .env_clear()
        .timeout(TIMEOUT)
        .arg(run_dir)
        .spawn(env!("CARGO_BIN_EXE_launchbound-tui"))
        .expect("failed to spawn the TUI in a PTY");
    // A quiet PTY is not a painted PTY: under parallel-test load the first
    // draw can land after an early idle window. Sync on content first.
    t.wait_until(|s| s.to_string().contains("launchbound"))
        .expect("first frame");
    t
}

fn spawn(size: (u16, u16)) -> Terminal {
    spawn_in(fixture_run(), size)
}

fn quit(mut t: Terminal, context: &str) {
    t.send(Key::Char('q')).expect("send q");
    let status = t.wait_exit().expect("TUI did not exit after q");
    assert!(status.success(), "{context}: exited with {status:?}");
    // main.rs leaves the alternate screen *after* the event loop returns,
    // on every path. Nothing asserted it, so a `?` that skipped the
    // teardown, or a panic escaping the loop, would leave the user's shell
    // painted with a dead frame and every test here still green — this is
    // a published binary that takes the terminal over. It is the vendored
    // skill's own rule, and it belongs in `quit` so every test that spawns
    // makes it rather than one.
    //
    // Not a second instant to race against: the child has exited, so no
    // further byte can arrive and this screen is final.
    assert!(
        !t.screen().alternate_screen(),
        "{context}: the TUI exited without leaving the alternate screen"
    );
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
    let before = t
        .wait_frame(|s| {
            let frame = s.to_string();
            frame.contains("q quit") && !frame.contains("[0.0398, 0.0402]")
        })
        .expect("the 80-column frame");
    t.resize(110, 32).expect("resize");
    let frame = t
        .wait_frame(|s| s.to_string().contains("[0.0398, 0.0402]"))
        .expect("the relaid-out frame");
    assert_golden("overview-110x32.txt", &frame.to_string(), "resized");
    // The subject of this test, said directly rather than inferred from two
    // files agreeing with two other files: the frame *changed*. Counting
    // changed cells rather than `!is_empty()` on purpose — a `ScreenDiff`
    // between two different sizes is non-empty from the sizes alone, which
    // would be true of an application that ignored SIGWINCH entirely. These
    // are cells inside the eighty-column overlap, so they can only come from
    // a re-layout. Its `Display` prints them when it fails.
    let diff = before.diff(&frame);
    assert!(
        diff.cells().count() > 0,
        "the 110-column frame re-laid out nothing inside the old width:\n{diff}"
    );
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

/// No shipped frame is cut mid-value or mid-word without an ellipsis.
///
/// Three views had this defect and two were fixed one at a time: #24 took
/// the chosen line, and the ranking and rejection views kept it — the
/// ranking losing every closing bracket, so each interval read as a number
/// with no upper bound, and the rejections losing the clause that says what
/// to do about the refusal. A golden is a recording of shipped behaviour,
/// so the goldens are where the scan belongs.
///
/// The rule is what a rendered field may *end* with at the panel border. A
/// digit, `,`, `[`, `(`, `=` or `-` there means the value continued and was
/// cut; a letter means a word was. An ellipsis is allowed: a marked
/// shortening is a choice, and an unmarked one is a bug.
#[test]
fn no_golden_line_is_cut_at_the_panel_border() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden");
    let mut checked = 0;
    for entry in fs::read_dir(&dir).expect("tests/golden must exist") {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = fs::read_to_string(&path).unwrap();
        for (number, line) in text.lines().enumerate() {
            // Only rows that reach a right border can be cut by one.
            let Some(inner) = line.strip_suffix('│') else {
                continue;
            };
            let Some(inner) = inner.strip_prefix('│') else {
                continue;
            };
            let Some(last) = inner.chars().next_back() else {
                continue;
            };
            // A box-drawing row is the border itself, not content.
            if inner.chars().all(|c| c == '─' || c == ' ') {
                continue;
            }
            if last == '…' {
                continue;
            }
            let cut = last.is_ascii_digit() || matches!(last, ',' | '[' | '(' | '=' | '-');
            assert!(
                !cut,
                "{name}:{}: a value is cut at the panel border (ends {last:?}):\n{line}",
                number + 1
            );
            // A word cut mid-way. A field that legitimately ends in a letter
            // (`ms`, a kernel name) is indistinguishable from a truncated
            // one by the last character alone, so this only fires when the
            // row is full to the border AND the last word is long enough to
            // be a sentence rather than a unit.
            let full = inner.chars().count() >= 76;
            let tail = inner.split_whitespace().next_back().unwrap_or("");
            assert!(
                !(full && last.is_alphabetic() && tail.len() > 6),
                "{name}:{}: a word is cut at the panel border ({tail:?}):\n{line}",
                number + 1
            );
        }
        checked += 1;
    }
    assert!(checked > 0, "no goldens found in {dir:?}");
}

/// The refusal reason reaches the reader whole, at a width where it does not
/// fit on one row.
///
/// This is a property of the rendered grid, which is why it is here and not
/// a string assertion: the reason is *wrapped* across rows now, so the
/// sentence exists only as a sequence of cells. At eighty columns the reader
/// used to get `splits a 64-threa` and never reach `safe only at one warp
/// (<= 32 threads)` — the only part that says what to do about the refusal —
/// with nothing marking the cut, so it read as the whole reason.
///
/// Narrower than the goldens on purpose: 60 columns is where wrapping has to
/// do real work, and the golden suite has no frame there.
#[test]
fn a_refusal_reason_survives_a_narrow_terminal_whole() {
    let mut t = spawn((60, 30));
    // NOT `ready`: that predicate looks for the footer's `q quit`, and at
    // sixty columns the footer itself is cut before it reaches those words.
    // A readiness marker has to hold at the width being tested, which is
    // the sort of thing only a narrow-terminal test finds out.
    let first = t
        .wait_frame(|s| s.to_string().contains("candidates ·"))
        .expect("the first complete frame");
    // And that is a measurement, not a note: the footer keeps four of its
    // five separators here and loses the words `ready` waits for. Pinning it
    // means the day the footer starts fitting at sixty columns, this comment
    // stops being a fact and says so, rather than quietly misinforming the
    // next person who writes a narrow-terminal test.
    let footer = first.rows() - 1;
    let separators = first
        .find_all("·")
        .into_iter()
        .filter(|(row, _)| *row == footer)
        .count();
    assert_eq!(
        separators, 4,
        "the sixty-column footer is cut mid-list:\n{first}"
    );
    assert!(
        first.locate("q quit").is_none(),
        "`ready` would hold at sixty columns after all — reread the comment \
         above and the one in AGENTS.md:\n{first}"
    );
    t.send(Key::Char('3')).expect("send 3");
    let frame = t
        .wait_frame(|s| s.to_string().contains("all refused configurations:"))
        .expect("the rejections view");

    // Rebuild the panel's prose from the grid: wrapping breaks at spaces, so
    // joining the rows and collapsing whitespace recovers the sentence.
    let joined = frame
        .to_string()
        .lines()
        .map(|line| line.trim_matches(['│', ' ']))
        .collect::<Vec<_>>()
        .join(" ");
    let prose: String = joined.split_whitespace().collect::<Vec<_>>().join(" ");

    assert!(
        prose.contains("safe only at one warp (<= 32 threads)"),
        "the actionable clause must reach the reader at 60 columns:\n{frame}"
    );
    assert!(
        prose.contains("divergence source `warp_id()` splits a 64-thread block"),
        "and so must the rest of the reason:\n{frame}"
    );

    // And nothing is cut at the border. The same rule the golden scan
    // applies, asserted here against a live grid at a width no golden covers.
    for row in 0..frame.rows() {
        let text = frame.row_text(row);
        let Some(inner) = text.strip_suffix('│').and_then(|t| t.strip_prefix('│')) else {
            continue;
        };
        if inner.chars().all(|c| c == '─' || c == ' ') {
            continue;
        }
        if let Some(last) = inner.chars().next_back() {
            assert!(
                last == '…' || !(last.is_ascii_digit() || matches!(last, ',' | '[' | '(' | '=')),
                "row {row} is cut at the border (ends {last:?}):\n{frame}"
            );
        }
    }

    quit(t, "narrow rejections");
}

/// The Metal banner is bold *and* reverse-video, and nothing else on the
/// frame is.
///
/// This is the project's central honesty claim reaching a screen. `app.rs`
/// paints the no-gate notice `BOLD | REVERSED` and its comment says the
/// banner "cannot be disabled (the same rule as the text renderer)" — and
/// until now that rule was enforced only for the *text* renderer, in
/// launchbound-report's `schema_and_golden`, as a plain substring. The TUI
/// had no gate=none frame at all, and a plain-text golden could not tell
/// `BOLD | REVERSED` from unstyled text if it had one: both render the same
/// characters. termlens 0.10's `with_styles()` is what makes the styling
/// assertable, so the claim is finally checked where the user meets it.
///
/// The fixture is `runs/reduce-stable-metal` copied verbatim into the crate
/// (see `fixture`).
#[test]
fn the_metal_banner_is_bold_and_reversed_and_nothing_else_is() {
    const BANNER: &str =
        "NO convergence gate exists on the Metal path: the same bug class is NOT checked";

    let mut t = spawn_in(fixture("run-metal"), (80, 24));
    let frame = t.wait_frame(ready).expect("the first complete frame");

    let at = frame.find(BANNER).unwrap_or_else(|| {
        panic!("the no-gate banner is not on the gate=none frame at all:\n{frame}")
    });
    assert_eq!(at, (1, 0), "the banner is the second line of the header");

    // Every cell of it, not the row: a banner that lost its styling halfway
    // is the failure worth catching, and the row is wider than the text.
    for col in 0..BANNER.chars().count() as u16 {
        let cell = frame
            .cell(1, col)
            .unwrap_or_else(|| panic!("cell (1, {col}) is off the grid"));
        let style = cell.style();
        assert!(
            style.bold && style.reverse,
            "banner cell (1, {col}) {:?} is not bold+reverse — the notice can \
             be read past:\n{frame}",
            cell.contents()
        );
    }

    // And it is the only reversed thing on the frame, so "reversed" still
    // means "this one banner" to a reader who has seen the screen once.
    for row in 0..frame.rows() {
        if row == 1 {
            continue;
        }
        for col in 0..frame.cols() {
            let Some(cell) = frame.cell(row, col) else {
                continue;
            };
            assert!(
                !cell.style().reverse,
                "row {row} col {col} is also reversed, which dilutes the \
                 banner:\n{frame}"
            );
        }
    }

    // The recording. A styled golden pins the rest of the frame's styling
    // too — the bold `launchbound` and the bold section headings.
    //
    // Deliberately no `Screen::parse` round trip here. `assert_styled_golden`
    // has just asserted the file equals this screen's `with_styles()` output,
    // so parsing it back and diffing could only fail if termlens' own round
    // trip were broken — a claim about the harness, not about launchbound,
    // and one `emulation.rs::a_frame_survives_the_snapshot_format_and_json`
    // makes properly against a live screen.
    assert_styled_golden("overview-metal-80x24.styled.txt", &frame, "metal overview");

    quit(t, "metal overview");
}

/// `wait_frame` works here *because* the binary brackets every repaint in
/// DEC 2026 synchronized updates. The module header at the top of this file
/// argues that at length; nothing asserted it.
///
/// It is worth one test because of how it fails: drop the
/// `BeginSynchronizedUpdate` at main.rs:74 and the emulator never sees a
/// frame boundary, so every `wait_frame` in this file times out after ten
/// seconds and reports what the application was showing — which looks like
/// eight broken assertions about the UI rather than one missing mode.
/// `repaints()` names the cause in one line.
#[test]
fn every_repaint_is_bracketed_so_wait_frame_sees_whole_frames() {
    let mut t = spawn((80, 24));
    let first = t.wait_frame(ready).expect("the first complete frame");
    assert_eq!(
        first.repaints(),
        1,
        "the first draw is one bracketed repaint, not zero (unbracketed) and \
         not several (bracketed per widget):\n{first}"
    );

    let mut expected = 1;
    for (key, needle) in [
        ('2', "ranking ("),
        ('3', "all refused configurations:"),
        ('4', "measured 11 of"),
    ] {
        t.send(Key::Char(key)).expect("send a view key");
        let frame = t
            .wait_frame(|s| s.to_string().contains(needle))
            .expect("the view's frame");
        expected += 1;
        assert_eq!(
            frame.repaints(),
            expected,
            "one keystroke is one whole frame ({needle}):\n{frame}"
        );
    }
    quit(t, "repaints");
}

/// The footer reaches the reader whole at eighty columns — every separator
/// and the last word of the last hint.
///
/// The interesting half is that this is the same measurement the sixty-column
/// test makes and gets a different answer to. `ready` rests on `q quit`
/// being on the grid, so every test in this file rests on it; at sixty
/// columns it is not, and that difference has already cost a ten-second
/// timeout. Pinning both ends means a layout change that starts cutting the
/// footer at eighty is a named failure here rather than eight timeouts.
#[test]
fn the_footer_reaches_the_reader_whole_at_eighty_columns() {
    let mut t = spawn((80, 24));
    let frame = t.wait_frame(ready).expect("the first complete frame");

    let footer = frame.rows() - 1;
    let separators: Vec<(u16, u16)> = frame
        .find_all("·")
        .into_iter()
        .filter(|(row, _)| *row == footer)
        .collect();
    assert_eq!(
        separators.len(),
        5,
        "the footer lists six hints separated by five `·`; it is cut:\n{frame}"
    );

    let Some(termlens::Location::Screen { row, col }) = frame.locate("q quit") else {
        panic!("the last hint `q quit` is not on the grid:\n{frame}");
    };
    assert_eq!(row, footer, "and it is on the footer row");
    assert!(
        col > separators.last().unwrap().1,
        "after the last separator:\n{frame}"
    );

    quit(t, "footer");
}
