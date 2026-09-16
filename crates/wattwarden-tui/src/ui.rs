use crate::app::{ActionItem, App};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use wattwarden_core::*;

const ASCII_LOGO: &[&str] = &[
    r#" __        ___  _____ _______        ___    ____  ____  _____ _   _ "#,
    r#" \ \      / / \|_   _|_   _\ \      / / \  |  _ \|  _ \| ____| \ | |"#,
    r#"  \ \ /\ / / _ \ | |   | |  \ \ /\ / / _ \ | |_) | | | |  _| |  \| |"#,
    r#"   \ V  V / ___ \| |   | |   \ V  V / ___ \|  _ <| |_| | |___| |\  |"#,
    r#"    \_/\_/_/   \_\_|   |_|    \_/\_/_/   \_\_| \_\____/|_____|_| \_|"#,
];

const BLOCKS: [char; 9] = [' ', ' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

pub fn draw(f: &mut Frame, app: &mut App) {
    let size = f.area();

    if size.width < 70 || size.height < 22 {
        let msg = Line::from(vec![
            Span::styled("PLEASE RESIZE TERMINAL", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(format!(" (Current: {}x{}, Minimum: 70x22)", size.width, size.height)),
        ]);
        let p = Paragraph::new(msg).block(Block::default().borders(Borders::ALL));
        f.render_widget(p, size);
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // ASCII Logo + Info Line
            Constraint::Min(12),   // Content (Adaptive Layout)
            Constraint::Length(3), // Footer / Toast
        ])
        .split(size);

    draw_header_banner(f, app, chunks[0]);
    draw_body(f, app, chunks[1]);
    draw_footer(f, app, chunks[2]);
}

fn draw_header_banner(f: &mut Frame, app: &App, area: Rect) {
    let mut lines = Vec::new();

    // Center ASCII Logo
    for logo_line in ASCII_LOGO {
        let pad = area.width.saturating_sub(68) / 2;
        let padded = format!("{:pad$}{}", "", logo_line, pad = pad as usize);
        lines.push(Line::from(Span::styled(
            padded,
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )));
    }

    // Status Info Line below logo
    let is_charging = app.backend.battery.is_charging().unwrap_or(false);
    let batt_pct = app.backend.battery.battery_percentage().unwrap_or(0);
    let watts = app.backend.battery.consumption_watts().unwrap_or(0.0);
    let est = app.backend.battery.time_remaining().unwrap_or_else(|_| "N/A".into());
    let current_profile = &app.config.profile;

    let status_str = if is_charging { "Charging (AC)" } else { "Discharging" };

    let summary_line = Line::from(vec![
        Span::styled("OS: ", Style::default().fg(Color::DarkGray)),
        Span::styled("Linux", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        Span::raw(" | "),
        Span::styled("Profile: ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{}", current_profile), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(" | "),
        Span::styled("Battery: ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{}% ({})", batt_pct, status_str), Style::default().fg(if batt_pct < 20 { Color::Red } else { Color::Green }).add_modifier(Modifier::BOLD)),
        Span::raw(" | "),
        Span::styled("Est: ", Style::default().fg(Color::DarkGray)),
        Span::styled(est, Style::default().fg(Color::LightBlue)),
        Span::raw(" | "),
        Span::styled("Power: ", Style::default().fg(Color::DarkGray)),
        Span::styled(format!("{:.1} W", watts), Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
    ]);

    lines.push(Line::from(""));
    lines.push(summary_line);

    let p = Paragraph::new(lines).alignment(ratatui::layout::Alignment::Center);
    f.render_widget(p, area);
}

fn draw_body(f: &mut Frame, app: &mut App, area: Rect) {
    let is_horizontal = area.width >= 120;

    if is_horizontal {
        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(48), Constraint::Percentage(52)])
            .split(area);

        draw_menu_list(f, app, h_chunks[0]);
        draw_graph_and_telemetry(f, app, h_chunks[1]);
    } else {
        let v_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(10), Constraint::Min(8)])
            .split(area);

        draw_ascii_bar_graph(f, app, v_chunks[0]);
        draw_menu_list(f, app, v_chunks[1]);
    }
}

fn draw_menu_list(f: &mut Frame, app: &App, area: Rect) {
    let mut lines = Vec::new();
    let width = area.width as usize;

    for (idx, item) in app.items.iter().enumerate() {
        if let ActionItem::Header(header_title) = item {
            lines.push(Line::from(Span::styled(
                format!(" {}", header_title),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )));
            continue;
        }

        let is_selected = idx == app.selected;

        let (name, val_str, is_active_profile) = match item {
            ActionItem::Header(_) => unreachable!(),
            ActionItem::ProfilePerformance => ("High Performance Profile", format!("[{}]", if app.config.profile == PowerProfile::Performance { "ACTIVE" } else { "OFF" }), app.config.profile == PowerProfile::Performance),
            ActionItem::ProfileExtreme => ("Extreme Battery Saver", format!("[{}]", if app.config.profile == PowerProfile::Extreme { "ACTIVE" } else { "OFF" }), app.config.profile == PowerProfile::Extreme),
            ActionItem::ProfileAutoExtreme => ("Auto Extreme Mode (Dynamic)", format!("[{}]", if app.config.profile == PowerProfile::AutoExtreme { "ACTIVE" } else { "OFF" }), app.config.profile == PowerProfile::AutoExtreme),
            ActionItem::ProfileRestore => ("Restore Factory Defaults", format!("[{}]", if app.config.profile == PowerProfile::Normal { "ACTIVE" } else { "OFF" }), app.config.profile == PowerProfile::Normal),
            ActionItem::Cores => {
                let online = app.backend.cpu.online_cores().unwrap_or(1);
                let total = app.backend.cpu.num_cpus();
                ("CPU Cores Active", format!("[{}/{}]", online, total), false)
            }
            ActionItem::FreqLimit => {
                let freq = app.backend.cpu.freq_limit().unwrap_or(0);
                ("Max CPU Frequency", format!("[{} MHz]", freq), false)
            }
            ActionItem::RaplPl1 => {
                let pl1 = app.backend.rapl.as_ref().and_then(|r| r.pl1_watts().ok()).unwrap_or(0);
                ("Intel RAPL PL1 Limit", format!("[{} W]", pl1), false)
            }
            ActionItem::RaplPl2 => {
                let pl2 = app.backend.rapl.as_ref().and_then(|r| r.pl2_watts().ok()).unwrap_or(0);
                ("Intel RAPL PL2 Boost", format!("[{} W]", pl2), false)
            }
            ActionItem::Turbo => {
                let t = app.backend.cpu.turbo_enabled().unwrap_or(false);
                ("CPU Turbo / Boost", format!("[{}]", if t { "ON" } else { "OFF" }), false)
            }
            ActionItem::Epp => {
                let epp = app.backend.cpu.energy_performance_preference().unwrap_or_else(|_| "N/A".into());
                ("Energy Perf Preference (EPP)", format!("[{}]", epp), false)
            }
            ActionItem::Brightness => {
                let b = app.backend.backlight.as_ref().and_then(|bl| bl.brightness_percent().ok()).unwrap_or(0);
                ("Display Backlight Brightness", format!("[{}%]", b), false)
            }
            ActionItem::ChargeLimit => {
                let t = app.backend.threshold.charge_threshold().unwrap_or(80);
                ("BMS Battery Charge Ceiling", format!("[{}%]", t), false)
            }
            ActionItem::AutoBrightness => {
                ("Auto-Brightness (Hyprland IPC)", format!("[{}]", if app.config.auto_brightness { "ACTIVE" } else { "OFF" }), false)
            }
            ActionItem::DropCaches => {
                ("Drop VM Memory Caches", "[CLEAN]".into(), false)
            }
        };

        let max_name_len = width.saturating_sub(22).max(10);
        let truncated_name = if name.len() > max_name_len {
            format!("{}...", &name[..max_name_len.saturating_sub(3)])
        } else {
            name.to_string()
        };

        if is_selected {
            let row_text = format!(" > {:<width$} {:>14} ", truncated_name, val_str, width = max_name_len);
            lines.push(Line::from(Span::styled(
                row_text,
                Style::default().fg(Color::Black).bg(Color::White).add_modifier(Modifier::BOLD),
            )));
        } else if is_active_profile {
            let row_text = format!(" ▸ {:<width$} {:>14} ", truncated_name, val_str, width = max_name_len);
            lines.push(Line::from(Span::styled(
                row_text,
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            )));
        } else {
            let row_text = format!("   {:<width$} {:>14} ", truncated_name, val_str, width = max_name_len);
            lines.push(Line::from(Span::styled(
                row_text,
                Style::default().fg(Color::White),
            )));
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Hardware Controls ")
        .style(Style::default().fg(Color::Cyan));

    let p = Paragraph::new(lines).block(block);
    f.render_widget(p, area);
}

fn draw_graph_and_telemetry(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(10), Constraint::Length(6)])
        .split(area);

    draw_ascii_bar_graph(f, app, chunks[0]);
    draw_cstates_panel(f, app, chunks[1]);
}

fn draw_ascii_bar_graph(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Real-Time Power Drain History (Watts) ")
        .style(Style::default().fg(Color::Green));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width < 18 || inner.height < 3 {
        return;
    }

    let graph_max = 15.0f64;
    let height = inner.height as usize;
    let width = inner.width as usize;
    let plot_width = width.saturating_sub(12);

    let mut lines = Vec::with_capacity(height);
    let step_val = if height > 1 { graph_max / (height - 1) as f64 } else { graph_max };

    for row in 0..height {
        let y_level = height - 1 - row;
        let val_label = (y_level as f64) * step_val;

        let mut row_spans = Vec::new();
        row_spans.push(Span::styled("│", Style::default().fg(Color::DarkGray)));

        for col in 0..plot_width {
            let hist_idx = if app.history.len() > plot_width {
                app.history.len() - plot_width + col
            } else if col < plot_width - app.history.len() {
                usize::MAX
            } else {
                col - (plot_width - app.history.len())
            };

            let val = if hist_idx < app.history.len() {
                app.history[hist_idx]
            } else {
                0.0
            };

            let ratio = (val / graph_max).clamp(0.0, 1.0);
            let total_dots = (ratio * (height * 8) as f64) as usize;
            let dots_in_row = total_dots.saturating_sub(y_level * 8);
            let char_idx = dots_in_row.min(8);

            let block_char = BLOCKS[char_idx];
            let color = if y_level >= height.saturating_sub(2) {
                Color::Red
            } else if y_level >= height / 2 {
                Color::Yellow
            } else {
                Color::Green
            };

            row_spans.push(Span::styled(block_char.to_string(), Style::default().fg(color)));
        }

        let label_color = if val_label >= 12.0 {
            Color::Red
        } else if val_label >= 6.0 {
            Color::Yellow
        } else {
            Color::Green
        };

        row_spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        row_spans.push(Span::styled(format!("{:4.1} W", val_label), Style::default().fg(label_color)));

        lines.push(Line::from(row_spans));
    }

    let p = Paragraph::new(lines);
    f.render_widget(p, inner);
}

fn draw_cstates_panel(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" CPU Package C-State Residency ")
        .style(Style::default().fg(Color::Yellow));

    let cstates = app.backend.cpu.cstates().unwrap_or_default();
    let mut lines = Vec::new();

    if cstates.is_empty() {
        lines.push(Line::from(Span::styled("No cpuidle C-state telemetry found", Style::default().fg(Color::DarkGray))));
    } else {
        let mut row_spans = Vec::new();
        for (i, state) in cstates.iter().take(6).enumerate() {
            let time_ms = state.time_microseconds / 1000;
            row_spans.push(Span::styled(format!(" {:<4}:", state.name), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)));
            row_spans.push(Span::styled(format!(" {:>6}ms ", time_ms), Style::default().fg(Color::LightGreen)));
            if (i + 1) % 3 == 0 {
                lines.push(Line::from(row_spans));
                row_spans = Vec::new();
            }
        }
        if !row_spans.is_empty() {
            lines.push(Line::from(row_spans));
        }
    }

    let p = Paragraph::new(lines).block(block);
    f.render_widget(p, area);
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let footer_line = if let Some(toast) = app.toast_message() {
        Line::from(vec![
            Span::styled(" 🔔 ", Style::default().fg(Color::Yellow)),
            Span::styled(toast, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
        ])
    } else {
        Line::from(vec![
            Span::styled(" [W/S, Up/Down] ", Style::default().fg(Color::Cyan)),
            Span::raw("Navigate   "),
            Span::styled(" [A/D, Left/Right] ", Style::default().fg(Color::Cyan)),
            Span::raw("Adjust Limits   "),
            Span::styled(" [Enter] ", Style::default().fg(Color::Cyan)),
            Span::raw("Toggle/Apply   "),
            Span::styled(" [Q / Esc] ", Style::default().fg(Color::Red)),
            Span::raw("Quit"),
        ])
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::DarkGray));

    let p = Paragraph::new(footer_line).block(block);
    f.render_widget(p, area);
}
