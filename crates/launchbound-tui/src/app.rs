//! TUI state and rendering: pure functions of (report, view, scroll) so
//! every frame is deterministic — no clocks, no animations, nothing that
//! could flake a golden (see the golden tests).

use launchbound_report::Report;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Overview,
    Ranking,
    Rejections,
    Progress,
}

pub struct App {
    pub report: Report,
    pub view: View,
    pub scroll: usize,
    /// Planned candidate count (from plan.json), for the progress view.
    pub planned: usize,
}

impl App {
    pub fn new(report: Report, planned: usize) -> Self {
        App {
            report,
            view: View::Overview,
            scroll: 0,
            planned,
        }
    }

    pub fn select(&mut self, view: View) {
        self.view = view;
        self.scroll = 0;
    }

    pub fn scroll_down(&mut self) {
        self.scroll = self.scroll.saturating_add(1);
    }

    pub fn scroll_up(&mut self) {
        self.scroll = self.scroll.saturating_sub(1);
    }
}

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(if app.report.convergence_gate == "none" {
                3
            } else {
                2
            }),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

    draw_header(frame, app, chunks[0]);
    match app.view {
        View::Overview => draw_overview(frame, app, chunks[1]),
        View::Ranking => draw_ranking(frame, app, chunks[1]),
        View::Rejections => draw_rejections(frame, app, chunks[1]),
        View::Progress => draw_progress(frame, app, chunks[1]),
    }
    let help =
        Paragraph::new("1 overview · 2 ranking · 3 rejections · 4 progress · j/k scroll · q quit");
    frame.render_widget(help, chunks[2]);
}

fn draw_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let r = &app.report;
    let mut lines = vec![Line::from(vec![
        Span::styled(
            "launchbound ",
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "— {} · gate cc {} · {} · {}",
            r.kernel,
            r.gate_cc,
            r.measurement_kind,
            r.device
                .as_ref()
                .map(|d| d.name.clone())
                .unwrap_or_else(|| "no device".into())
        )),
    ])];
    // The Metal asymmetry is published, never buried: this banner cannot be
    // disabled (the same rule as the text renderer — see the golden tests).
    if r.convergence_gate == "none" {
        lines.push(Line::from(Span::styled(
            "NO convergence gate exists on the Metal path: the same bug class is NOT checked",
            Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
        )));
    }
    lines.push(Line::from(format!(
        "{} candidates · {} admitted · {} refused · {} measured ok",
        r.totals.candidates, r.totals.admitted, r.totals.refused, r.totals.measured_ok
    )));
    frame.render_widget(Paragraph::new(lines), area);
}

const CHOSEN_LABEL: &str = "CHOSEN  ";

/// The chosen configuration, with its interval only if the interval fits.
///
/// At eighty columns — the default terminal size, and the width this suite
/// mandates — the line used to be cut mid-value:
///
/// ```text
/// │CHOSEN  c1-0000000000000009  block_x=32 tile=512 unroll=4  0.0400 ms [0.0398, │
/// ```
///
/// That is not a shortened interval, it is a number with no upper bound and
/// a dangling comma, on the one line carrying the result the reader came
/// for. Dropping the interval whole is the honest shortening: the
/// configuration and its time outrank the interval, and a reader who needs
/// the interval has view 2 and a wider terminal.
fn chosen_tail(
    id: &str,
    config: &str,
    median_ms: f64,
    lo_ms: f64,
    hi_ms: f64,
    panel_width: u16,
) -> String {
    let without = format!("{id}  {config}  {median_ms:.4} ms");
    let with = format!("{without} [{lo_ms:.4}, {hi_ms:.4}]");
    // The panel's two border columns are not text.
    let usable = usize::from(panel_width).saturating_sub(2 + CHOSEN_LABEL.len());
    if with.chars().count() <= usable {
        with
    } else {
        without
    }
}

/// The banner above the field when refused configurations measured faster.
///
/// A function rather than an inline `format!` so both arities can be tested.
/// The count comes from measured timings, and the one fixture the golden
/// frames are built on yields exactly one — so the plural branch is not
/// reachable from a rendered frame without rebuilding that fixture, which
/// would move every golden in the suite to cover two words.
fn refused_faster_banner(count: usize) -> String {
    let noun = if count == 1 {
        "configuration"
    } else {
        "configurations"
    };
    format!("{count} REFUSED {noun} measured FASTER than the chosen one — view 3")
}

fn draw_overview(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let r = &app.report;
    let mut lines = Vec::new();
    match &r.chosen {
        Some(chosen) => {
            let s = &chosen.summary;
            lines.push(Line::from(vec![
                Span::styled(CHOSEN_LABEL, Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(chosen_tail(
                    &chosen.id,
                    &chosen.config,
                    s.median_ms,
                    s.ci95_lo_ms,
                    s.ci95_hi_ms,
                    area.width,
                )),
            ]));
            if !r.indistinguishable_from_chosen.is_empty() {
                lines.push(Line::from(format!(
                    "indistinguishable: {}",
                    r.indistinguishable_from_chosen.join(", ")
                )));
            }
        }
        None => lines.push(Line::from("CHOSEN: none — nothing measured yet")),
    }
    if !r.rejected_faster.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            refused_faster_banner(r.rejected_faster.len()),
            Style::default().add_modifier(Modifier::BOLD),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(format!(
        "GPU-seconds consumed: {:.1}",
        r.totals.gpu_seconds
    )));

    // The field, against the winner. An autotuner's result is not a
    // configuration, it is the claim that the configuration is *worth
    // choosing* — and that claim is unreadable without the alternatives. A
    // field within a percent says the tuning did not matter; one spanning 4x
    // says it did, and the overview is where that belongs. The list is the
    // head of view 2's, so the two can never disagree.
    let measured = measured_fastest_first(app);
    if measured.len() > 1 {
        // Everything the fixed lines above did not take, less the heading and
        // the borders. The overview never scrolls, so the field is truncated
        // rather than paged; view 2 is the whole list and says so.
        let room = usize::from(area.height)
            .saturating_sub(lines.len() + 3)
            .min(measured.len());
        if room > 1 {
            let more = measured.len() - room;
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                if more > 0 {
                    format!("the field, fastest first — {more} more in view 2")
                } else {
                    "the field, fastest first".to_string()
                },
                Style::default().add_modifier(Modifier::BOLD),
            )));

            let best = measured[0]
                .summary
                .as_ref()
                .expect("filtered on Some")
                .median_ms;
            let chosen_id = r.chosen.as_ref().map(|c| c.id.as_str());
            for candidate in measured.iter().take(room) {
                let summary = candidate.summary.as_ref().expect("filtered on Some");
                // Relative to the fastest, not to the chosen: when the chosen
                // one was refused a faster rival the difference is the whole
                // story, and anchoring on the winner would hide it.
                let relative = if best > 0.0 && summary.median_ms > best {
                    format!("{:+.1}%", (summary.median_ms / best - 1.0) * 100.0)
                } else {
                    String::new()
                };
                let marker = if Some(candidate.id.as_str()) == chosen_id {
                    "»"
                } else if candidate.verdict == "disqualified" {
                    "x"
                } else {
                    " "
                };
                lines.push(Line::from(format!(
                    "{marker} {:<24} {:>9.4} ms  {relative:>6}",
                    candidate.config, summary.median_ms
                )));
            }
        }
    }

    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("overview")),
        area,
    );
}

/// Every measured candidate, fastest first.
///
/// Shared by the overview and the ranking so the two can never disagree about
/// what "next fastest" means — the overview shows the head of exactly the list
/// view 2 pages through.
fn measured_fastest_first(app: &App) -> Vec<&launchbound_report::CandidateReport> {
    let mut measured: Vec<_> = app
        .report
        .candidates
        .iter()
        .filter(|c| c.measurement_status == "ok" && c.summary.is_some())
        .collect();
    measured.sort_by(|a, b| {
        let (sa, sb) = (a.summary.as_ref().unwrap(), b.summary.as_ref().unwrap());
        sa.median_ms.partial_cmp(&sb.median_ms).expect("no NaN")
    });
    measured
}

fn draw_ranking(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let measured = measured_fastest_first(app);
    let chosen_id = app.report.chosen.as_ref().map(|c| c.id.as_str());
    let items: Vec<ListItem<'_>> = measured
        .iter()
        .skip(app.scroll)
        .map(|c| {
            let s = c.summary.as_ref().unwrap();
            let marker = if Some(c.id.as_str()) == chosen_id {
                "»"
            } else if c.verdict == "disqualified" {
                "x"
            } else {
                " "
            };
            ListItem::new(format!(
                "{marker} {}  {}  {:.4} ms [{:.4}, {:.4}]",
                c.id, c.config, s.median_ms, s.ci95_lo_ms, s.ci95_hi_ms
            ))
        })
        .collect();
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("ranking ({} measured)", measured.len())),
        ),
        area,
    );
}

fn draw_rejections(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mut lines = Vec::new();
    if !app.report.rejected_faster.is_empty() {
        lines.push(Line::from(Span::styled(
            "REFUSED BUT FASTER — a tuner without a convergence gate would hand you one:",
            Style::default().add_modifier(Modifier::BOLD),
        )));
        for r in &app.report.rejected_faster {
            let s = &r.summary;
            lines.push(Line::from(format!(
                "  {}  {}  {:.4} ms — {:.2}x faster",
                r.id, r.config, s.median_ms, r.speedup_vs_chosen
            )));
            for rule in &r.rules {
                lines.push(Line::from(format!(
                    "    {} at {}: {}",
                    rule.rule,
                    rule.span.as_deref().unwrap_or("<no span>"),
                    rule.reason
                )));
            }
        }
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        "all refused configurations:",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    for c in app
        .report
        .candidates
        .iter()
        .filter(|c| c.verdict == "disqualified")
    {
        lines.push(Line::from(format!("  x {}  {}", c.id, c.config)));
        for rule in &c.rules {
            lines.push(Line::from(format!(
                "      {} at {}: {}",
                rule.rule,
                rule.span.as_deref().unwrap_or("<no span>"),
                rule.reason
            )));
        }
    }
    let visible: Vec<Line<'_>> = lines.into_iter().skip(app.scroll).collect();
    frame.render_widget(
        Paragraph::new(visible).block(Block::default().borders(Borders::ALL).title("rejections")),
        area,
    );
}

fn draw_progress(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let measured = app
        .report
        .candidates
        .iter()
        .filter(|c| c.measurement_status != "unmeasured")
        .count();
    let planned = app.planned.max(measured);
    let mut lines = vec![
        Line::from(format!(
            "measured {measured} of {planned} planned candidates"
        )),
        Line::from(bar(
            measured,
            planned,
            area.width.saturating_sub(4) as usize,
        )),
    ];
    for c in app
        .report
        .candidates
        .iter()
        .filter(|c| c.measurement_status != "unmeasured")
        .skip(app.scroll)
    {
        lines.push(Line::from(format!(
            "  {} {}  {}",
            match c.measurement_status.as_str() {
                "ok" => "+",
                "timeout" => "T",
                _ => "!",
            },
            c.id,
            c.config
        )));
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("progress")),
        area,
    );
}

fn bar(done: usize, total: usize, width: usize) -> String {
    if total == 0 || width == 0 {
        return String::new();
    }
    let filled = done * width / total;
    let mut s = String::with_capacity(width);
    for i in 0..width {
        s.push(if i < filled { '#' } else { '.' });
    }
    s
}

#[cfg(test)]
mod tests {
    use super::refused_faster_banner;

    #[test]
    fn the_chosen_line_drops_its_interval_rather_than_cutting_it() {
        let tail = |w| {
            super::chosen_tail(
                "c1-0000000000000009",
                "block_x=32 tile=512 unroll=4",
                0.0400,
                0.0398,
                0.0402,
                w,
            )
        };
        // 110 columns: room for all of it.
        assert!(tail(110).ends_with("[0.0398, 0.0402]"), "{}", tail(110));
        // 80 columns: the interval goes, whole.
        let narrow = tail(80);
        assert!(narrow.ends_with("0.0400 ms"), "{narrow}");
        assert!(!narrow.contains('['), "no half-interval: {narrow}");
        // And what is left fits the panel.
        assert!(
            narrow.chars().count() + 2 + super::CHOSEN_LABEL.len() <= 80,
            "{narrow}"
        );
    }

    #[test]
    fn the_refused_faster_banner_agrees_with_its_own_count() {
        assert_eq!(
            refused_faster_banner(1),
            "1 REFUSED configuration measured FASTER than the chosen one — view 3"
        );
        assert_eq!(
            refused_faster_banner(2),
            "2 REFUSED configurations measured FASTER than the chosen one — view 3"
        );
        // Not reachable through the view — the banner is drawn only when the
        // list is non-empty — but the function is total, so it is pinned.
        assert!(refused_faster_banner(0).starts_with("0 REFUSED configurations"));
    }
}
