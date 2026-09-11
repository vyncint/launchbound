//! What the emulator can and cannot see of launchbound-tui — the assertion
//! the rest of the suite rests on.
//!
//! Every screen assertion in this crate reads a grid that a VT emulator
//! produced from the binary's bytes: five golden files, two border scans and
//! a styled banner. If the binary emits a sequence the emulator does not
//! implement, that grid is quietly wrong and all of them are being made
//! against a plausible-looking fiction — a golden blessed from it would
//! record the fiction and pass forever. termlens 0.10 made that checkable:
//! `Screen::unsupported` lists what was dropped.
//!
//! These are whole-suite invariants rather than feature tests. They are
//! cheap, and when one breaks the right response is to distrust `tui.rs`
//! until it is understood.

use std::path::{Path, PathBuf};
use std::time::Duration;

use termlens::{Key, Screen, Terminal};

const TIMEOUT: Duration = Duration::from_secs(10);

/// The complete set of sequences launchbound-tui emits that termlens does
/// not model. Pinned exactly, and measured rather than guessed: every
/// non-CUP sequence in the byte stream is `^[[?1049h/l` (alternate screen),
/// `^[[?25l/h` (cursor visibility), `^[[?2026h/l` (DEC 2026 synchronized
/// update), `^[[1m`/`^[[22m` (bold on/off), `^[[7m` (reverse — the Metal
/// banner only), `^[[39m`, `^[[49m`, `^[[59m` and `^[[0m`.
///
/// Of those, `SGR 59` — "underline colour: default", which ratatui writes as
/// part of resetting a style — is the only one termlens drops. termlens
/// carries no underline colour, so it records the sequence and moves on, and
/// because the attribute changes no cell, nothing on the grid is wrong as a
/// result. That is what lets this list be pinned exactly: anything joining
/// it is a sequence that *might* change a cell, and would need reading
/// before the goldens are trusted again.
///
/// The pin carries no known-defect caveat. It used to note that termlens
/// reported blink and strikethrough as unsupported although its attribute
/// shadow implements them (termlens#320); that was fixed in termlens 0.10.2,
/// so an entry here is a real gap whatever this application's modifiers are
/// — and they are only `Modifier::BOLD` and `Modifier::BOLD |
/// Modifier::REVERSED` in any case.
const EXPECTED_UNSUPPORTED: [&str; 1] = ["^[[59m"];

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn spawn(run_dir: &str, size: (u16, u16)) -> termlens::Result<Terminal> {
    let mut t = Terminal::builder()
        .size(size.0, size.1)
        .env_clear()
        .timeout(TIMEOUT)
        .arg(fixture(run_dir))
        .spawn(env!("CARGO_BIN_EXE_launchbound-tui"))?;
    // A quiet PTY is not a painted PTY (see tui.rs): sync on content. The
    // header is the one line every fixture and every width shows.
    t.wait_until(|s| s.contains("launchbound"))?;
    Ok(t)
}

/// The views, as (key, a needle true only of that view).
///
/// The panel's own top border, because it is the one marker that is unique
/// to a view at every width and on both fixtures — the body text is not
/// (`measured ` is in the header totals of every frame, and the gate=none
/// run words its rejection panel differently). A predicate true of frames
/// other than the one being waited for is the shape that already flaked
/// this suite once; see the comment in `resize_relayouts_the_frame`.
const VIEWS: [(char, &str); 3] = [
    ('2', "\u{250c}ranking"),
    ('3', "\u{250c}rejections"),
    ('4', "\u{250c}progress"),
];

fn check(label: &str, screen: &Screen) {
    // One comparison for both halves of the record: termlens 0.11's
    // `Unsupported` view is equal to a slice only when the retained shapes
    // match *and* nothing overflowed the bound, so a truncated record fails
    // here rather than passing as a shorter list.
    assert_eq!(
        screen.unsupported(),
        EXPECTED_UNSUPPORTED,
        "{label}: launchbound-tui emitted a sequence termlens does not \
         model, or the record was truncated. Until it is understood, every \
         golden in this crate is being held against a grid that may be \
         wrong:\n{screen}"
    );
}

/// The invariant, over both fixtures, all four views and both widths the
/// suite uses.
///
/// The gate=none fixture is in here on purpose: it is the only run that
/// takes the `^[[7m` reverse-video path, so it is the only one whose bytes
/// would show a dropped SGR that the other four views never emit.
#[test]
fn the_emulator_drops_nothing_that_could_change_a_cell() -> termlens::Result<()> {
    for (run, size) in [
        ("run-flip", (80u16, 24u16)),
        ("run-flip", (60, 30)),
        ("run-metal", (80, 24)),
        ("run-metal", (60, 30)),
    ] {
        let mut t = spawn(run, size)?;
        let first = t.wait_frame(|s| s.contains("candidates ·"))?;
        check(&format!("{run} {}x{} overview", size.0, size.1), &first);

        for (key, needle) in VIEWS {
            t.send(Key::Char(key))?;
            let frame = t.wait_frame(|s| s.contains(needle))?;
            check(&format!("{run} {}x{} view {key}", size.0, size.1), &frame);
        }

        // And the teardown, which is bytes nothing else here looks at: the
        // leave-alternate-screen and cursor-restore that run after the loop.
        t.send(Key::Char('q'))?;
        assert!(t.wait_exit()?.success());
        check(
            &format!("{run} {}x{} after exit", size.0, size.1),
            &t.screen(),
        );
    }
    Ok(())
}

/// Four smaller invariants that would each make the grid a lie, and that
/// nothing else in this crate would notice.
#[test]
fn the_tui_leaves_the_terminal_modes_alone() -> termlens::Result<()> {
    let mut t = spawn("run-flip", (80, 24))?;
    let screen = t.wait_frame(|s| s.contains("q quit"))?;

    // Insert mode pushes the rest of a row right. An application that left
    // it on would draw a correct-looking panel with every row shifted, and
    // the goldens would record the shift as the layout.
    assert!(!screen.insert_mode(), "launchbound-tui never sets IRM");
    // A bell is a thing the grid cannot show, so a golden cannot catch one.
    assert_eq!(screen.visual_bells(), 0, "no visual bell");
    assert_eq!(screen.bells(), 0, "and no audible one either");

    // Nothing captures the mouse. This is the assertion that would catch a
    // crossterm or ratatui upgrade quietly enabling mouse reporting: the
    // frame would be identical, and the user would silently lose the ability
    // to select text in the pane.
    assert!(
        screen.mouse_modes().is_empty(),
        "the TUI enables no mouse reporting, got {:?}",
        screen.mouse_modes()
    );
    assert!(!screen.bracketed_paste(), "and no bracketed paste");
    assert!(!screen.focus_events(), "and no focus reporting");
    assert!(
        !screen.application_cursor(),
        "and no application cursor keys"
    );
    assert!(!screen.cursor().2, "the cursor is hidden while drawing");

    t.send(Key::Char('q'))?;
    assert!(t.wait_exit()?.success());
    Ok(())
}

/// No row wraps — including at sixty columns, where the refusal reason is
/// visibly broken across five rows.
///
/// That reads like a contradiction and is the point. ratatui's
/// `Wrap { trim: false }` does the breaking itself and emits each visual row
/// as its own line, so the terminal's own autowrap never fires and the wrap
/// flag is never set. The consequence is concrete: `logical_text()` will
/// *not* rejoin the refusal reason, which is why
/// `a_refusal_reason_survives_a_narrow_terminal_whole` joins the rows by
/// hand instead of reaching for it.
///
/// Pinned so that if a future ratatui starts letting the terminal wrap, this
/// fails and names the hand-rolled join as the thing to revisit — rather
/// than the join silently producing doubled text.
#[test]
fn the_renderer_wraps_the_text_itself_so_no_terminal_row_is_wrapped() -> termlens::Result<()> {
    let mut t = spawn("run-flip", (60, 30))?;
    t.wait_frame(|s| s.contains("candidates ·"))?;
    t.send(Key::Char('3'))?;
    let screen = t.wait_frame(|s| s.contains("all refused configurations:"))?;

    let wrapped: Vec<u16> = (0..screen.rows())
        .filter(|row| screen.row_wrapped(*row))
        .collect();
    assert!(
        wrapped.is_empty(),
        "rows {wrapped:?} are terminal-wrapped. The renderer used to break \
         lines itself, and `a_refusal_reason_survives_a_narrow_terminal_whole` \
         rebuilds the prose on that assumption — reread it before changing \
         this:\n{screen}"
    );
    // Deliberately not asserting that the sentence is absent from any single
    // row. It is 58 characters inside a 60-column bordered frame, so it could
    // not fit whatever the renderer did — that assertion is arithmetic
    // dressed as a test. The claim worth making is the one above: no row is
    // *terminal*-wrapped, so the breaking was the renderer's doing.

    t.send(Key::Char('q'))?;
    assert!(t.wait_exit()?.success());
    Ok(())
}

/// A frame has to survive being saved and read back, because that is what a
/// bug report, a `TERMLENS_ARTIFACT_DIR` file and every golden in this crate
/// are.
///
/// The gate=none frame is the one worth checking: it is the only one
/// carrying an attribute (`REVERSED`) that a text comparison cannot see, so
/// a round trip that dropped styles would pass against any other view.
#[test]
fn a_frame_survives_the_snapshot_format_and_json() -> termlens::Result<()> {
    let mut t = spawn("run-metal", (80, 24))?;
    let screen = t.wait_frame(|s| s.contains("q quit"))?;

    // The text format: a saved golden, or a block pasted out of a CI log.
    let saved = screen.with_styles().to_string();
    let parsed = Screen::parse(&saved)?;
    assert!(screen.diff(&parsed).is_empty(), "{}", screen.diff(&parsed));
    assert_eq!(parsed.with_styles().to_string(), saved, "byte for byte");

    // And JSON, which is what `TERMLENS_ARTIFACT_DIR` writes in CI now that
    // the dependency carries the `serde` feature — the file the report
    // action renders into the job summary when a wait times out.
    let json = serde_json::to_string(&screen).expect("a Screen serializes");
    let back: Screen = serde_json::from_str(&json).expect("and comes back");
    assert!(screen.diff(&back).is_empty(), "{}", screen.diff(&back));

    // The banner's attributes specifically. Both round trips above compare
    // pictures, and `diff` does see styles — but naming this cell says which
    // property of the format is load-bearing here, and fails with the one
    // sentence that matters if it ever stops holding.
    let banner = screen.cell(1, 0).expect("the banner's first cell");
    assert!(banner.style().bold && banner.style().reverse);
    for other in [&parsed, &back] {
        let cell = other.cell(1, 0).expect("the banner's first cell, restored");
        assert_eq!(
            cell.style(),
            banner.style(),
            "the no-gate banner came back unstyled: a saved screen would \
             read as an ordinary line of text"
        );
    }

    t.send(Key::Char('q'))?;
    assert!(t.wait_exit()?.success());
    Ok(())
}
