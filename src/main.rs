//! GameOverlay - a small always-on-top overlay that shows the current FPS and
//! the ping (round trip time) to a chosen server while you are in a game.

mod autostart;
mod overlay;
mod ping;

use std::sync::atomic::Ordering;
use std::time::Duration;

use overlay::AppState;
use ping::{PingMode, Target};

/// Hides the console window so the overlay does not steal focus on launch.
/// Keeps it visible in `--verbose` mode so the measurements can be read.
fn hide_console() {
    use windows::Win32::System::Console::GetConsoleWindow;
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    unsafe {
        let console = GetConsoleWindow();
        if !console.is_invalid() {
            let _ = ShowWindow(console, SW_HIDE);
        }
    }
}

fn print_help() {
    println!(
        r#"GameOverlay 1.1 - FPS & ping overlay for games

USAGE:
    GameOverlay [OPTIONS]

OPTIONS:
    --host <HOST>        Server to ping (default: 1.1.1.1)
    --port <PORT>        Port used for the TCP latency probe (default: 443)
    --mode <icmp|tcp>    Ping method (default: icmp, falls back to tcp)
    --targets <LIST>     Comma separated "host:port" list, cycled with F2/F3
    --fixed-position     Do not follow the game window, stay in the corner
    --autostart          Register the overlay in HKCU Run, then start
    --no-autostart       Remove the HKCU Run entry, then start
    --autostart-status   Print whether autostart is registered and exit
    --verbose            Print measured FPS / ping to the console
    -h, --help           Show this help

HOTKEYS:
    F1        show / hide the overlay
    F2 / F3   next / previous ping target
    F4        toggle "follow the game window" positioning
    F6        toggle autostart (HKCU Run entry)
    End       quit the overlay
"#
    );
}

struct Options {
    targets: Vec<Target>,
    fixed: bool,
    verbose: bool,
    autostart: Option<bool>,
    autostart_status: bool,
}

fn parse_args() -> Result<Options, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut host = "1.1.1.1".to_string();
    let mut port: u16 = 443;
    let mut mode = PingMode::Icmp;
    let mut targets: Option<Vec<Target>> = None;
    let mut fixed = false;
    let mut verbose = false;
    let mut autostart_flag: Option<bool> = None;
    let mut autostart_status = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            "--host" => {
                i += 1;
                host = args.get(i).ok_or("--host needs a value")?.clone();
            }
            "--port" => {
                i += 1;
                port = args
                    .get(i)
                    .ok_or("--port needs a value")?
                    .parse()
                    .map_err(|_| "bad port value")?;
            }
            "--mode" => {
                i += 1;
                mode = PingMode::parse(args.get(i).ok_or("--mode needs a value")?)
                    .ok_or("mode must be icmp or tcp")?;
            }
            "--targets" => {
                i += 1;
                let list = args.get(i).ok_or("--targets needs a value")?;
                let mut parsed = Vec::new();
                for item in list.split(',') {
                    let item = item.trim();
                    if item.is_empty() {
                        continue;
                    }
                    let (h, p) = match item.rsplit_once(':') {
                        Some((h, p)) => (h.to_string(), p.parse::<u16>().map_err(|_| "bad port")?),
                        None => (item.to_string(), port),
                    };
                    parsed.push(Target::new(&h, p, mode));
                }
                if parsed.is_empty() {
                    return Err("no targets parsed".into());
                }
                targets = Some(parsed);
            }
            "--fixed-position" => fixed = true,
            "--verbose" => verbose = true,
            "--autostart" => autostart_flag = Some(true),
            "--no-autostart" => autostart_flag = Some(false),
            "--autostart-status" => autostart_status = true,
            // Accepted and ignored: the autostart entry launches with it.
            "--tray-quiet" => {}
            other => return Err(format!("unknown option: {other}")),
        }
        i += 1;
    }

    let targets = targets.unwrap_or_else(|| vec![Target::new(&host, port, mode)]);
    Ok(Options {
        targets,
        fixed,
        verbose,
        autostart: autostart_flag,
        autostart_status,
    })
}

fn main() {
    let opts = match parse_args() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}\n");
            print_help();
            std::process::exit(1);
        }
    };

    if opts.autostart_status {
        println!(
            "autostart: {}",
            if autostart::is_enabled() { "enabled" } else { "disabled" }
        );
        return;
    }

    if let Some(enable) = opts.autostart {
        let result = if enable { autostart::enable() } else { autostart::disable() };
        match result {
            Ok(()) => println!(
                "autostart {}",
                if enable { "enabled" } else { "disabled" }
            ),
            Err(e) if opts.verbose => eprintln!("autostart failed: {e}"),
            Err(_) => {}
        }
    }

    let state = AppState::new();
    state.follow_window.store(!opts.fixed, Ordering::Relaxed);
    state.verbose.store(opts.verbose, Ordering::Relaxed);
    state.autostart.store(autostart::is_enabled(), Ordering::Relaxed);

    if !opts.verbose {
        hide_console();
    }

    // ping worker
    {
        let state = state.clone();
        let targets = opts.targets.clone();
        let verbose = opts.verbose;
        std::thread::spawn(move || {
            loop {
                if state.should_quit.load(Ordering::Relaxed) {
                    break;
                }
                let idx = state.target_index.load(Ordering::Relaxed) % targets.len();
                let target = &targets[idx];

                if let Ok(mut name) = state.target_name.lock() {
                    *name = target.label();
                }

                let result = ping::probe(target, Duration::from_millis(1500));
                match result.rtt_ms {
                    Some(ms) => {
                        let deci = (ms * 10.0).round() as i32;
                        state.ping_deci.store(deci, Ordering::Relaxed);
                        state.ping_lost.store(false, Ordering::Relaxed);
                        state.push_ping_sample(deci);
                        if verbose {
                            println!("[ping] {} -> {:.1} ms", target.label(), ms);
                        }
                    }
                    None => {
                        state.ping_deci.store(-1, Ordering::Relaxed);
                        state.ping_lost.store(true, Ordering::Relaxed);
                        state.push_ping_sample(-1);
                        if verbose {
                            println!("[ping] {} -> timeout", target.label());
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(1000));
            }
        });
    }

    // fps worker
    {
        let state = state.clone();
        std::thread::spawn(move || overlay::fps_thread(state));
    }

    println!(
        "GameOverlay 1.1 started. Targets: {} | autostart: {}",
        opts.targets.len(),
        if state.autostart.load(Ordering::Relaxed) { "on" } else { "off" }
    );
    for (i, t) in opts.targets.iter().enumerate() {
        println!("  [{}] {}", i, t.label());
    }
    println!("F1 hide/show | F2/F3 target | F4 position | F6 autostart | End quit");

    overlay::run_overlay(state);
}
