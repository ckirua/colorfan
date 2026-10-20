mod app;
mod config;
mod openrgb;
mod ui;

use std::io::{self, stdout};
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use app::App;
use config::Config;

fn main() {
    if let Err(e) = run() {
        eprintln!("colorfan error: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let config = Config::load().context("loading config")?;

    eprintln!("colorfan: probing OpenRGB devices (may take a few seconds)…");
    let devices = match openrgb::list_devices(&config) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("warning: could not list devices ({e:#}); using fallback zones");
            openrgb::parse_devices("")
        }
    };

    let mut app = App::new(config, devices);

    let mut terminal = setup_terminal()?;
    let result = event_loop(&mut terminal, &mut app);
    restore_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode().context("enable raw mode")?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen).context("enter alternate screen")?;
    let backend = CrosstermBackend::new(out);
    Terminal::new(backend).context("create terminal")
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();
    Ok(())
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    loop {
        terminal.draw(|f| ui::draw(f, app))?;

        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            // Ignore key-release / repeat noise on some terminals.
            if key.kind != crossterm::event::KeyEventKind::Press {
                continue;
            }
            app.handle_key(key)?;
            if app.should_quit {
                break;
            }
        }
    }
    Ok(())
}
