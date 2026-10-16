//! The seven apps: how to start each one with its agent control channel, how to start its own MCP
//! server, and how to tell whether it is running. The apps themselves are never modified.

use crate::config::{ensure_dir, logs_dir, Config};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub struct App {
    pub id: &'static str,
    pub name: &'static str,
    pub like: &'static str,
    pub what: &'static str,
    /// Two-word label shown under the icon in the launcher.
    pub short: &'static str,
    /// Loopback port of the live app's control channel; None when the agent runs headless.
    pub port: Option<u16>,
}

pub const APPS: [App; 7] = [
    App { id: "photocraft", name: "PhotoCraft", short: "Photo editor", like: "Photoshop", what: "Photo editing, layers, retouching", port: Some(7878) },
    App { id: "vectorcraft", name: "VectorCraft", short: "Vector design", like: "Illustrator", what: "Vector graphics and logos", port: Some(7979) },
    App { id: "lightcraft", name: "LightCraft", short: "Photo library", like: "Lightroom", what: "Photo library and RAW development", port: Some(7980) },
    App { id: "designcraft", name: "DesignCraft", short: "Page layout", like: "InDesign", what: "Page layout and print", port: Some(7981) },
    App { id: "filmcraft", name: "FilmCraft", short: "Video editor", like: "Premiere Pro", what: "Video editing", port: Some(9876) },
    App { id: "effectcraft", name: "EffectCraft", short: "Motion graphics", like: "After Effects", what: "Motion graphics and compositing", port: Some(9877) },
    App { id: "pdfcraft", name: "PdfCraft", short: "PDF editor", like: "Acrobat Pro", what: "PDF editing, forms, OCR, accessibility", port: None },
];

/// Where the apps live. `Bundle` is the installed macOS suite: Studio Hub.app carries the seven CLIs in
/// Contents/Helpers and the apps are normal .app bundles (next to it, in /Applications or ~/Applications).
/// `Folder` is the installed Windows suite: `<install>\studio-hub.exe` and `<install>\apps\<id>\<id>.exe`
/// (+ `<id>-cli.exe`). `Dev` is the source layout: `<apps_root>/<id>/target/release/<id>`.
#[derive(Clone, Debug)]
pub enum Layout {
    Dev,
    Bundle { helpers: PathBuf, search: Vec<PathBuf> },
    Folder { apps: PathBuf },
}

const EXE: &str = std::env::consts::EXE_SUFFIX;

pub fn layout() -> &'static Layout {
    static L: std::sync::OnceLock<Layout> = std::sync::OnceLock::new();
    L.get_or_init(|| {
        if cfg!(windows) {
            // No canonicalize here: on Windows it returns a \\?\ path that matches nothing else.
            let apps = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("apps")));
            return match apps {
                Some(a) if a.join("photocraft").join(format!("photocraft-cli{EXE}")).is_file() => Layout::Folder { apps: a },
                _ => Layout::Dev,
            };
        }
        let exe = std::env::current_exe().ok().and_then(|p| p.canonicalize().ok());
        let contents = exe.as_ref().and_then(|e| e.parent()).and_then(|m| m.parent()).map(|c| c.to_path_buf());
        if let Some(c) = contents {
            let helpers = c.join("Helpers");
            if helpers.join("photocraft-cli").is_file() {
                let mut search = Vec::new();
                if let Some(next_to_hub) = c.parent().and_then(|app| app.parent()) {
                    search.push(next_to_hub.to_path_buf());
                }
                search.push(PathBuf::from("/Applications"));
                search.push(crate::config::home().join("Applications"));
                return Layout::Bundle { helpers, search };
            }
        }
        Layout::Dev
    })
}

pub fn find(id: &str) -> Option<&'static App> {
    let id = id.trim().to_ascii_lowercase();
    APPS.iter().find(|a| a.id == id || a.name.to_ascii_lowercase() == id)
}

impl App {
    /// The installed app (`<Name>.app` on macOS, `apps\<id>` on Windows), when running as the installed suite.
    pub fn bundle(&self) -> Option<PathBuf> {
        match layout() {
            Layout::Bundle { search, .. } => search.iter().map(|d| d.join(format!("{}.app", self.name))).find(|p| p.is_dir()),
            Layout::Folder { apps } => Some(apps.join(self.id)).filter(|p| p.is_dir()),
            Layout::Dev => None,
        }
    }
    /// Working directory for the app and its CLI.
    pub fn dir(&self, cfg: &Config) -> PathBuf {
        match layout() {
            Layout::Dev => cfg.apps_root.join(self.id),
            Layout::Bundle { helpers, .. } => helpers.clone(),
            Layout::Folder { apps } => apps.join(self.id),
        }
    }
    pub fn binary(&self, cfg: &Config) -> PathBuf {
        match layout() {
            Layout::Dev => cfg.apps_root.join(self.id).join("target/release").join(format!("{}{EXE}", self.id)),
            Layout::Bundle { .. } => self.bundle().map(|b| b.join("Contents/MacOS").join(self.name))
                .unwrap_or_else(|| PathBuf::from(format!("/Applications/{}.app/Contents/MacOS/{}", self.name, self.name))),
            Layout::Folder { apps } => apps.join(self.id).join(format!("{}{EXE}", self.id)),
        }
    }
    pub fn cli(&self, cfg: &Config) -> PathBuf {
        match layout() {
            Layout::Dev => cfg.apps_root.join(self.id).join("target/release").join(format!("{}-cli{EXE}", self.id)),
            Layout::Bundle { helpers, .. } => helpers.join(format!("{}-cli", self.id)),
            Layout::Folder { apps } => apps.join(self.id).join(format!("{}-cli{EXE}", self.id)),
        }
    }
    pub fn installed(&self, cfg: &Config) -> bool {
        self.binary(cfg).is_file() && self.cli(cfg).is_file()
    }

    /// Arguments that open the app with its loopback control channel (as each app's docs describe).
    pub fn launch_args(&self, cfg: &Config) -> Vec<String> {
        let work = cfg.work_dir.display().to_string();
        let tok = |f: &str| cfg.tokens_dir.join(f).display().to_string();
        let port = self.port.map(|p| p.to_string()).unwrap_or_default();
        let v: Vec<String> = match self.id {
            "photocraft" => vec!["--control".into(), port, "--control-token-file".into(), tok("photocraft-control.token"),
                                 "--automation-read-root".into(), work.clone(), "--automation-write-root".into(), work],
            "vectorcraft" => vec!["--control".into(), port, "--automation-read-root".into(), work.clone(),
                                  "--automation-write-root".into(), work],
            "pdfcraft" => vec!["--control".into(), tok("pdfcraft-control.json")],
            _ => vec!["--control".into(), port],
        };
        v
    }

    /// Arguments for the app's own MCP server (stdio), which the gateway runs as a child process.
    pub fn mcp_args(&self, cfg: &Config) -> Vec<String> {
        let port = self.port.unwrap_or(0);
        let tok = cfg.tokens_dir.join("photocraft-control.token").display().to_string();
        match self.id {
            "photocraft" => vec!["mcp".into(), "--bridge".into(), format!("127.0.0.1:{port}"), "--control-token-file".into(), tok],
            "filmcraft" => vec!["mcp".into(), "--bridge".into(), format!("127.0.0.1:{port}")],
            "effectcraft" => vec!["mcp".into(), "--bridge".into(), port.to_string()],
            "designcraft" => vec!["mcp".into(), "--connect".into(), port.to_string()],
            "pdfcraft" => vec!["mcp".into(), "--root".into(), cfg.work_dir.display().to_string()],
            _ => vec!["mcp".into(), "--connect".into(), format!("127.0.0.1:{port}")],
        }
    }

    /// The agent channel is reachable: the control port answers (PdfCraft's agent is headless: always).
    pub fn agent_ready(&self) -> bool {
        match self.port {
            Some(p) => port_open(p),
            None => true,
        }
    }
}

/// Bring an open, installed app to the front (Dev builds have no bundle to activate).
#[cfg(not(windows))]
pub fn show(app: &App) -> Result<(), String> {
    let bundle = app.bundle().ok_or("only installed apps can be brought to the front")?;
    Command::new("/usr/bin/open").arg("-a").arg(bundle).status().map_err(|e| e.to_string())
        .and_then(|s| if s.success() { Ok(()) } else { Err(format!("open exited with {s}")) })
}

/// Bring an open, installed app to the front: restore and focus its top-level windows.
#[cfg(windows)]
pub fn show(app: &App) -> Result<(), String> {
    app.bundle().ok_or("only installed apps can be brought to the front")?;
    let cfg = Config::load();
    let pids = statuses(&cfg).into_iter().zip(APPS.iter()).find(|(_, a)| a.id == app.id).map(|(s, _)| s.pids).unwrap_or_default();
    if win::for_each_window(&pids, win::bring_to_front) == 0 {
        return Err(format!("{} has no window to bring to the front", app.name));
    }
    Ok(())
}

/// The hub's own copy of the app is running and its agent channel answers. (A port that answers is not
/// enough: another copy of the app, e.g. a development build, may be holding it with other credentials.)
pub fn live(app: &App, cfg: &Config) -> bool {
    match app.port {
        None => app.installed(cfg),
        Some(_) => statuses(cfg).into_iter().zip(APPS.iter()).any(|(s, a)| a.id == app.id && s.agent_ready),
    }
}

/// An installed app that is open without its agent channel (started from Launchpad or Finder):
/// ask it to quit the normal Mac way (it can still ask to save changes), wait, then reopen it with
/// the channel. Fails without touching anything if it does not quit within a minute.
pub fn reopen_with_agent(app: &App, cfg: &Config) -> Result<&'static str, String> {
    ask_to_quit(app, cfg)?;
    let start = Instant::now();
    loop {
        let open = statuses(cfg).into_iter().zip(APPS.iter()).any(|(s, a)| a.id == app.id && s.window_open);
        if !open {
            break;
        }
        if start.elapsed() > Duration::from_secs(60) {
            return Err(format!("{} did not quit (it may be waiting for you to save a document)", app.name));
        }
        std::thread::sleep(Duration::from_millis(400));
    }
    launch(app, cfg).map_err(|e| e.to_string())
}

/// Ask the app to quit the way its own Quit does, so it can still ask to save changes.
#[cfg(not(windows))]
fn ask_to_quit(app: &App, _cfg: &Config) -> Result<(), String> {
    let bundle = app.bundle().ok_or("only installed apps can be reopened from Studio Hub")?;
    let out = Command::new("/usr/bin/plutil").args(["-extract", "CFBundleIdentifier", "raw", "-o", "-"])
        .arg(bundle.join("Contents/Info.plist")).output().map_err(|e| e.to_string())?;
    let bundle_id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if bundle_id.is_empty() {
        return Err(format!("could not read {}'s bundle identifier", app.name));
    }
    let script = format!("tell application id \"{bundle_id}\" to quit");
    Command::new("/usr/bin/osascript").args(["-e", &script]).output().map_err(|e| e.to_string())?;
    Ok(())
}

/// Windows: send WM_CLOSE to the app's windows (what clicking the window's X does).
#[cfg(windows)]
fn ask_to_quit(app: &App, cfg: &Config) -> Result<(), String> {
    app.bundle().ok_or("only installed apps can be reopened from Studio Hub")?;
    let pids = statuses(cfg).into_iter().zip(APPS.iter()).find(|(_, a)| a.id == app.id).map(|(s, _)| s.pids).unwrap_or_default();
    if win::for_each_window(&pids, win::close) == 0 {
        return Err(format!("{} has no window to close", app.name));
    }
    Ok(())
}

pub fn port_open(port: u16) -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&addr, Duration::from_millis(250)).is_ok()
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub installed: bool,
    pub window_open: bool,
    pub agent_ready: bool,
    pub pids: Vec<u32>,
}

/// The running processes as (pid, command line). macOS/Linux: one `ps` call.
#[cfg(not(windows))]
fn processes() -> Vec<(u32, String)> {
    let ps = Command::new("/bin/ps").args(["-axo", "pid=,command="]).output().map(|o| o.stdout).unwrap_or_default();
    String::from_utf8_lossy(&ps)
        .lines()
        .filter_map(|l| {
            let (pid, cmd) = l.trim_start().split_once(' ')?;
            Some((pid.parse().ok()?, cmd.trim_start().to_string()))
        })
        .collect()
}

/// Is `cmd` (a process's command line, or its image path on Windows) our binary at `bin`?
fn runs_binary(cmd: &str, bin: &str) -> bool {
    if cfg!(windows) {
        // Windows paths are case-insensitive; the image path has no arguments.
        return cmd.eq_ignore_ascii_case(bin);
    }
    cmd.strip_prefix(bin).is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
}

/// Which of our binaries are running (matched by full path).
pub fn statuses(cfg: &Config) -> Vec<Status> {
    #[cfg(windows)]
    let procs = win::processes();
    #[cfg(not(windows))]
    let procs = processes();
    APPS.iter()
        .map(|a| {
            let bin = a.binary(cfg).display().to_string();
            let pids: Vec<u32> = procs.iter().filter(|(_, cmd)| runs_binary(cmd, &bin)).map(|(pid, _)| *pid).collect();
            let window_open = !pids.is_empty();
            let agent_ready = match a.port {
                Some(p) => window_open && port_open(p),
                None => a.installed(cfg),
            };
            Status { installed: a.installed(cfg), window_open, agent_ready, pids }
        })
        .collect()
}

#[derive(Debug)]
pub enum LaunchError {
    NotInstalled(PathBuf),
    Spawn(std::io::Error),
    PortTimeout(u16),
    OpenWithoutAgent(&'static str),
    PortTaken(&'static str, u16),
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LaunchError::NotInstalled(p) => write!(f, "the app is not installed (missing {})", p.display()),
            LaunchError::Spawn(e) => write!(f, "could not start the app: {e}"),
            LaunchError::PortTaken(n, p) => write!(f, "port {p}, which {n}'s agent channel uses, is taken by another program (possibly another copy of {n}); quit it and try again"),
            LaunchError::OpenWithoutAgent(n) => write!(f, "{n} is already open but was started without its agent channel (for example from Launchpad). Quit {n}, then open it from Studio Hub"),
            LaunchError::PortTimeout(p) => write!(f, "the app started but its agent port {p} did not open within 45 s (see the log in Studio Hub's logs folder)"),
        }
    }
}

/// Open an app with its control channel unless it is already open. The app runs in its own process
/// group, so closing the hub (or the agent session) does not close the app. Waits for the agent port.
pub fn launch(app: &App, cfg: &Config) -> Result<&'static str, LaunchError> {
    if !app.installed(cfg) {
        return Err(LaunchError::NotInstalled(app.binary(cfg)));
    }
    let running = statuses(cfg).into_iter().zip(APPS.iter()).find(|(_, a)| a.id == app.id).map(|(s, _)| s).unwrap_or_default();
    if running.window_open && app.agent_ready() {
        return Ok("already open");
    }
    if running.window_open && app.port.is_some() {
        return Err(LaunchError::OpenWithoutAgent(app.name));
    }
    if let Some(p) = app.port {
        if port_open(p) {
            return Err(LaunchError::PortTaken(app.name, p));
        }
    }
    if !running.window_open {
        let _ = ensure_dir(&cfg.work_dir);
        let _ = ensure_dir(&cfg.tokens_dir);
        let _ = ensure_dir(&logs_dir());
        let log = std::fs::File::create(logs_dir().join(format!("{}.log", app.id))).map_err(LaunchError::Spawn)?;
        let err = log.try_clone().map_err(LaunchError::Spawn)?;
        let mut cmd = match app.bundle() {
            // Installed: `open` starts it as a normal Mac app (Dock icon, menu bar) with our arguments.
            Some(bundle) if cfg!(target_os = "macos") => {
                let mut c = Command::new("/usr/bin/open");
                c.arg("-a").arg(bundle).arg("--args").args(app.launch_args(cfg));
                c
            }
            _ => {
                let mut c = Command::new(app.binary(cfg));
                c.args(app.launch_args(cfg)).current_dir(app.dir(cfg));
                c
            }
        };
        cmd.stdin(Stdio::null()).stdout(log).stderr(err);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(win::DETACHED);
        }
        let mut child = cmd.spawn().map_err(LaunchError::Spawn)?;
        // Reap the process when it exits so no zombie lingers while the hub keeps running.
        std::thread::spawn(move || {
            let _ = child.wait();
        });
    }
    if let Some(p) = app.port {
        let start = Instant::now();
        while !port_open(p) {
            if start.elapsed() > Duration::from_secs(45) {
                return Err(LaunchError::PortTimeout(p));
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    }
    Ok("opened")
}

/// Windows process and window helpers (the macOS side uses `ps`, `open` and AppleScript instead).
#[cfg(windows)]
pub mod win {
    use windows_sys::core::BOOL;
    use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, TRUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, DETACHED_PROCESS,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindow, GetWindowThreadProcessId, IsIconic, IsWindowVisible, PostMessageW, SetForegroundWindow,
        ShowWindow, GW_OWNER, SW_RESTORE, WM_CLOSE,
    };

    /// An app started from the hub outlives the hub and gets no console window.
    pub const DETACHED: u32 = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
    /// A console helper (an app's CLI, `claude`) started from the hub or gateway: no console window flashes up.
    pub const NO_WINDOW: u32 = CREATE_NO_WINDOW;

    /// Every process as (pid, full image path). Processes we may not query (other users, system) are skipped.
    pub fn processes() -> Vec<(u32, String)> {
        let mut out = Vec::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap.is_null() || snap as isize == -1 {
                return out;
            }
            let mut e: PROCESSENTRY32W = std::mem::zeroed();
            e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut ok = Process32FirstW(snap, &mut e);
            while ok != 0 {
                if let Some(path) = image_path(e.th32ProcessID) {
                    out.push((e.th32ProcessID, path));
                }
                ok = Process32NextW(snap, &mut e);
            }
            CloseHandle(snap);
        }
        out
    }

    fn image_path(pid: u32) -> Option<String> {
        if pid == 0 {
            return None;
        }
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return None;
            }
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
            CloseHandle(h);
            (ok != 0).then(|| String::from_utf16_lossy(&buf[..len as usize]))
        }
    }

    struct Search<'a> {
        pids: &'a [u32],
        act: fn(HWND),
        hits: usize,
    }

    unsafe extern "system" fn visit(hwnd: HWND, lp: LPARAM) -> BOOL {
        let s = unsafe { &mut *(lp as *mut Search) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        // Visible top-level windows only (no owner): the app's main window, not its tooltips or popups.
        if s.pids.contains(&pid) && unsafe { IsWindowVisible(hwnd) } != 0 && unsafe { GetWindow(hwnd, GW_OWNER) }.is_null() {
            (s.act)(hwnd);
            s.hits += 1;
        }
        TRUE
    }

    /// Run `act` on each visible top-level window of these processes; returns how many there were.
    pub fn for_each_window(pids: &[u32], act: fn(HWND)) -> usize {
        if pids.is_empty() {
            return 0;
        }
        let mut s = Search { pids, act, hits: 0 };
        unsafe { EnumWindows(Some(visit), &mut s as *mut Search as LPARAM) };
        s.hits
    }

    pub fn bring_to_front(hwnd: HWND) {
        unsafe {
            if IsIconic(hwnd) != 0 {
                ShowWindow(hwnd, SW_RESTORE);
            }
            SetForegroundWindow(hwnd);
        }
    }

    pub fn close(hwnd: HWND) {
        unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
    }
}
