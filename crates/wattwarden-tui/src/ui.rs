use crate::app::{ActionItem, App};
use ratatui::{
    buffer::Buffer,
    style::{Color, Modifier, Style},
    Frame,
};
use std::time::Duration;
use wattwarden_core::*;

const ASCII_LOGO: &[&str] = &[
    r#" __        ___  _____ _______        ___    ____  ____  _____ _   _ "#,
    r#" \ \      / / \|_   _|_   _\ \      / / \  |  _ \|  _ \| ____| \ | |"#,
    r#"  \ \ /\ / / _ \ | |   | |  \ \ /\ / / _ \ | |_) | | | |  _| |  \| |"#,
    r#"   \ V  V / ___ \| |   | |   \ V  V / ___ \|  _ <| |_| | |___| |\  |"#,
    r#"    \_/\_/_/   \_\_|   |_|    \_/\_/_/   \_\_| \_\____/|_____|_| \_|"#,
];

const BLOCKS: [char; 9] = [' ', ' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

fn set_str(buf: &mut Buffer, x: usize, y: usize, text: &str, style: Style) {
    let w = buf.area.width as usize;
    let h = buf.area.height as usize;
    if x >= w || y >= h {
        return;
    }
    buf.set_string(x as u16, y as u16, text, style);
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let w = area.width as usize;
    let h = area.height as usize;

    if w < 70 || h < 20 {
        let msg = "PLEASE RESIZE TERMINAL (Minimum size: 70x20)";
        let msg_x = (w.saturating_sub(msg.len())) / 2;
        let msg_y = h / 2;
        let style = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
        set_str(f.buffer_mut(), msg_x, msg_y, msg, style);
        return;
    }

    let buf = f.buffer_mut();

    // 1. Draw Centered ASCII Banner
    let art_y = 1;
    let art_x = (w.saturating_sub(68)) / 2;
    let title_style = Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD);

    for (i, line) in ASCII_LOGO.iter().enumerate() {
        set_str(buf, art_x, art_y + i, line, title_style);
    }

    // 2. Centered Status Summary Line
    let is_charging = app.backend.battery.is_charging().unwrap_or(false);
    let batt_pct = app.backend.battery.battery_percentage().unwrap_or(0);
    let watts = app.backend.battery.consumption_watts().unwrap_or(0.0);
    let est = app.backend.battery.time_remaining().unwrap_or_else(|_| "N/A".into());
    let status_str = if is_charging { "Charging" } else { "Discharging" };

    let summary = format!(
        "OS: Linux | Battery: {}% ({}) | Est: {} | Power: {:.1}W",
        batt_pct, status_str, est, watts
    );
    let info_y = art_y + ASCII_LOGO.len() + 1;
    let info_x = (w.saturating_sub(summary.len())) / 2;
    set_str(buf, info_x, info_y, &summary, Style::default().add_modifier(Modifier::BOLD));

    // 3. Layout Dimensions Calculation
    let is_horizontal = w >= 130;
    let (menu_x, menu_y, menu_w, graph_x, graph_y, graph_w, graph_h) = if is_horizontal {
        let mx = 4;
        let my = info_y + 2;
        let mw = 52;
        let gx = mx + mw + 4;
        let gy = info_y + 2;
        let gw = w.saturating_sub(gx + 4);
        let gh = h.saturating_sub(gy + 9).max(6);
        (mx, my, mw, gx, gy, gw, gh)
    } else {
        let gx = 4;
        let gy = info_y + 2;
        let gw = w.saturating_sub(8);
        let needed_for_menu = app.items.len();
        let available = h.saturating_sub(gy + 4);
        let gh = if available > needed_for_menu + 10 {
            (available - needed_for_menu).min(12)
        } else {
            8
        };
        let mx = 4;
        let my = gy + gh + 2;
        let mw = w.saturating_sub(8);
        (mx, my, mw, gx, gy, gw, gh)
    };

    // 4. Draw Real-Time Bar Graph
    draw_bar_graph(buf, graph_x, graph_y, graph_w, graph_h, &app.history, 15.0);

    // 5. Draw C-States Telemetry (in wide mode below graph)
    if is_horizontal {
        draw_cstates(buf, graph_x, graph_y + graph_h + 2, graph_w, app);
    }

    // 6. Draw Menu with Viewport Scrolling and Scrollbar
    draw_menu(buf, app, menu_x, menu_y, menu_w, h);

    // 7. Draw Toast Popup (if active)
    if let Some((msg, time)) = &app.toast {
        if time.elapsed() < Duration::from_secs(3) {
            let toast_str = format!(" {} ", msg);
            let toast_x = (w.saturating_sub(toast_str.len())) / 2;
            let toast_y = h.saturating_sub(3);
            set_str(
                buf,
                toast_x,
                toast_y,
                &toast_str,
                Style::default().bg(Color::Yellow).fg(Color::Black).add_modifier(Modifier::BOLD),
            );
        }
    }

    // 8. Draw Extreme Mode Confirmation Modal
    if app.confirm_extreme {
        draw_extreme_modal(buf, w, h);
    }

    // 9. Draw Footer Controls Help
    let footer_text = "[UP/DOWN] Navigate | [L/R] Adjust | [ENTER] Apply | [R] Restore | [Q] Quit";
    set_str(buf, 2, h.saturating_sub(1), footer_text, Style::default().fg(Color::DarkGray));
}

fn draw_bar_graph(
    buf: &mut Buffer,
    start_x: usize,
    start_y: usize,
    width: usize,
    height: usize,
    data_list: &[f64],
    graph_max: f64,
) {
    if width < 20 || height < 3 {
        return;
    }

    let plot_w = width.saturating_sub(14);

    // Draw left axis
    for y_offset in 0..height {
        let screen_y = start_y + height - 1 - y_offset;
        set_str(buf, start_x, screen_y, "│", Style::default().fg(Color::DarkGray));
    }
    // Draw bottom axis
    let bottom_line = format!("└{}", "─".repeat(plot_w));
    set_str(buf, start_x, start_y + height, &bottom_line, Style::default().fg(Color::DarkGray));

    // Plot historical data columns
    for i in 0..plot_w {
        let data_idx = if data_list.len() > plot_w {
            data_list.len() - plot_w + i
        } else if i < plot_w - data_list.len() {
            continue;
        } else {
            i - (plot_w - data_list.len())
        };

        if data_idx >= data_list.len() {
            continue;
        }

        let val = data_list[data_idx];
        let x_pos = start_x + 1 + i;
        let ratio = (val / graph_max).clamp(0.0, 1.0);
        let total_dots = (ratio * (height * 8) as f64) as usize;

        for y_offset in 0..height {
            let screen_y = start_y + height - 1 - y_offset;
            let dots_in_row = total_dots.saturating_sub(y_offset * 8);
            let char_idx = if dots_in_row >= 8 {
                8
            } else {
                dots_in_row
            };

            if char_idx > 0 {
                let color = if y_offset >= height.saturating_sub(2) {
                    Color::Red
                } else if y_offset >= height / 2 {
                    Color::Yellow
                } else {
                    Color::Green
                };
                let ch = BLOCKS[char_idx];
                if let Some(cell) = buf.cell_mut((x_pos as u16, screen_y as u16)) {
                    cell.set_char(ch).set_fg(color);
                }
            }
        }
    }

    // Right axis and wattage scale labels
    let step_val = if height > 1 {
        graph_max / (height - 1) as f64
    } else {
        1.0
    };

    for y_offset in 0..height {
        let screen_y = start_y + height - 1 - y_offset;
        let val_label = (y_offset as f64) * step_val;
        let color = if val_label >= 12.0 {
            Color::Red
        } else if val_label >= 6.0 {
            Color::Yellow
        } else {
            Color::Green
        };

        set_str(buf, start_x + width - 12, screen_y, "│ ", Style::default().fg(Color::DarkGray));
        set_str(buf, start_x + width - 10, screen_y, &format!("{:4.1} W", val_label), Style::default().fg(color));
    }
}

fn draw_cstates(buf: &mut Buffer, x: usize, y: usize, width: usize, app: &App) {
    let header_line = format!("─── [ CPU C-STATE RESIDENCY ] {}", "─".repeat(width.saturating_sub(32)));
    set_str(buf, x, y, &header_line, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

    let cstates = app.backend.cpu.cstates().unwrap_or_default();
    if cstates.is_empty() {
        set_str(buf, x + 2, y + 2, "No cpuidle C-state telemetry detected", Style::default().fg(Color::DarkGray));
        return;
    }

    let mut col_offset = 0;
    let mut row_offset = 2;

    for (i, state) in cstates.iter().take(8).enumerate() {
        let time_ms = state.time_microseconds / 1000;
        let item_str = format!("{:<4}: {:>7}ms", state.name, time_ms);
        set_str(buf, x + col_offset, y + row_offset, &item_str, Style::default().fg(Color::LightGreen));

        col_offset += 18;
        if (i + 1) % 3 == 0 {
            col_offset = 0;
            row_offset += 1;
        }
    }
}

fn get_item_info(item: &ActionItem, app: &App) -> (&'static str, String, bool) {
    match item {
        ActionItem::Header(_) => ("", "".into(), false),
        ActionItem::ProfilePerformance => (
            "⚡ Performance Mode",
            if app.config.profile == PowerProfile::Performance { "[ACTIVE]".into() } else { "[EXECUTE]".into() },
            app.config.profile == PowerProfile::Performance,
        ),
        ActionItem::ProfileExtreme => (
            "🔋 Extreme Mode",
            if app.config.profile == PowerProfile::Extreme { "[ACTIVE]".into() } else { "[EXECUTE]".into() },
            app.config.profile == PowerProfile::Extreme,
        ),
        ActionItem::ProfileAutoExtreme => (
            "⚡ Auto Extreme Mode",
            if app.config.profile == PowerProfile::AutoExtreme { "[ACTIVE]".into() } else { "[OFF]".into() },
            app.config.profile == PowerProfile::AutoExtreme,
        ),
        ActionItem::ProfileRestore => (
            "🔄 Restore Defaults",
            "[EXECUTE]".into(),
            false,
        ),
        ActionItem::Cores => {
            let online = app.backend.cpu.online_cores().unwrap_or(1);
            let total = app.backend.cpu.num_cpus();
            ("⚙️  CPU Cores Active", format!("[{}/{}]", online, total), false)
        }
        ActionItem::FreqLimit => {
            let freq = app.backend.cpu.freq_limit().unwrap_or(0);
            ("📈 Max CPU Frequency", format!("[{} MHz]", freq), false)
        }
        ActionItem::RaplPl1 => {
            let pl1 = app.backend.rapl.as_ref().and_then(|r| r.pl1_watts().ok()).unwrap_or(0);
            ("⚡ Intel RAPL PL1 Limit", format!("[{} W]", pl1), false)
        }
        ActionItem::RaplPl2 => {
            let pl2 = app.backend.rapl.as_ref().and_then(|r| r.pl2_watts().ok()).unwrap_or(0);
            ("🚀 Intel RAPL PL2 Boost", format!("[{} W]", pl2), false)
        }
        ActionItem::Turbo => {
            let t = app.backend.cpu.turbo_enabled().unwrap_or(false);
            ("🔥 CPU Turbo / Boost", format!("[{}]", if t { "ON" } else { "OFF" }), t)
        }
        ActionItem::Epp => {
            let epp = app.backend.cpu.energy_performance_preference().unwrap_or_else(|_| "N/A".into());
            ("🎯 Energy Perf Preference (EPP)", format!("[{}]", epp), false)
        }
        ActionItem::Brightness => {
            let b = app.backend.backlight.as_ref().and_then(|bl| bl.brightness_percent().ok()).unwrap_or(0);
            ("💡 Display Backlight Brightness", format!("[{}%]", b), false)
        }
        ActionItem::ChargeLimit => {
            let t = app.backend.threshold.charge_threshold().unwrap_or(80);
            ("🛡️  BMS Battery Charge Ceiling", format!("[{}%]", t), false)
        }
        ActionItem::AutoBrightness => {
            let active = app.config.auto_brightness;
            ("🤖 Auto-Brightness (Hyprland IPC)", format!("[{}]", if active { "ACTIVE" } else { "OFF" }), active)
        }
        ActionItem::DropCaches => {
            ("🧹 Drop VM Memory Caches", "[CLEAN]".into(), false)
        }
    }
}

fn draw_menu(
    buf: &mut Buffer,
    app: &mut App,
    menu_x: usize,
    menu_y: usize,
    menu_w: usize,
    h: usize,
) {
    let visible_items = h.saturating_sub(menu_y + 4).max(1);

    // Dynamic scroll bounds
    if app.selected < app.scroll_offset {
        app.scroll_offset = app.selected;
    }
    if app.selected >= app.scroll_offset + visible_items {
        app.scroll_offset = app.selected - visible_items + 1;
    }

    // Scroll up indicator
    if app.scroll_offset > 0 {
        let msg = " ▲ SCROLL UP FOR MORE OPTIONS ▲ ";
        let msg_x = menu_x + (menu_w.saturating_sub(msg.len())) / 2;
        set_str(buf, msg_x, menu_y.saturating_sub(1), msg, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    }

    for i in 0..visible_items {
        let idx = app.scroll_offset + i;
        if idx >= app.items.len() {
            break;
        }

        let item = &app.items[idx];
        let y = menu_y + i;

        if let ActionItem::Header(header_title) = item {
            set_str(buf, menu_x, y, header_title, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
            continue;
        }

        let (name, val_str, is_active) = get_item_info(item, app);
        let max_name_len = menu_w.saturating_sub(20).max(5);
        let truncated_name = if name.chars().count() > max_name_len {
            format!("{}...", name.chars().take(max_name_len.saturating_sub(3)).collect::<String>())
        } else {
            name.to_string()
        };

        let is_selected = idx == app.selected;
        let line_text = format!(" > {:<width$} {:>15} ", truncated_name, val_str, width = max_name_len);
        let normal_text = format!("   {:<width$} {:>15} ", truncated_name, val_str, width = max_name_len);

        if is_selected {
            set_str(
                buf,
                menu_x,
                y,
                &line_text,
                Style::default().fg(Color::Blue).bg(Color::White).add_modifier(Modifier::BOLD),
            );
        } else if is_active {
            set_str(
                buf,
                menu_x,
                y,
                &normal_text,
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
            );
        } else {
            set_str(buf, menu_x, y, &normal_text, Style::default().fg(Color::White));
        }
    }

    // Scrollbar track on right edge of menu
    if visible_items < app.items.len() {
        let bar_x = menu_x + menu_w;
        for r in 0..visible_items {
            set_str(buf, bar_x, menu_y + r, "│", Style::default().fg(Color::DarkGray));
        }

        let scrollbar_height = ((visible_items * visible_items) / app.items.len()).max(1);
        let scrollbar_pos = (app.scroll_offset * (visible_items.saturating_sub(scrollbar_height)))
            / (app.items.len().saturating_sub(visible_items)).max(1);

        for r in 0..scrollbar_height {
            set_str(buf, bar_x, menu_y + scrollbar_pos + r, "█", Style::default().fg(Color::White));
        }
    }

    // Scroll down indicator
    if app.scroll_offset + visible_items < app.items.len() {
        let msg = " ▼ SCROLL DOWN FOR MORE OPTIONS ▼ ";
        let msg_x = menu_x + (menu_w.saturating_sub(msg.len())) / 2;
        set_str(buf, msg_x, menu_y + visible_items, msg, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    }
}

fn draw_extreme_modal(buf: &mut Buffer, w: usize, h: usize) {
    let box_w = 62;
    let box_h = 9;
    let box_x = (w.saturating_sub(box_w)) / 2;
    let box_y = (h.saturating_sub(box_h)) / 2;

    let border_style = Style::default().fg(Color::Red).bg(Color::Black).add_modifier(Modifier::BOLD);
    let text_style = Style::default().fg(Color::White).bg(Color::Black).add_modifier(Modifier::BOLD);
    let warn_style = Style::default().fg(Color::Yellow).bg(Color::Black).add_modifier(Modifier::BOLD);

    let blank = " ".repeat(box_w);
    for r in 0..box_h {
        set_str(buf, box_x, box_y + r, &blank, Style::default().bg(Color::Black));
    }

    // Border box
    set_str(buf, box_x, box_y, &format!("╭{}╮", "─".repeat(box_w.saturating_sub(2))), border_style);
    for r in 1..(box_h.saturating_sub(1)) {
        set_str(buf, box_x, box_y + r, "│", border_style);
        set_str(buf, box_x + box_w - 1, box_y + r, "│", border_style);
    }
    set_str(buf, box_x, box_y + box_h - 1, &format!("╰{}╯", "─".repeat(box_w.saturating_sub(2))), border_style);

    // Content lines inside modal
    let lines = [
        (1, "⚠️  WARNING: EXTREME MODE", warn_style),
        (3, "This will minimize all hardware performance.", text_style),
        (4, "Press 'R' at any time to restore normal operation.", Style::default().fg(Color::DarkGray).bg(Color::Black)),
        (6, "[ Y - Confirm ]    [ N - Cancel ]", warn_style),
    ];

    for (row_offset, text, style) in lines {
        let text_x = box_x + (box_w.saturating_sub(text.chars().count())) / 2;
        set_str(buf, text_x, box_y + row_offset, text, style);
    }
}
