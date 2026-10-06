use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Axis, Block, Cell, Chart, Dataset, GraphType, Paragraph, Row, Table};

use crate::App;
use crate::source::Mode;
use crate::stats::Rate;

const WINDOWS: [(&str, f64); 4] = [("10s", 10.0), ("30s", 30.0), ("1m", 60.0), ("5m", 300.0)];
/// Graph lines: short smoothing (also drives min/max) and the slower trend line.
const LINE_WINDOW: f64 = 5.0;
const AVG_WINDOW: f64 = 30.0;

pub fn draw(f: &mut Frame, app: &App) {
    let [chart, bottom, status] =
        Layout::vertical([Constraint::Min(8), Constraint::Length(11), Constraint::Length(1)]).areas(f.area());
    let [stats, jobs] = Layout::horizontal([Constraint::Length(44), Constraint::Min(20)]).areas(bottom);

    draw_chart(f, app, chart);
    draw_stats(f, app, stats);
    draw_jobs(f, app, jobs);
    draw_status(f, app, status);
}

pub fn fmt_rate(r: f64) -> String {
    match r {
        r if r < 10.0 => format!("{r:.2}"),
        r if r < 100.0 => format!("{r:.1}"),
        r => format!("{r:.0}"),
    }
}

fn fmt_eta(secs: f64) -> String {
    let s = secs.round() as u64;
    match s {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m{:02}s", s / 60, s % 60),
        s => format!("{}h{:02}m", s / 3600, (s % 3600) / 60),
    }
}

fn draw_chart(f: &mut Frame, app: &App, area: Rect) {
    let stats = app.view_stats();
    let now = stats.now().unwrap_or(0.0);
    let visible = |series: Vec<(f64, f64)>| -> Vec<(f64, f64)> {
        series.into_iter().filter(|&(t, _)| t >= now - app.view).map(|(t, r)| (t - now, r)).collect()
    };
    let line = visible(stats.avg_series(LINE_WINDOW));
    let avg = visible(stats.avg_series(AVG_WINDOW));

    let peak = line.iter().chain(&avg).map(|&(_, r)| r).fold(0.0, f64::max);
    let y_max = (peak * 1.15).max(1.0);

    let datasets = vec![
        Dataset::default()
            .name(format!("{}s avg", LINE_WINDOW as u64))
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Cyan))
            .data(&line),
        Dataset::default()
            .name(format!("{}s avg", AVG_WINDOW as u64))
            .marker(Marker::Braille)
            .graph_type(GraphType::Line)
            .style(Style::default().fg(Color::Yellow))
            .data(&avg),
    ];

    let v = app.view;
    let (name, pos, slots) = app.selection();
    let title = Line::from(vec![
        " immich jobs/s ".bold(),
        Span::raw("— "),
        Span::styled(name.to_string(), Style::default().fg(Color::Cyan).bold()),
        Span::styled(format!(" ({pos}/{slots}) "), Style::default().fg(Color::DarkGray)),
        Span::raw(format!("· {} ", app.target)),
        Span::styled(format!("[{}s window] ", v as u64), Style::default().fg(Color::DarkGray)),
    ]);
    let chart = Chart::new(datasets)
        .block(Block::bordered().title(title))
        .x_axis(
            Axis::default()
                .bounds([-v, 0.0])
                .labels([format!("-{}s", v as u64), format!("-{}s", (v / 2.0) as u64), "now".to_string()])
                .style(Style::default().fg(Color::DarkGray)),
        )
        .y_axis(
            Axis::default()
                .bounds([0.0, y_max])
                .labels(["0".to_string(), fmt_rate(y_max / 2.0), fmt_rate(y_max)])
                .style(Style::default().fg(Color::DarkGray)),
        );
    f.render_widget(chart, area);
}

fn rate_span(r: Option<Rate>) -> Span<'static> {
    match r {
        Some(Rate { per_sec, partial }) => {
            let txt = format!("{}{}/s", if partial { "~" } else { "" }, fmt_rate(per_sec));
            if partial { Span::styled(txt, Style::default().fg(Color::DarkGray)) } else { txt.bold() }
        }
        None => Span::styled("—", Style::default().fg(Color::DarkGray)),
    }
}

fn draw_stats(f: &mut Frame, app: &App, area: Rect) {
    let stats = app.view_stats();
    let label = |s: &str| Span::styled(format!("{s:<9}"), Style::default().fg(Color::Gray));
    let mut lines = Vec::new();

    let source = match app.mode {
        Some(Mode::Metrics) => Span::styled("metrics (exact)", Style::default().fg(Color::Green)),
        Some(Mode::Drain) => Span::styled("REST drain (low while enqueuing)", Style::default().fg(Color::Yellow)),
        None => Span::styled("waiting for data…", Style::default().fg(Color::DarkGray)),
    };
    lines.push(Line::from(vec![label("source"), source]));

    for pair in WINDOWS.chunks(2) {
        let mut spans = Vec::new();
        for (name, w) in pair {
            spans.push(label(&format!("last {name}")));
            let s = rate_span(stats.rate(*w));
            let pad = 13usize.saturating_sub(s.width());
            spans.push(s);
            spans.push(Span::raw(" ".repeat(pad)));
        }
        lines.push(Line::from(spans));
    }

    let now = stats.now().unwrap_or(0.0);
    let line_rates: Vec<f64> =
        stats.avg_series(LINE_WINDOW).into_iter().filter(|&(t, _)| t >= now - app.view).map(|(_, r)| r).collect();
    if !line_rates.is_empty() {
        let max = line_rates.iter().copied().fold(0.0, f64::max);
        let min = line_rates.iter().copied().fold(f64::INFINITY, f64::min);
        lines.push(Line::from(vec![
            label("min/max"),
            Span::raw(format!("{} / {} /s (5s avg, in view)", fmt_rate(min), fmt_rate(max))),
        ]));
    }

    lines.push(Line::from(vec![
        label("done"),
        Span::raw(format!("{}", stats.total_done())),
        Span::styled("  failed ", Style::default().fg(Color::Gray)),
        Span::styled(
            format!("{}", app.view_failed()),
            if app.view_failed() > 0 { Style::default().fg(Color::Red) } else { Style::default() },
        ),
    ]));

    if let Some(q) = app.view_queue() {
        let b = q.backlog;
        let mut spans = vec![
            label("queue"),
            Span::raw(format!("wait {} act {} dly {}", b.waiting, b.active, b.delayed)),
        ];
        if q.is_paused {
            spans.push(Span::styled(" PAUSED", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)));
        }
        lines.push(Line::from(spans));

        // ETA uses the viewed rate: only meaningful when the selected job feeds --queue.
        let rate = stats.rate(60.0).or_else(|| stats.rate(AVG_WINDOW));
        let eta = match rate {
            _ if b.pending() == 0 => "idle".to_string(),
            Some(r) if r.per_sec > 0.0 => fmt_eta(b.pending() as f64 / r.per_sec),
            _ => "∞".to_string(),
        };
        lines.push(Line::from(vec![label("ETA"), Span::raw(eta)]));
    }

    f.render_widget(Paragraph::new(lines).block(Block::bordered().title(format!(" stats: {} ", app.selection().0))), area);
}

fn draw_jobs(f: &mut Frame, app: &App, area: Rect) {
    let drain = app.mode == Some(Mode::Drain);
    // Name order (BTreeMap), same as cycle order, so `.` walks down the table.
    let rows: Vec<(&str, f64, u64)> = app
        .job_stats
        .iter()
        .map(|(name, s)| (name.as_str(), s.rate(60.0).map_or(0.0, |r| r.per_sec), s.total_done()))
        .collect();

    let block = Block::bordered().title(if drain { " per queue (1m) " } else { " per job (1m) " });
    if rows.is_empty() {
        let msg = if drain { "no busy queues yet" } else { "no jobs seen yet" };
        f.render_widget(Paragraph::new(msg).fg(Color::DarkGray).block(block), area);
        return;
    }

    let mut header = vec![if drain { "queue" } else { "job" }, "jobs/s", "done"];
    let mut widths = vec![Constraint::Min(16), Constraint::Length(8), Constraint::Length(9)];
    if drain {
        header.push("pending");
        widths.push(Constraint::Length(9));
    }
    let header = Row::new(header).style(Style::default().fg(Color::Gray).bold());
    let rows = rows.into_iter().map(|(name, rate, done)| {
        let style = if app.selected.as_deref() == Some(name) {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD | Modifier::REVERSED)
        } else if rate > 0.0 {
            Style::default()
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let mut cells = vec![Cell::from(name.to_string()), Cell::from(fmt_rate(rate)), Cell::from(done.to_string())];
        if drain {
            let pending = app.queues.get(name).map_or(0, |q| q.backlog.pending());
            cells.push(Cell::from(pending.to_string()));
        }
        Row::new(cells).style(style)
    });
    let table = Table::new(rows, widths).header(header).block(block);
    f.render_widget(table, area);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let age = app.last_poll.map_or("never".into(), |t| format!("{:.1}s ago", t.elapsed().as_secs_f64()));
    let mut spans = vec![Span::styled(
        format!(
            " poll {} every {:.1}s · , . job · a all · r reset · +/- zoom · q quit ",
            age,
            app.interval.as_secs_f64()
        ),
        Style::default().fg(Color::DarkGray),
    )];
    if let Some((at, msg)) = &app.flash
        && at.elapsed().as_secs_f64() < 3.0
    {
        spans.push(Span::styled(format!(" {msg}"), Style::default().fg(Color::Yellow)));
    } else if let Some(e) = app.errors.first() {
        spans.push(Span::styled(format!(" {e}"), Style::default().fg(Color::Red)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(fmt_rate(4.567), "4.57");
        assert_eq!(fmt_rate(42.42), "42.4");
        assert_eq!(fmt_rate(1234.5), "1234");
        assert_eq!(fmt_eta(42.0), "42s");
        assert_eq!(fmt_eta(125.0), "2m05s");
        assert_eq!(fmt_eta(3725.0), "1h02m");
    }
}
