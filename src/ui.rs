use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{App, Focus, PRESETS};

pub fn draw(frame: &mut Frame, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(3),
            Constraint::Length(6),
            Constraint::Length(2),
        ])
        .split(frame.area());

    draw_header(frame, root[0], app);
    draw_main(frame, root[1], app);
    draw_hex(frame, root[2], app);
    draw_status(frame, root[3], app);
    draw_help(frame, root[4]);
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let sudo = match app.config.use_sudo {
        crate::config::SudoMode::Always => "sudo=always",
        crate::config::SudoMode::Never => "sudo=never",
        crate::config::SudoMode::Auto => "sudo=auto",
    };
    let mode = app.config.mode.as_str();
    let title = Paragraph::new(Line::from(vec![
        Span::styled(
            " colorfan ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(255, 80, 60))
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!(
                "Device {}: {}",
                app.device().index,
                app.device().name
            ),
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(
            format!("mode={mode}  {sudo}"),
            Style::default().fg(Color::DarkGray),
        ),
    ]))
    .block(Block::default().borders(Borders::ALL).title("OpenRGB Aura"));
    frame.render_widget(title, area);
}

fn draw_main(frame: &mut Frame, area: Rect, app: &App) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    draw_zones(frame, cols[0], app);
    draw_presets(frame, cols[1], app);
}

fn focus_block<'a>(title: &'a str, focused: bool) -> Block<'a> {
    let style = if focused {
        Style::default()
            .fg(Color::Rgb(255, 80, 60))
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    Block::default()
        .borders(Borders::ALL)
        .border_style(style)
        .title(title)
}

fn draw_zones(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::Zones;
    let mut items = Vec::new();
    for i in 0..app.zone_count() {
        let label = app.zone_label(i);
        let selected = i == app.zone_sel;
        let marker = if selected { "▸ " } else { "  " };
        let style = if selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        items.push(ListItem::new(Line::from(Span::styled(
            format!("{marker}{label}"),
            style,
        ))));
    }
    let list = List::new(items).block(focus_block("Zones  (a = all)", focused));
    frame.render_widget(list, area);
}

fn draw_presets(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::Presets;
    let mut items = Vec::new();
    for (i, p) in PRESETS.iter().enumerate() {
        let selected = i == app.preset_sel;
        let marker = if selected { "▸ " } else { "  " };
        let swatch = color_from_hex(p.hex).unwrap_or(Color::Gray);
        let style = if selected {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        items.push(ListItem::new(Line::from(vec![
            Span::styled(format!("{marker}{} ", i + 1), style.fg(Color::DarkGray)),
            Span::styled("██ ", Style::default().fg(swatch)),
            Span::styled(format!("{}  #{}", p.name, p.hex), style.fg(Color::White)),
        ])));
    }
    let list = List::new(items).block(focus_block("Presets  (1–8 apply)", focused));
    frame.render_widget(list, area);
}

fn draw_hex(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::Hex;
    let swatch = color_from_hex(&app.color_hex).unwrap_or(Color::Gray);
    let cursor = if focused { "▌" } else { "" };
    let line = Line::from(vec![
        Span::raw("  "),
        Span::styled("████  ", Style::default().fg(swatch)),
        Span::styled(
            format!("#{}{cursor}", app.hex_input.trim_start_matches('#')),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("   active "),
        Span::styled(
            format!("#{}", app.color_hex),
            Style::default().fg(swatch),
        ),
    ]);
    let p = Paragraph::new(line).block(focus_block("Hex  (type RRGGBB, Enter apply)", focused));
    frame.render_widget(p, area);
}

fn draw_status(frame: &mut Frame, area: Rect, app: &App) {
    let ok_style = match app.last_ok {
        Some(true) => Style::default().fg(Color::Green),
        Some(false) => Style::default().fg(Color::Red),
        None => Style::default().fg(Color::Gray),
    };
    let mut lines = vec![Line::from(vec![
        Span::styled("cmd  ", Style::default().fg(Color::DarkGray)),
        Span::raw(if app.last_command.is_empty() {
            "—".into()
        } else {
            app.last_command.clone()
        }),
    ])];
    for (i, part) in app.last_status.lines().take(3).enumerate() {
        if i == 0 {
            lines.push(Line::from(vec![
                Span::styled("out  ", ok_style),
                Span::styled(part.to_string(), ok_style),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                format!("     {part}"),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    if !app.status_line.is_empty() {
        lines.push(Line::from(Span::styled(
            format!(" · {}", app.status_line),
            Style::default().fg(Color::Cyan),
        )));
    }
    let p = Paragraph::new(lines)
        .wrap(Wrap { trim: true })
        .block(Block::default().borders(Borders::ALL).title("Last run"));
    frame.render_widget(p, area);
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let help = Paragraph::new(Line::from(vec![
        Span::styled(" q ", Style::default().fg(Color::Yellow)),
        Span::raw("quit  "),
        Span::styled(" Tab/←→/hl ", Style::default().fg(Color::Yellow)),
        Span::raw("focus  "),
        Span::styled(" ↑↓/jk ", Style::default().fg(Color::Yellow)),
        Span::raw("move  "),
        Span::styled(" Enter ", Style::default().fg(Color::Yellow)),
        Span::raw("apply  "),
        Span::styled(" 1-8 ", Style::default().fg(Color::Yellow)),
        Span::raw("preset+apply  "),
        Span::styled(" r ", Style::default().fg(Color::Yellow)),
        Span::raw("reload  "),
        Span::styled(" a ", Style::default().fg(Color::Yellow)),
        Span::raw("all zones  "),
        Span::styled(" p ", Style::default().fg(Color::Yellow)),
        Span::raw("solid-red profile"),
    ]));
    frame.render_widget(help, area);
}

fn color_from_hex(hex: &str) -> Option<Color> {
    let s = hex.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Color::Rgb(r, g, b))
}
