//! launchbound-tui: browse a run directory. Every repaint is bracketed in
//! DEC 2026 synchronized updates; frames are pure functions of state (no
//! clocks, no animations) so the termlens goldens cannot flake.

mod app;

use app::{App, View, draw};
use crossterm::event::{Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen, LeaveAlternateScreen,
    disable_raw_mode, enable_raw_mode,
};
use crossterm::{execute, queue};
use launchbound_report::{RunDir, build_report};
use std::io::Write;
use std::path::PathBuf;

const USAGE: &str = "\
usage: launchbound-tui <run-dir>

  <run-dir>   a directory written by `launchbound stage` or `launchbound tune`,
              holding verdicts.json (and results.json once measured)

  1 overview \u{b7} 2 ranking \u{b7} 3 rejections \u{b7} 4 progress \u{b7} j/k scroll \u{b7} q quit
";

fn main() -> anyhow::Result<()> {
    // One positional, read straight from `args()` — but a leading dash is
    // answered rather than opened as a path. `--help` used to come back as
    // `run dir: --help/verdicts.json: No such file or directory`, which
    // reads as a broken tool rather than as an unknown flag, and this is a
    // published binary.
    let run_dir = match std::env::args().nth(1).as_deref() {
        Some("-h" | "--help") => {
            print!("{USAGE}");
            return Ok(());
        }
        Some("-V" | "--version") => {
            println!("launchbound-tui {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        // Rejecting every other leading dash is what stops this recurring:
        // otherwise `--ascii` or `--no-color` lands here next, as a path.
        Some(flag) if flag.starts_with('-') => {
            return Err(anyhow::anyhow!("unknown option `{flag}`\n\n{USAGE}"));
        }
        Some(path) => PathBuf::from(path),
        None => return Err(anyhow::anyhow!("{USAGE}")),
    };
    let run = RunDir::load(&run_dir)?;
    let planned = run.plan.as_ref().map(|p| p.candidates.len()).unwrap_or(0);
    let report = build_report(&run)?;
    let mut app = App::new(report, planned);

    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = ratatui::backend::CrosstermBackend::new(std::io::stdout());
    let mut terminal = ratatui::Terminal::new(backend)?;

    let result = event_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(std::io::stdout(), LeaveAlternateScreen)?;
    result
}

fn event_loop(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
) -> anyhow::Result<()> {
    loop {
        // DEC 2026 synchronized update around every repaint (S8).
        queue!(std::io::stdout(), BeginSynchronizedUpdate)?;
        terminal.draw(|frame| draw(frame, app))?;
        queue!(std::io::stdout(), EndSynchronizedUpdate)?;
        std::io::stdout().flush()?;

        match crossterm::event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Char('1') => app.select(View::Overview),
                KeyCode::Char('2') => app.select(View::Ranking),
                KeyCode::Char('3') => app.select(View::Rejections),
                KeyCode::Char('4') => app.select(View::Progress),
                KeyCode::Char('j') | KeyCode::Down => app.scroll_down(),
                KeyCode::Char('k') | KeyCode::Up => app.scroll_up(),
                KeyCode::Tab => {
                    let next = match app.view {
                        View::Overview => View::Ranking,
                        View::Ranking => View::Rejections,
                        View::Rejections => View::Progress,
                        View::Progress => View::Overview,
                    };
                    app.select(next);
                }
                _ => {}
            },
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
}
