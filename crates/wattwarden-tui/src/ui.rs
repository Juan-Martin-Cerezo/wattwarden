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
    r#"                                                                    "#,
];

const BLOCKS: [char; 9] = [' ', ' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Minimum terminal the dashboard lays out for; below this only the notice shows.
const MIN_WIDTH: usize = 60;
const MIN_HEIGHT: usize = 14;

/// Summary line format across all platforms, displaying OS, battery status, time estimate, and wattage.
fn summary_line(os: &str, battery_pct: u8, charging: bool, est: &str, watts: f64) -> String {
    let status = if charging { "Charging" } else { "Discharging" };
    format!("OS: {os} | Battery: {battery_pct}% ({status}) | Est: {est} | Power: {watts:.1}W")
}

/// Writes `text` at (x, y), clipped to the buffer edge (never overflows).
fn set_str(buf: &mut Buffer, x: usize, y: usize, text: &str, style: Style) {
    let w = buf.area.width as usize;
    let h = buf.area.height as usize;
    if x >= w || y >= h {
        return;
    }
    let avail = w - x;
    let clipped: String = text.chars().take(avail).collect();
    buf.set_string(x as u16, y as u16, clipped, style);
}

/// Upper bound of the wattage axis: the smallest "nice" ceiling that fits the
/// peak draw with 20% headroom. Machines that burst past 15 W simply grow the
/// scale (15 → 20 → 25 → … → 500 W); quiet machines keep a fine-grained axis
/// instead of a permanently half-empty plot.
pub fn graph_scale(peak_watts: f64) -> f64 {
    const STEPS: [f64; 16] = [
        5.0, 10.0, 15.0, 20.0, 25.0, 30.0, 40.0, 50.0, 60.0, 80.0, 100.0, 120.0, 150.0, 200.0,
        300.0, 500.0,
    ];
    if peak_watts <= 0.0 {
        return 15.0;
    }
    let target = peak_watts * 1.2;
    STEPS
        .iter()
        .copied()
        .find(|step| *step >= target)
        .unwrap_or(*STEPS.last().expect("non-empty"))
}

/// Vertical stacking of the content region: the plot takes a share of the rows
/// (never more than half, never less than 3), the menu keeps the rest. The
/// `+ 2` covers the plot's title and axis rows. Below the smallest stack
/// (plot + caption + axis + one menu row) the plot is dropped: `(0, h)`.
fn split_vertical(content_h: usize) -> (usize, usize) {
    if content_h < 6 {
        return (0, content_h.max(1));
    }
    let raw = if content_h >= 24 {
        (content_h / 2).min(20)
    } else if content_h >= 14 {
        content_h / 3
    } else {
        content_h / 4
    };
    let graph_h = raw.clamp(3, content_h.saturating_sub(3));
    let menu_h = content_h.saturating_sub(graph_h + 2).max(1);
    (graph_h, menu_h)
}

pub fn draw(f: &mut Frame, app: &mut App) {
    app.refresh_rows_if_stale();

    let area = f.area();
    let w = area.width as usize;
    let h = area.height as usize;

    if w < MIN_WIDTH || h < MIN_HEIGHT {
        let msg = format!("PLEASE RESIZE TERMINAL (Minimum size: {MIN_WIDTH}x{MIN_HEIGHT})");
        let msg_x = (w.saturating_sub(msg.len())) / 2;
        let msg_y = h / 2;
        let style = Style::default().fg(Color::Red).add_modifier(Modifier::BOLD);
        set_str(f.buffer_mut(), msg_x, msg_y, &msg, style);
        return;
    }

    let buf = f.buffer_mut();
    let title_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    // 1. Header: full banner when it fits, compact title otherwise.
    let mut y = 1;
    if h >= 28 && w >= 72 {
        let art_x = w.saturating_sub(68) / 2;
        for (i, line) in ASCII_LOGO.iter().enumerate() {
            set_str(buf, art_x, y + i, line, title_style);
        }
        y += ASCII_LOGO.len() + 1;
    } else {
        let title = "WATTWARDEN";
        let tx = w.saturating_sub(title.len()) / 2;
        set_str(buf, tx, y, title, title_style);
        y += 2;
    }

    // 2. Centered status summary line.
    let summary = summary_line(
        app.backend.os_name(),
        app.backend.battery.battery_percentage().unwrap_or(0),
        app.backend.battery.is_charging().unwrap_or(false),
        &app.backend
            .battery
            .time_remaining()
            .unwrap_or_else(|_| "N/A".into()),
        app.backend.battery.consumption_watts().unwrap_or(0.0),
    );
    let info_x = w.saturating_sub(summary.len()) / 2;
    set_str(
        buf,
        info_x,
        y,
        &summary,
        Style::default().add_modifier(Modifier::BOLD),
    );
    y += 2;

    // 3. Content budget: everything up to one row above the footer.
    let content_h = h.saturating_sub(1).saturating_sub(y);
    let is_horizontal = w >= 110;

    let (menu_x, menu_y, menu_w, menu_h, graph_x, graph_y, graph_w, graph_h) = if is_horizontal {
        let menu_w = (w / 2).clamp(40, 52);
        let menu_x = 2;
        let graph_x = menu_x + menu_w + 2;
        let graph_w = w.saturating_sub(graph_x + 2);
        // Plot + title + axis need at least 34 columns to be readable.
        let (graph_w, graph_h) = if graph_w >= 34 {
            (graph_w, content_h.saturating_sub(2))
        } else {
            (0, 0)
        };
        (menu_x, y, menu_w, content_h, graph_x, y, graph_w, graph_h)
    } else {
        let (plot_h, menu_h) = split_vertical(content_h);
        let menu_w = w.saturating_sub(4);
        // Without a plot the menu starts at the top of the content region.
        let menu_y = if plot_h >= 3 { y + plot_h + 2 } else { y };
        (2, menu_y, menu_w, menu_h, 2, y, w.saturating_sub(4), plot_h)
    };

    // 4. Real-time power plot (adaptive wattage scale).
    if graph_h >= 3 && graph_w >= 34 {
        draw_graph_block(buf, app, graph_x, graph_y, graph_w, graph_h);
    }

    // 5. Menu with viewport scrolling and scrollbar.
    draw_menu(buf, app, menu_x, menu_y, menu_w, menu_h);

    // 6. Toast popup (if active).
    if let Some((msg, time)) = &app.toast {
        if time.elapsed() < Duration::from_secs(3) {
            let toast_str = format!(" {} ", msg);
            let toast_x = w.saturating_sub(toast_str.len()) / 2;
            let toast_y = h.saturating_sub(3);
            set_str(
                buf,
                toast_x,
                toast_y,
                &toast_str,
                Style::default()
                    .bg(Color::Yellow)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            );
        }
    }

    // 7. Extreme mode confirmation modal.
    if app.confirm_extreme {
        draw_extreme_modal(buf, w, h);
    }

    // 8. Footer controls help.
    let footer_text =
        "[UP/DOWN] Navigate | [L/R] Adjust | [ENTER] Apply | [R] Restore | [+/-] Speed | [Q] Quit";
    set_str(
        buf,
        2,
        h.saturating_sub(1),
        footer_text,
        Style::default().fg(Color::DarkGray),
    );
}

/// Power plot block: one caption row, the bars, the axis and the watt scale.
fn draw_graph_block(buf: &mut Buffer, app: &mut App, x: usize, y: usize, w: usize, h: usize) {
    let data = app.history.make_contiguous();
    let peak = data.iter().copied().fold(0.0_f64, f64::max);
    let scale = graph_scale(peak);
    let caption = format!(" POWER DRAW - scale {:.0}W - peak {:.1}W ", scale, peak);
    set_str(
        buf,
        x,
        y,
        &caption,
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    );
    draw_bar_graph(buf, x, y + 1, w, h, data, scale);
}

/// Scale-relative band colour: top third red, middle yellow, bottom green.
fn band_color(ratio: f64) -> Color {
    if ratio >= 2.0 / 3.0 {
        Color::Red
    } else if ratio >= 1.0 / 3.0 {
        Color::Yellow
    } else {
        Color::Green
    }
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
    if width < 34 || height < 3 {
        return;
    }

    let plot_w = width.saturating_sub(14);

    // Draw left axis
    for y_offset in 0..height {
        let screen_y = start_y + height - 1 - y_offset;
        set_str(
            buf,
            start_x,
            screen_y,
            "│",
            Style::default().fg(Color::DarkGray),
        );
    }
    // Draw bottom axis
    let bottom_line = format!("└{}", "─".repeat(plot_w));
    set_str(
        buf,
        start_x,
        start_y + height,
        &bottom_line,
        Style::default().fg(Color::DarkGray),
    );

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
            let char_idx = if dots_in_row >= 8 { 8 } else { dots_in_row };

            if char_idx > 0 {
                let color = band_color((y_offset + 1) as f64 / height as f64);
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
        let color = band_color(val_label / graph_max);

        set_str(
            buf,
            start_x + width - 12,
            screen_y,
            "│ ",
            Style::default().fg(Color::DarkGray),
        );
        set_str(
            buf,
            start_x + width - 10,
            screen_y,
            &format!("{:4.1} W", val_label),
            Style::default().fg(color),
        );
    }
}

/// Formats boolean states as `[ACTIVE]` or `[OFF]`.
pub(crate) fn display_bool(value: bool) -> String {
    if value {
        "[ACTIVE]".into()
    } else {
        "[OFF]".into()
    }
}

/// Formats numeric metrics as `[<n>]` or `[N/A]` when non-positive.
pub(crate) fn format_number(value: i64) -> String {
    if value <= 0 {
        "[N/A]".into()
    } else {
        format!("[{value}]")
    }
}

fn draw_menu(
    buf: &mut Buffer,
    app: &mut App,
    menu_x: usize,
    menu_y: usize,
    menu_w: usize,
    menu_h: usize,
) {
    let visible_items = menu_h.max(1);

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
        set_str(
            buf,
            msg_x,
            menu_y.saturating_sub(1),
            msg,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    }

    for i in 0..visible_items {
        let idx = app.scroll_offset + i;
        if idx >= app.items.len() || idx >= app.rows.len() {
            break;
        }

        let item = &app.items[idx];
        let row = &app.rows[idx];
        let y = menu_y + i;

        if let ActionItem::Header(header_title) = item {
            set_str(
                buf,
                menu_x,
                y,
                header_title,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
            continue;
        }

        let max_name_len = menu_w.saturating_sub(20).max(5);
        let truncated_name = if row.name.chars().count() > max_name_len {
            format!(
                "{}...",
                row.name
                    .chars()
                    .take(max_name_len.saturating_sub(3))
                    .collect::<String>()
            )
        } else {
            row.name.clone()
        };

        let is_selected = idx == app.selected;
        let line_text = format!(
            " > {:<width$} {:>15} ",
            truncated_name,
            row.value,
            width = max_name_len
        );
        let normal_text = format!(
            "   {:<width$} {:>15} ",
            truncated_name,
            row.value,
            width = max_name_len
        );

        if is_selected {
            set_str(
                buf,
                menu_x,
                y,
                &line_text,
                Style::default()
                    .fg(Color::Blue)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );
        } else if row.active {
            set_str(
                buf,
                menu_x,
                y,
                &normal_text,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            );
        } else {
            set_str(
                buf,
                menu_x,
                y,
                &normal_text,
                Style::default().fg(Color::White),
            );
        }
    }

    // Scrollbar track on right edge of menu
    if visible_items < app.items.len() {
        let bar_x = menu_x + menu_w;
        for r in 0..visible_items {
            set_str(
                buf,
                bar_x,
                menu_y + r,
                "│",
                Style::default().fg(Color::DarkGray),
            );
        }

        let scrollbar_height = ((visible_items * visible_items) / app.items.len()).max(1);
        let scrollbar_pos = (app.scroll_offset * (visible_items.saturating_sub(scrollbar_height)))
            / (app.items.len().saturating_sub(visible_items)).max(1);

        for r in 0..scrollbar_height {
            set_str(
                buf,
                bar_x,
                menu_y + scrollbar_pos + r,
                "█",
                Style::default().fg(Color::White),
            );
        }
    }

    // Scroll down indicator
    if app.scroll_offset + visible_items < app.items.len() {
        let msg = " ▼ SCROLL DOWN FOR MORE OPTIONS ▼ ";
        let msg_x = menu_x + (menu_w.saturating_sub(msg.len())) / 2;
        set_str(
            buf,
            msg_x,
            menu_y + visible_items,
            msg,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    }
}

fn draw_extreme_modal(buf: &mut Buffer, w: usize, h: usize) {
    let box_w = 62;
    let box_h = 9;
    let box_x = (w.saturating_sub(box_w)) / 2;
    let box_y = (h.saturating_sub(box_h)) / 2;

    let border_style = Style::default()
        .fg(Color::Red)
        .bg(Color::Black)
        .add_modifier(Modifier::BOLD);
    let text_style = Style::default()
        .fg(Color::White)
        .bg(Color::Black)
        .add_modifier(Modifier::BOLD);
    let warn_style = Style::default()
        .fg(Color::Yellow)
        .bg(Color::Black)
        .add_modifier(Modifier::BOLD);

    let blank = " ".repeat(box_w);
    for r in 0..box_h {
        set_str(
            buf,
            box_x,
            box_y + r,
            &blank,
            Style::default().bg(Color::Black),
        );
    }

    // Border box
    set_str(
        buf,
        box_x,
        box_y,
        &format!("╭{}╮", "─".repeat(box_w.saturating_sub(2))),
        border_style,
    );
    for r in 1..(box_h.saturating_sub(1)) {
        set_str(buf, box_x, box_y + r, "│", border_style);
        set_str(buf, box_x + box_w - 1, box_y + r, "│", border_style);
    }
    set_str(
        buf,
        box_x,
        box_y + box_h - 1,
        &format!("╰{}╯", "─".repeat(box_w.saturating_sub(2))),
        border_style,
    );

    // Content lines inside modal
    let lines = [
        (1, "⚠️  WARNING: EXTREME MODE", warn_style),
        (
            3,
            "This will minimize all hardware performance.",
            text_style,
        ),
        (
            4,
            "Press 'R' at any time to restore normal operation.",
            Style::default().fg(Color::DarkGray).bg(Color::Black),
        ),
        (6, "[ Y - Confirm ]    [ N - Cancel ]", warn_style),
    ];

    for (row_offset, text, style) in lines {
        let mut text_x = box_x + (box_w.saturating_sub(text.chars().count())) / 2;
        if row_offset == 1 {
            // Shift warning title by one column to compensate for emoji width.
            text_x += 1;
        }
        set_str(buf, text_x, box_y + row_offset, text, style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Boolean rows render as `[ACTIVE]`/`[OFF]`.
    #[test]
    fn golden_display_bool_is_active_or_off() {
        assert_eq!(display_bool(true), "[ACTIVE]");
        assert_eq!(display_bool(false), "[OFF]");
    }

    /// Numeric readings render as `[<n>]`, or literal `[N/A]` when non-positive.
    #[test]
    fn golden_numeric_values_are_bracketed_or_na() {
        assert_eq!(format_number(1100), "[1100]");
        assert_eq!(format_number(45), "[45]");
        assert_eq!(format_number(0), "[N/A]");
        assert_eq!(format_number(-3), "[N/A]");
    }

    /// LCD brightness value format.
    #[test]
    fn golden_lcd_brightness_value_has_no_percent_sign() {
        assert_eq!(format!("[{}]", 100), "[100]");
        assert_eq!(format!("[{}]", 0), "[0]");
    }

    /// Summary telemetry line formatting across supported platforms.
    #[test]
    fn golden_summary_line_formatting() {
        assert_eq!(
            summary_line("Linux", 82, false, "4:12", 12.0),
            "OS: Linux | Battery: 82% (Discharging) | Est: 4:12 | Power: 12.0W"
        );
        assert_eq!(
            summary_line("macOS", 100, true, "Charging", 0.0),
            "OS: macOS | Battery: 100% (Charging) | Est: Charging | Power: 0.0W"
        );
    }

    /// The wattage axis adapts to the machine: above 15 W it grows to the next
    /// nice step, quiet machines keep a finer axis, and every peak fits with
    /// headroom (never clipped).
    #[test]
    fn graph_scale_adapts_above_and_below_15w() {
        assert_eq!(
            graph_scale(0.0),
            15.0,
            "empty history keeps the old default"
        );
        assert_eq!(graph_scale(4.0), 5.0);
        assert_eq!(graph_scale(8.0), 10.0);
        assert_eq!(graph_scale(12.0), 15.0);
        assert_eq!(graph_scale(16.0), 20.0, "a 16 W burst must not clip");
        assert_eq!(graph_scale(14.0), 20.0);
        assert_eq!(graph_scale(38.0), 50.0);
        assert_eq!(graph_scale(115.0), 150.0);
        assert_eq!(graph_scale(400.0), 500.0);
        assert_eq!(graph_scale(9999.0), 500.0, "capped at the largest step");
        // Whatever the peak, the scale leaves headroom for it.
        for peak in [3.7, 15.0, 21.3, 44.9, 99.0, 150.0, 250.0] {
            assert!(graph_scale(peak) >= peak, "{peak}W must fit its own scale");
        }
    }

    /// The vertical stack always gives the plot room and the menu the rest.
    #[test]
    fn split_vertical_reserves_plot_and_menu_rows() {
        for content_h in 6..=60 {
            let (graph_h, menu_h) = split_vertical(content_h);
            assert!(graph_h >= 3, "plot needs at least 3 rows ({content_h})");
            assert!(menu_h >= 1, "menu needs at least 1 row ({content_h})");
            assert!(
                graph_h + 2 + menu_h <= content_h,
                "rows must fit: content={content_h} graph={graph_h} menu={menu_h}"
            );
        }
        // No room for the plot at all: the menu gets everything.
        for content_h in 1..6 {
            let (graph_h, menu_h) = split_vertical(content_h);
            assert_eq!(graph_h, 0);
            assert_eq!(menu_h, content_h);
        }
        // Roomy terminals give the plot a real share.
        let (graph_h, menu_h) = split_vertical(40);
        assert_eq!((graph_h, menu_h), (20, 18));
    }

    /// `set_str` clips instead of overflowing the buffer.
    #[test]
    fn set_str_clips_at_the_buffer_edge() {
        use ratatui::layout::Rect;
        let mut buf = Buffer::empty(Rect::new(0, 0, 10, 2));
        set_str(&mut buf, 6, 0, "OVERFLOWING", Style::default());
        let line: String = (0u16..10)
            .map(|x| buf.cell((x, 0u16)).unwrap().symbol().to_string())
            .collect();
        assert_eq!(line, "      OVER");
    }
}
