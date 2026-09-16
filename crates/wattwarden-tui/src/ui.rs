use crate::app::{App, MenuItem};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Sparkline},
    Frame,
};
use wattwarden_core::*;

pub fn draw(f: &mut Frame, app: &mut App) {
    let size = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Top Banner
            Constraint::Min(10),   // Content
            Constraint::Length(3), // Footer / Toast
        ])
        .split(size);

    draw_header(f, app, chunks[0]);
    draw_content(f, app, chunks[1]);
    draw_footer(f, app, chunks[2]);
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let battery_pct = app.backend.battery.battery_percentage().unwrap_or(0);
    let is_charging = app.backend.battery.is_charging().unwrap_or(false);
    let watts = app.backend.battery.consumption_watts().unwrap_or(0.0);
    let time_left = app.backend.battery.time_remaining().unwrap_or_else(|_| "N/A".into());
    let active_profile = &app.config.profile;

    let charging_indicator = if is_charging {
        Span::styled(" [AC CHARGING] ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
    } else {
        Span::styled(" [ON BATTERY] ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
    };

    let title_line = Line::from(vec![
        Span::styled(" ⚡ WattWarden ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        charging_indicator,
        Span::raw(" | Battery: "),
        Span::styled(format!("{}%", battery_pct), Style::default().fg(if battery_pct < 20 { Color::Red } else { Color::Green }).add_modifier(Modifier::BOLD)),
        Span::raw(" | Drain: "),
        Span::styled(format!("{:.2} W", watts), Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        Span::raw(" | Remaining: "),
        Span::styled(time_left, Style::default().fg(Color::LightBlue)),
        Span::raw(" | Profile: "),
        Span::styled(format!("[{}]", active_profile), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::DarkGray));

    let paragraph = Paragraph::new(title_line).block(block);
    f.render_widget(paragraph, area);
}

fn draw_content(f: &mut Frame, app: &mut App, area: Rect) {
    let horizontal_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    draw_menu(f, app, horizontal_chunks[0]);
    draw_telemetry(f, app, horizontal_chunks[1]);
}

fn draw_menu(f: &mut Frame, app: &App, area: Rect) {
    let mut list_items = Vec::new();

    for (idx, item) in app.menu_items.iter().enumerate() {
        let is_selected = idx == app.selected_menu;

        let (label, val_str) = match item {
            MenuItem::Profile => ("Active Profile", format!("{}", app.config.profile)),
            MenuItem::Cores => {
                let online = app.backend.cpu.online_cores().unwrap_or(1);
                let total = app.backend.cpu.num_cpus();
                ("Online CPU Cores", format!("{}/{}", online, total))
            }
            MenuItem::FreqLimit => {
                let freq = app.backend.cpu.freq_limit().unwrap_or(0);
                ("Max CPU Frequency", format!("{} MHz", freq))
            }
            MenuItem::Brightness => {
                let b = app.backend.backlight.as_ref().and_then(|bl| bl.brightness_percent().ok()).unwrap_or(0);
                ("Display Brightness", format!("{}%", b))
            }
            MenuItem::ChargeLimit => {
                let t = app.backend.threshold.charge_threshold().unwrap_or(80);
                ("BMS Charge Ceiling", format!("{}%", t))
            }
            MenuItem::Turbo => {
                let t = app.backend.cpu.turbo_enabled().unwrap_or(false);
                ("CPU Turbo Boost", format!("{}", if t { "ON" } else { "OFF" }))
            }
            MenuItem::Epp => {
                let epp = app.backend.cpu.energy_performance_preference().unwrap_or_else(|_| "N/A".into());
                ("Energy Perf Pref (EPP)", epp)
            }
            MenuItem::RaplPl1 => {
                let pl1 = app.backend.rapl.as_ref().and_then(|r| r.pl1_watts().ok()).unwrap_or(0);
                ("Intel RAPL PL1 Limit", format!("{} W", pl1))
            }
        };

        let style = if is_selected {
            Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let line = Line::from(vec![
            Span::styled(format!("  {:<24} : ", label), style),
            Span::styled(format!("{:<15}", val_str), style.add_modifier(Modifier::BOLD)),
        ]);

        list_items.push(ListItem::new(line));
    }

    let menu_block = Block::default()
        .borders(Borders::ALL)
        .title(" Hardware Controls (Enter: Toggle | Left/Right: Adjust) ")
        .style(Style::default().fg(Color::Cyan));

    let list = List::new(list_items).block(menu_block);
    f.render_widget(list, area);
}

fn draw_telemetry(f: &mut Frame, app: &App, area: Rect) {
    let telemetry_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
        .split(area);

    // 1. Power drain sparkline
    let sparkline_block = Block::default()
        .borders(Borders::ALL)
        .title(" Power Drain Real-Time History (Watts) ")
        .style(Style::default().fg(Color::Green));

    let sparkline = Sparkline::default()
        .block(sparkline_block)
        .data(&app.power_history)
        .style(Style::default().fg(Color::Green));

    f.render_widget(sparkline, telemetry_chunks[0]);

    // 2. C-State residency table
    let cstates = app.backend.cpu.cstates().unwrap_or_default();
    let mut cstate_lines = Vec::new();
    if cstates.is_empty() {
        cstate_lines.push(Line::from(Span::styled("No cpuidle C-state telemetry detected", Style::default().fg(Color::DarkGray))));
    } else {
        for state in cstates.iter().take(6) {
            let time_ms = state.time_microseconds / 1000;
            cstate_lines.push(Line::from(vec![
                Span::styled(format!(" {:<8}", state.name), Style::default().fg(Color::Yellow)),
                Span::raw(" | Time: "),
                Span::styled(format!("{:>8} ms", time_ms), Style::default().fg(Color::LightGreen)),
                Span::raw(" | Transitions: "),
                Span::styled(format!("{:>6}", state.usage_count), Style::default().fg(Color::LightBlue)),
            ]));
        }
    }

    let cstate_block = Block::default()
        .borders(Borders::ALL)
        .title(" CPU Package C-State Residency ")
        .style(Style::default().fg(Color::Yellow));

    let cstate_p = Paragraph::new(cstate_lines).block(cstate_block);
    f.render_widget(cstate_p, telemetry_chunks[1]);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let footer_text = if let Some(toast) = app.toast_message() {
        Line::from(vec![
            Span::styled(" 🔔 ", Style::default().fg(Color::Yellow)),
            Span::styled(toast, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" [W/S, Up/Down] ", Style::default().fg(Color::DarkGray)),
            Span::raw("Select   "),
            Span::styled(" [A/D, Left/Right] ", Style::default().fg(Color::DarkGray)),
            Span::raw("Adjust   "),
            Span::styled(" [Enter] ", Style::default().fg(Color::DarkGray)),
            Span::raw("Toggle   "),
            Span::styled(" [Q / Esc] ", Style::default().fg(Color::DarkGray)),
            Span::raw("Quit"),
        ])
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::DarkGray));

    let paragraph = Paragraph::new(footer_text).block(block);
    f.render_widget(paragraph, area);
}
