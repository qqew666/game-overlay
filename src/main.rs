//! GameOverlay — FPS and ping overlay for games.
//!
//! The binary is a Windows GUI app: launching it opens the control panel and
//! puts the card on top of the active window. There is no console; `--verbose`
//! writes `%LOCALAPPDATA%\GameOverlay\overlay.log` instead.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod appearance;
mod artwork;
mod autostart;
mod deployment;
mod draw;
mod fps;
mod instance;
mod layout;
mod log;
mod movement;
mod overlay;
mod ping;
mod settings;
mod state;
mod ui;
mod update;

/// Версия, которую приложение сравнивает с тегом последнего релиза.
pub const APP_VERSION: &str = "1.5.5";

use std::sync::atomic::Ordering;
use std::time::Duration;

use ping::{PingMode, Target};
use state::AppState;

fn print_help() {
    crate::vlog!(
        r#"GameOverlay 1.5 - FPS & ping overlay for games

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
    --autostart-status   Log whether autostart is registered and exit
    --tray-quiet         Start without showing the control panel
    --verbose            Write measurements to overlay.log
    -h, --help           Show this help

HOTKEYS:
    F1        show / hide the overlay
    F2 / F3   next / previous ping target
    F7        compact / full overlay
    F4        toggle "follow the game window" positioning
    F6        toggle autostart (HKCU Run entry)
    End       quit
"#
    );
}

struct Options {
    targets: Vec<Target>,
    fixed: bool,
    verbose: bool,
    autostart: Option<bool>,
    autostart_status: bool,
    tray_quiet: bool,
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
    let mut tray_quiet = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" => {
                // The log is the only sink in a GUI build; initialise it first.
                log::init(true);
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
            "--tray-quiet" => tray_quiet = true,
            // Accepted and ignored: kept so old shortcuts keep working.
            "--elevated" | "--replace" | "--panel" => {}
            other => return Err(format!("unknown option: {other}")),
        }
        i += 1;
    }

    let custom = args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--host" | "--port" | "--mode"));
    let targets = targets.unwrap_or_else(|| {
        if custom {
            vec![Target::new(&host, port, mode)]
        } else {
            vec![
                Target::new("1.1.1.1", 443, mode),
                Target::new("8.8.8.8", 53, mode),
                Target::new("9.9.9.9", 443, mode),
            ]
        }
    });
    Ok(Options {
        targets,
        fixed,
        verbose,
        autostart: autostart_flag,
        autostart_status,
        tray_quiet,
    })
}

fn main() {
    if deployment::bootstrap() {
        return;
    }
    let opts = match parse_args() {
        Ok(v) => v,
        Err(e) => {
            log::init(true);
            crate::vlog!("error: {e}");
            print_help();
            std::process::exit(1);
        }
    };

    log::init(opts.verbose);
    appearance::init();

    if opts.autostart_status {
        crate::vlog!(
            "autostart: {}",
            if autostart::is_enabled() {
                "enabled"
            } else {
                "disabled"
            }
        );
        if !opts.verbose {
            return;
        }
    }

    if let Some(enable) = opts.autostart {
        let result = if enable {
            autostart::enable()
        } else {
            autostart::disable()
        };
        match result {
            Ok(()) => crate::vlog!("autostart {}", if enable { "enabled" } else { "disabled" }),
            Err(e) => crate::vlog!("autostart failed: {e}"),
        }
    }

    let handoff = std::env::args().any(|arg| arg == "--elevated" || arg == "--replace");
    let _instance = match instance::enter(handoff) {
        Ok(Some(instance)) => instance,
        Ok(None) => return,
        Err(error) => {
            crate::vlog!("Cannot establish single instance: {error}");
            return;
        }
    };
    let state = AppState::new(opts.targets.clone());
    state.follow_window.store(!opts.fixed, Ordering::Relaxed);
    state.verbose.store(opts.verbose, Ordering::Relaxed);
    state
        .autostart
        .store(autostart::is_enabled(), Ordering::Relaxed);
    state
        .overlay_scale
        .store(settings::load_scale(), Ordering::Relaxed);

    // Ping worker.
    {
        let state = state.clone();
        let targets = opts.targets.clone();
        let verbose = opts.verbose;
        std::thread::spawn(move || loop {
            if state.should_quit.load(Ordering::Relaxed) {
                break;
            }
            if targets.is_empty() {
                break;
            }
            let idx = state.target_index.load(Ordering::Relaxed) % targets.len();
            let target = &targets[idx];

            if let Ok(mut name) = state.target_name.lock() {
                *name = target.label();
            }

            let measured = ping::probe(target, Duration::from_millis(1500)).rtt_ms;
            if state.target_index.load(Ordering::Relaxed) % targets.len() != idx {
                continue;
            }
            match measured {
                Some(ms) => {
                    let deci = (ms * 10.0).round() as i32;
                    state.ping_deci.store(deci, Ordering::Relaxed);
                    state.ping_lost.store(false, Ordering::Relaxed);
                    state.push_ping_sample(deci);
                    if verbose {
                        crate::vlog!("[ping] {} -> {:.1} ms", target.label(), ms);
                    }
                }
                None => {
                    state.ping_deci.store(-1, Ordering::Relaxed);
                    state.ping_lost.store(true, Ordering::Relaxed);
                    state.push_ping_sample(-1);
                    if verbose {
                        crate::vlog!("[ping] {} -> timeout", target.label());
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(1000));
        });
    }

    // Frame rate of the foreground window.
    fps::start(state.clone());

    // Overlay on its own thread: it owns the hotkeys and the layered window.
    {
        let state = state.clone();
        std::thread::spawn(move || overlay::run_overlay(state));
    }

    crate::vlog!(
        "GameOverlay 1.5 started · targets {} · autostart {} · verbose {}",
        opts.targets.len(),
        state.autostart.load(Ordering::Relaxed),
        opts.verbose
    );

    ui::run_panel(state.clone(), opts.tray_quiet);

    state.should_quit.store(true, Ordering::Relaxed);
    fps::shutdown();
}
