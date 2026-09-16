use crate::app::App;
use crate::ui::draw;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::stdout;
use std::time::Duration;
use wattwarden_core::*;

pub fn run_tui(mut app: App) -> Result<()> {
    enable_raw_mode().map_err(|e| WattWardenError::GeneralIo(e))?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen).map_err(|e| WattWardenError::GeneralIo(e))?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend).map_err(|e| WattWardenError::GeneralIo(e))?;

    let res = run_loop(&mut terminal, &mut app);

    // Clean terminal restoration
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();

    res
}

fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<()> {
    while !app.should_quit {
        app.tick();
        terminal.draw(|f| draw(f, app)).map_err(|e| WattWardenError::GeneralIo(e))?;

        if event::poll(Duration::from_millis(250)).map_err(|e| WattWardenError::GeneralIo(e))? {
            if let Event::Key(key) = event::read().map_err(|e| WattWardenError::GeneralIo(e))? {
                if key.kind != crossterm::event::KeyEventKind::Press {
                    continue;
                }

                // If Extreme Mode confirmation modal is visible
                if app.confirm_extreme {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            app.confirm_extreme_mode();
                        }
                        KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                            app.cancel_extreme_mode();
                        }
                        _ => {}
                    }
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                        app.should_quit = true;
                    }
                    KeyCode::Char('r') | KeyCode::Char('R') => {
                        app.restore_defaults();
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
        }
    }
    Ok(())
}
