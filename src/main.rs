//! rat: a Ratatui terminal dashboard for a GoChain / GoChain-AMM node,
//! with a small resident rat who reacts to chain events.
//!
//! One binary, two souls: a live node dashboard and a playful rat.
//! Modes:
//!   rat                     poll http://127.0.0.1:8080
//!   rat --node URL          poll another node (e.g. an AMM node port)
//!   rat --demo              generated fake chain, no node needed
//!   rat --snapshot [--size WxH]   render one frame as text and exit

mod http;
mod model;
mod ui;

use std::io;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::{CrosstermBackend, TestBackend};
use ratatui::Terminal;

use model::{App, Focus};

pub struct Args {
    pub node: String,
    pub demo: bool,
    pub snapshot: bool,
    pub size: (u16, u16),
}

fn parse_args() -> Args {
    let mut args = Args {
        node: "http://127.0.0.1:8080".to_string(),
        demo: false,
        snapshot: false,
        size: (100, 30),
    };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--node" => {
                if i + 1 < argv.len() {
                    args.node = argv[i + 1].trim_end_matches('/').to_string();
                    i += 1;
                }
            }
            "--demo" => args.demo = true,
            "--snapshot" => args.snapshot = true,
            "--size" => {
                if i + 1 < argv.len() {
                    if let Some((w, h)) = argv[i + 1].split_once('x') {
                        if let (Ok(w), Ok(h)) = (w.parse::<u16>(), h.parse::<u16>()) {
                            args.size = (w.max(20), h.max(8));
                        }
                    }
                    i += 1;
                }
            }
            "--help" | "-h" => {
                println!(
                    "rat - GoChain terminal dashboard with a resident rat\n\n\
                     USAGE:\n  rat [--node URL] [--demo] [--snapshot [--size WxH]]\n\n\
                     KEYS:\n  q  quit\n  r  force refresh\n  tab  move panel focus"
                );
                std::process::exit(0);
            }
            _ => {}
        }
        i += 1;
    }
    args
}

fn main() -> io::Result<()> {
    let args = parse_args();

    if args.snapshot {
        let mut app = App::new(&args);
        app.poll();
        let backend = TestBackend::new(args.size.0, args.size.1);
        let mut terminal = Terminal::new(backend)?;
        terminal.draw(|f| ui::render(f, &mut app))?;
        let buffer = terminal.backend().buffer().clone();
        let area = buffer.area;
        for y in 0..area.height {
            let mut line = String::new();
            for x in 0..area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            println!("{}", line.trim_end());
        }
        return Ok(());
    }

    // Restore the terminal even if we panic inside the loop.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = io::stdout().execute(LeaveAlternateScreen);
        default_hook(info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run(&mut terminal, &args);

    disable_raw_mode()?;
    io::stdout().execute(LeaveAlternateScreen)?;
    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, args: &Args) -> io::Result<()> {
    let mut app = App::new(args);
    app.poll();

    let tick_rate = Duration::from_millis(200);
    let poll_rate = Duration::from_millis(2000);
    let mut last_tick = Instant::now();
    let mut last_poll = Instant::now();

    loop {
        terminal.draw(|f| ui::render(f, &mut app))?;

        let timeout = tick_rate.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') => return Ok(()),
                        KeyCode::Char('r') => {
                            app.poll();
                            last_poll = Instant::now();
                        }
                        KeyCode::Tab => app.focus = app.focus.next(),
                        KeyCode::BackTab => app.focus = app.focus.prev(),
                        _ => {}
                    }
                }
            }
        }
        if last_tick.elapsed() >= tick_rate {
            app.tick();
            last_tick = Instant::now();
        }
        if last_poll.elapsed() >= poll_rate {
            app.poll();
            last_poll = Instant::now();
        }
    }
}

impl Focus {
    fn next(self) -> Focus {
        match self {
            Focus::Blocks => Focus::Peers,
            Focus::Peers => Focus::Rat,
            Focus::Rat => Focus::Blocks,
        }
    }
    fn prev(self) -> Focus {
        match self {
            Focus::Blocks => Focus::Rat,
            Focus::Peers => Focus::Blocks,
            Focus::Rat => Focus::Peers,
        }
    }
}
