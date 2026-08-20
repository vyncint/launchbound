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

fn draw_overview(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let r = &app.report;
    let mut lines = Vec::new();
    match &r.chosen {
        Some(chosen) => {
            let s = &chosen.summary;
            lines.push(Line::from(vec![
                Span::styled("CHOSEN  ", Style::default().add_modifier(Modifier::BOLD)),
                Span::raw(format!(
                    "{}  {}  {:.4} ms [{:.4}, {:.4}]",
                    chosen.id, chosen.config, s.median_ms, s.ci95_lo_ms, s.ci95_hi_ms
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
            format!(
                "{} REFUSED configuration(s) measured FASTER than the chosen one — view 3",
                r.rejected_faster.len()
            ),
            Style::default().add_modifier(Modifier::BOLD),
        )));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(format!(
        "GPU-seconds consumed: {:.1}",
        r.totals.gpu_seconds
    )));
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("overview")),
        area,
    );
}

fn draw_ranking(frame: &mut Frame<'_>, app: &App, area: Rect) {
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
