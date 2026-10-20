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

/// Compact header used whenever the full banner cannot fit.
const TITLE: &str = "WATTWARDEN";
const BANNER_W: usize = 68;
const BANNER_H: usize = 6;

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

/// A rectangular block of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

/// Where every block goes for a given terminal size. Valid for **any** `w`/`h`:
/// blocks are dropped — never stretched past the screen — when they cannot fit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// 6-row logo, or a 1-row title on smaller terminals.
    pub banner: Option<Region>,
    pub summary_y: Option<usize>,
    pub menu: Region,
    /// Plot area (its caption and axis rows sit outside this region).
    pub graph: Option<Region>,
    pub footer_y: Option<usize>,
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

/// Vertical stacking of the content region: caption + plot + axis + one blank
/// row (scroll indicator) above the menu. The plot takes up to half the rows
/// (never more than 20, never fewer than 3); below 7 rows there is no plot.
/// Returns `(plot_h, menu_h)` and tiles the rows exactly.
fn split_vertical(content_h: usize) -> (usize, usize) {
    if content_h < 7 {
        return (0, content_h);
    }
    let raw = if content_h >= 26 {
        (content_h / 2).min(20)
    } else if content_h >= 16 {
        content_h / 3
    } else {
        content_h / 4
    };
    let graph_h = raw.clamp(3, content_h.saturating_sub(4));
    let menu_h = content_h.saturating_sub(graph_h + 3);
    (graph_h, menu_h)
}

/// Computes where everything goes. Pure and total: no size is ever rejected.
pub fn layout(w: usize, h: usize) -> Layout {
    let mut y = 0usize;

    let banner = if w >= 72 && h >= 28 {
        y = 1;
        let region = Region {
            x: (w - BANNER_W) / 2,
            y: 1,
            w: BANNER_W,
            h: BANNER_H,
        };
        y += BANNER_H + 1;
        Some(region)
    } else if w >= 16 && h >= 4 {
        y = 1;
        let region = Region {
            x: (w - TITLE.len()) / 2,
            y: 1,
            w: TITLE.len(),
            h: 1,
        };
        y += 2;
        Some(region)
    } else {
        None
    };

    let summary_y = if w >= 24 && h >= y + 3 {
        let sy = y;
        y += 2;
        Some(sy)
    } else {
        None
    };

    let footer_y = if h >= 3 { Some(h - 1) } else { None };
    let bottom = footer_y.map(|f| f.saturating_sub(1)).unwrap_or(h);
    let content_h = bottom.saturating_sub(y);

    let (menu, graph) = if w >= 110 && content_h >= 5 {
        // Side by side: the menu keeps a readable column and the plot takes
        // the rest — both grow with the terminal.
        let menu_w = (w * 45 / 100).clamp(44, 72);
        let gx = 2 + menu_w + 2;
        let gw = w.saturating_sub(gx + 2);
        let gh = content_h.saturating_sub(2);
        let graph = (gw >= 34 && gh >= 3).then_some(Region {
            x: gx,
            y,
            w: gw,
            h: gh,
        });
        let menu = if graph.is_some() {
            Region {
                x: 2,
                y,
                w: menu_w,
                h: content_h,
            }
        } else {
            Region {
                x: 2,
                y,
                w: w.saturating_sub(4),
                h: content_h,
            }
        };
        (menu, graph)
    } else {
        // Stacked: plot above (caption/axis/blank), menu below.
        let (plot_h, menu_h) = split_vertical(content_h);
        let graph = (plot_h >= 3 && w >= 40).then_some(Region {
            x: 2,
            y,
            w: w.saturating_sub(4),
            h: plot_h,
        });
        let menu_y = if graph.is_some() { y + plot_h + 3 } else { y };
        let menu = Region {
            x: 2,
            y: menu_y,
            w: w.saturating_sub(4),
            h: menu_h,
        };
        (menu, graph)
    };

    Layout {
        banner,
        summary_y,
        menu,
        graph,
        footer_y,
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    app.refresh_rows_if_stale();

    let area = f.area();
    let w = area.width as usize;
    let h = area.height as usize;
    let l = layout(w, h);

    let buf = f.buffer_mut();
    let title_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);

    // 1. Header: full banner or compact title, whichever fits.
    if let Some(b) = l.banner {
        if b.h >= BANNER_H {
            for (i, line) in ASCII_LOGO.iter().enumerate() {
                set_str(buf, b.x, b.y + i, line, title_style);
            }
        } else {
            set_str(buf, b.x, b.y, TITLE, title_style);
        }
    }

    // 2. Centered status summary line.
    if let Some(sy) = l.summary_y {
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
        let info_x = w.saturating_sub(summary.chars().count()) / 2;
        set_str(
            buf,
            info_x,
            sy,
            &summary,
            Style::default().add_modifier(Modifier::BOLD),
        );
    }

    // 3. Real-time power plot (adaptive wattage scale).
    if let Some(g) = l.graph {
        draw_graph_block(buf, app, g.x, g.y, g.w, g.h);
    }

    // 4. Menu with viewport scrolling and scrollbar.
    if l.menu.h >= 1 {
        draw_menu(buf, app, l.menu.x, l.menu.y, l.menu.w, l.menu.h);
    }

    // 5. Toast popup (if active).
    if let Some((msg, time)) = &app.toast {
        if time.elapsed() < Duration::from_secs(3) {
            let toast_str = format!(" {} ", msg);
            let toast_x = w.saturating_sub(toast_str.chars().count()) / 2;
            let toast_y = if h >= 3 { h - 3 } else { h.saturating_sub(1) };
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

    // 6. Extreme mode confirmation modal.
    if app.confirm_extreme {
        draw_extreme_modal(buf, w, h);
    }

    // 7. Footer controls help.
    if let Some(fy) = l.footer_y {
        let footer_text = "[UP/DOWN] Navigate | [L/R] Adjust | [ENTER] Apply | [R] Restore | \
                          [+/-] Speed | [Q] Quit";
        set_str(
            buf,
            2,
            fy,
            footer_text,
            Style::default().fg(Color::DarkGray),
        );
    }
}

/// Power plot block: caption row, the bars, the axis and the watt scale.
/// `h` is the plot height; the caption and axis rows sit outside it.
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

/// Truncates to `w` chars, marking the cut with `...`.
fn fit(text: &str, w: usize) -> String {
    if text.chars().count() <= w {
        return text.to_string();
    }
    if w <= 3 {
        return text.chars().take(w).collect();
    }
    let mut out: String = text.chars().take(w - 3).collect();
    out.push_str("...");
    out
}

/// One menu row fitted to exactly `menu_w` columns: marker + name + value.
/// Fields shrink before anything may overflow, so every row fits at any width.
pub fn menu_line(menu_w: usize, prefix: &str, name: &str, value: &str) -> String {
    if menu_w == 0 {
        return String::new();
    }
    let val_w = (menu_w / 3).clamp(5, 15).min(menu_w.saturating_sub(1));
    let pref_w = prefix.chars().count().min(menu_w);
    let name_w = menu_w.saturating_sub(pref_w + val_w + 1).max(1);
    let line = format!(
        "{}{:<name_w$} {:>val_w$}",
        prefix,
        fit(name, name_w),
        fit(value, val_w),
        name_w = name_w,
        val_w = val_w
    );
    line.chars().take(menu_w).collect()
}

fn draw_menu(
    buf: &mut Buffer,
    app: &mut App,
    menu_x: usize,
    menu_y: usize,
    menu_w: usize,
    menu_h: usize,
) {
    let visible_items = menu_h;

    // Dynamic scroll bounds
    if app.selected < app.scroll_offset {
        app.scroll_offset = app.selected;
    }
    if app.selected >= app.scroll_offset + visible_items {
        app.scroll_offset = app.selected - visible_items + 1;
    }

    // Scroll up indicator
    if app.scroll_offset > 0 {
        let msg = " ▲ MORE ABOVE ▲ ";
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
                &fit(header_title, menu_w),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );
            continue;
        }

        let is_selected = idx == app.selected;
        let prefix = if is_selected { " > " } else { "   " };
        let line = menu_line(menu_w, prefix, &row.name, &row.value);

        if is_selected {
            set_str(
                buf,
                menu_x,
                y,
                &line,
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
                &line,
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            );
        } else {
            set_str(buf, menu_x, y, &line, Style::default().fg(Color::White));
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
        let msg = " ▼ MORE BELOW ▼ ";
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
    let box_w = 62.min(w).max(3);
    let box_h = 9.min(h).max(1);
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
        if row_offset >= box_h {
            continue;
        }
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

    /// The vertical stack gives the plot room, the menu the rest, and tiles
    /// the rows exactly (caption + plot + axis + blank + menu).
    #[test]
    fn split_vertical_reserves_plot_and_menu_rows() {
        for content_h in 7..=60 {
            let (graph_h, menu_h) = split_vertical(content_h);
            assert!(graph_h >= 3, "plot needs at least 3 rows ({content_h})");
            assert!(menu_h >= 1, "menu needs at least 1 row ({content_h})");
            assert_eq!(
                graph_h + 3 + menu_h,
                content_h,
                "rows must tile exactly: content={content_h}"
            );
        }
        // No room for the plot at all: the menu gets everything.
        for content_h in 0..7 {
            let (graph_h, menu_h) = split_vertical(content_h);
            assert_eq!(graph_h, 0);
            assert_eq!(menu_h, content_h);
        }
        // Roomy terminals give the plot a real share.
        assert_eq!(split_vertical(40), (20, 17));
    }

    /// Everything stays inside the screen at EVERY size, blocks never overlap,
    /// and no size is ever rejected — the dashboard is fully responsive.
    #[test]
    fn layout_fits_every_terminal_size() {
        for w in 4..=240usize {
            for h in 2..=80usize {
                let l = layout(w, h);

                let inside = |r: Region| r.x + r.w <= w && r.y + r.h <= h;
                if let Some(b) = l.banner {
                    assert!(inside(b), "banner outside {w}x{h}: {b:?}");
                }
                if let Some(g) = l.graph {
                    assert!(inside(g), "graph outside {w}x{h}: {g:?}");
                }
                assert!(inside(l.menu), "menu outside {w}x{h}: {:?}", l.menu);
                if let Some(fy) = l.footer_y {
                    assert_eq!(fy, h - 1);
                    assert!(l.menu.y + l.menu.h <= fy, "menu overlaps footer at {w}x{h}");
                }
                if let Some(g) = l.graph {
                    if g.y == l.menu.y {
                        // Side by side: columns must not overlap.
                        assert!(
                            g.x >= l.menu.x + l.menu.w,
                            "side-by-side blocks overlap at {w}x{h}"
                        );
                    } else {
                        // Stacked: caption/axis/blank rows must clear the menu.
                        assert!(
                            g.y + g.h + 3 <= l.menu.y,
                            "stacked plot overlaps menu at {w}x{h}"
                        );
                    }
                }
                if let Some(sy) = l.summary_y {
                    assert!(sy < h);
                }
            }
        }
    }

    /// A comfortable terminal shows every section.
    #[test]
    fn layout_shows_everything_at_typical_sizes() {
        for (w, h) in [
            (60usize, 14usize),
            (80, 24),
            (100, 30),
            (130, 40),
            (200, 60),
        ] {
            let l = layout(w, h);
            assert!(l.banner.is_some(), "{w}x{h} must show a header");
            assert!(l.summary_y.is_some(), "{w}x{h} must show telemetry");
            assert!(l.graph.is_some(), "{w}x{h} must show the power plot");
            assert!(l.menu.h > 0, "{w}x{h} must show menu rows");
            assert!(l.footer_y.is_some(), "{w}x{h} must show the footer");
        }
        // Wide terminals go side by side.
        let wide = layout(130, 40);
        assert!(wide.graph.expect("plot").x >= wide.menu.x + wide.menu.w);
    }

    /// Menu rows fit their column at any width: never overflow, never empty.
    #[test]
    fn menu_line_fits_any_width() {
        for menu_w in 1..=80usize {
            for (name, value) in [
                ("Charge Limit", "[80%]"),
                ("⚡ Performance Mode", "[EXECUTE]"),
                ("Autosuspend PCI/USB", "[ACTIVE]"),
                ("CPU Freq (MHz)", "[N/A]"),
            ] {
                for prefix in [" > ", "   "] {
                    let line = menu_line(menu_w, prefix, name, value);
                    assert!(
                        line.chars().count() <= menu_w,
                        "row overflow at width {menu_w}: {line:?}"
                    );
                }
            }
        }
        // Golden: a comfortable width keeps name and value apart.
        assert_eq!(
            menu_line(40, " > ", "Charge Limit", "[80%]"),
            " > Charge Limit                    [80%]"
        );
        // Narrow: the name gives way before the value does.
        let narrow = menu_line(12, " > ", "Autosuspend PCI/USB", "[ACTIVE]");
        assert!(narrow.chars().count() <= 12);
        assert!(narrow.contains('A'), "value must survive the squeeze");
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

    /// Render smoke test: drawing the full dashboard never panics and never
    /// shows a "resize" wall at any size — the layout adapts instead.
    #[test]
    fn render_smoke_at_every_size_never_panics() {
        use ratatui::{backend::TestBackend, Terminal};

        let backend = wattwarden_platform::PlatformBackend::new().expect("backend");
        let config = Config::load_or_default(None);
        let mut app = App::new(backend, config);

        for (w, h) in [
            (20u16, 5u16),
            (30, 8),
            (40, 10),
            (50, 12),
            (60, 14),
            (70, 20),
            (80, 24),
            (100, 30),
            (110, 35),
            (130, 40),
            (180, 50),
            (240, 60),
            (12, 3),
            (45, 7),
            (72, 28),
        ] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).expect("terminal");
            terminal
                .draw(|f| draw(f, &mut app))
                .unwrap_or_else(|e| panic!("draw failed at {w}x{h}: {e}"));
            let buf = terminal.backend().buffer();
            let text: String = buf
                .content()
                .iter()
                .map(|c| c.symbol().to_string())
                .collect();
            assert!(
                !text.contains("RESIZE"),
                "no resize wall allowed at {w}x{h}"
            );
        }
    }
}
