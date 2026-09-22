use crate::app::App;
use crate::ui::draw;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::stdout;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use wattwarden_core::*;

/// Short poll interval so keypresses stay responsive; the graph sampling cadence is
/// governed by `App::refresh_delay`, not by this timeout.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Set from the SIGINT/SIGTERM/SIGHUP handler so the loop can unwind and restore the
/// terminal instead of leaving it in raw mode.
static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);

extern "C" fn request_quit(_signal: i32) {
    // Async-signal-safe: just flip a flag; the main loop does the cleanup.
    QUIT_REQUESTED.store(true, Ordering::SeqCst);
}

fn install_signal_handlers() {
    #[cfg(unix)]
    {
        use nix::sys::signal::{signal, SigHandler, Signal};
        // SAFETY: the handler only performs an atomic store, which is async-signal-safe.
        unsafe {
            let _ = signal(Signal::SIGINT, SigHandler::Handler(request_quit));
            let _ = signal(Signal::SIGTERM, SigHandler::Handler(request_quit));
            let _ = signal(Signal::SIGHUP, SigHandler::Handler(request_quit));
        }
    }
}

fn quit_requested() -> bool {
    QUIT_REQUESTED.load(Ordering::SeqCst)
}

pub fn run_tui(mut app: App) -> Result<()> {
    QUIT_REQUESTED.store(false, Ordering::SeqCst);
    install_signal_handlers();

    enable_raw_mode().map_err(WattWardenError::GeneralIo)?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen).map_err(WattWardenError::GeneralIo)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend).map_err(WattWardenError::GeneralIo)?;

    let res = run_loop(&mut terminal, &mut app);

    // Clean terminal restoration, also on signal-driven shutdown.
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    res
}

fn run_loop<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    while !app.should_quit {
        if quit_requested() {
            app.should_quit = true;
            break;
        }

        app.tick();
        terminal
            .draw(|f| draw(f, app))
            .map_err(WattWardenError::GeneralIo)?;

        if !event::poll(POLL_INTERVAL).map_err(WattWardenError::GeneralIo)? {
            continue;
        }
        let Event::Key(key) = event::read().map_err(WattWardenError::GeneralIo)? else {
            continue;
        };
        if key.kind != crossterm::event::KeyEventKind::Press {
            continue;
        }

        // Extreme Mode confirmation modal takes over the keyboard while visible.
        if app.confirm_extreme {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => app.confirm_extreme_mode(),
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => app.cancel_extreme_mode(),
                _ => {}
            }
            continue;
        }

        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            // Ctrl-C is not delivered as SIGINT in raw mode, so handle the key too.
            KeyCode::Char('c') if ctrl => {
                app.should_quit = true;
            }
            KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                app.should_quit = true;
            }
            // Go hotkey `r`/`R`/Ctrl-R: restore + "System Restored" toast.
            KeyCode::Char('r') | KeyCode::Char('R') => {
                app.restore("System Restored");
            }
            KeyCode::Char('+') => {
                app.speed_up();
            }
            KeyCode::Char('-') => {
                app.speed_down();
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('w') => {
                app.prev_menu();
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('s') => {
                app.next_menu();
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Char('a') => {
                app.handle_left();
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Char('d') => {
                app.handle_right();
            }
            KeyCode::Enter => {
                app.handle_enter();
            }
            _ => {}
        }
    }
    Ok(())
}
