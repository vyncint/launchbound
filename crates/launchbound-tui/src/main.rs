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

fn main() -> anyhow::Result<()> {
    let run_dir = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("usage: launchbound-tui <run-dir>"))?;
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
