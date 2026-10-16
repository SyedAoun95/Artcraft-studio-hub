//! Studio Hub: one desktop hub for the seven Craft apps and one MCP gateway for AI agents.
//!
//!   studio-hub                 open the hub window
//!   studio-hub mcp [--full]    MCP server on stdin/stdout (what AI tools run)
//!   studio-hub status          app status as JSON
//!   studio-hub open <app>      open an app with its agent channel

// Windows: a GUI program, so opening the hub shows no console window. `mcp` still talks over the pipes
// the AI tool gives it; `status` and `open` attach to the terminal they were typed in.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod activity;
mod apps;
mod config;
mod connect;
mod gateway;
mod ui;

use serde_json::json;

fn main() {
    // Finder may pass a "-psn_…" process-serial argument when opening the .app; it is not a command.
    let args: Vec<String> = std::env::args().skip(1).filter(|a| !a.starts_with("-psn_")).collect();
    #[cfg(windows)]
    if !matches!(args.first().map(String::as_str), None | Some("mcp")) {
        use windows_sys::Win32::System::Console::{AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE};
        // Output already redirected (a pipe or file) is kept; otherwise print to the parent's console.
        // SAFETY: plain Win32 calls; AttachConsole fails harmlessly when there is no parent console.
        unsafe {
            if GetStdHandle(STD_OUTPUT_HANDLE).is_null() {
                AttachConsole(ATTACH_PARENT_PROCESS);
            }
        }
    }
    match args.first().map(String::as_str) {
        None => {
            if let Err(e) = ui::run() {
                eprintln!("studio-hub: {e}");
                std::process::exit(1);
            }
        }
        Some("mcp") => gateway::run(args.iter().any(|a| a == "--full")),
        Some("status") => {
            let cfg = config::Config::load();
            let st = apps::statuses(&cfg);
            let v: Vec<_> = apps::APPS.iter().zip(st).map(|(a, s)| json!({"app": a.id, "installed": s.installed,
                "window_open": s.window_open, "agent_ready": s.agent_ready, "pids": s.pids})).collect();
            println!("{}", serde_json::to_string_pretty(&json!({"config": cfg, "apps": v})).unwrap_or_default());
        }
        Some("open") => {
            let cfg = config::Config::load();
            let Some(app) = args.get(1).and_then(|a| apps::find(a)) else {
                eprintln!("usage: studio-hub open <photocraft|vectorcraft|lightcraft|designcraft|filmcraft|effectcraft|pdfcraft>");
                std::process::exit(2);
            };
            match apps::launch(app, &cfg) {
                Ok(m) => println!("{}: {m}", app.name),
                Err(e) => {
                    eprintln!("{}: {e}", app.name);
                    std::process::exit(1);
                }
            }
        }
        Some(other) => {
            eprintln!("studio-hub: unknown command `{other}` (use: mcp, status, open <app>, or no argument for the window)");
            std::process::exit(2);
        }
    }
}
