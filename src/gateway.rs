//! One MCP server (stdio, newline-delimited JSON-RPC) in front of the seven apps' own MCP servers.
//!
//! Agents see a compact tool set - status, launch, list/describe an app's tools, call one - instead of
//! ~800 tools at once (LightCraft alone lists 502 while its window is open). `--full` additionally
//! lists every tool of every connected app as `<app>__<tool>`.

use crate::activity::{self, Entry};
use crate::apps::{self, App, APPS};
use crate::config::{Config, PRODUCT};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child as Proc, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const SUPPORTED: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
const CALL_TIMEOUT: Duration = Duration::from_secs(300);

/// One running `<app>-cli mcp` child.
struct Child {
    proc: Mutex<Proc>,
    stdin: Mutex<ChildStdin>,
    pending: Mutex<HashMap<u64, Sender<Value>>>,
    next: AtomicU64,
    alive: Arc<AtomicBool>,
    tools: Mutex<Option<Vec<Value>>>,
}

impl Child {
    fn spawn(app: &App, cfg: &Config) -> Result<Arc<Child>, String> {
        // The app's agent is given the work folder (PdfCraft's refuses a --root that does not exist yet).
        let _ = crate::config::ensure_dir(&cfg.work_dir);
        let mut cmd = Command::new(app.cli(cfg));
        cmd.args(app.mcp_args(cfg)).current_dir(app.dir(cfg)).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(apps::win::NO_WINDOW);
        }
        let mut proc = cmd
            .spawn()
            .map_err(|e| format!("could not start {}'s MCP server: {e}", app.name))?;
        let stdin = proc.stdin.take().ok_or("no stdin")?;
        let stdout = proc.stdout.take().ok_or("no stdout")?;
        let stderr = proc.stderr.take().ok_or("no stderr")?;
        let child = Arc::new(Child {
            proc: Mutex::new(proc),
            stdin: Mutex::new(stdin),
            pending: Mutex::new(HashMap::new()),
            next: AtomicU64::new(1),
            alive: Arc::new(AtomicBool::new(true)),
            tools: Mutex::new(None),
        });
        let errbuf = Arc::new(Mutex::new(String::new()));
        {
            let errbuf = errbuf.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    if let Ok(mut b) = errbuf.lock() {
                        if b.len() < 4000 {
                            b.push_str(&line);
                            b.push('\n');
                        }
                    }
                }
            });
        }
        {
            let c = child.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let Ok(msg) = serde_json::from_str::<Value>(&line) else { continue };
                    if let Some(id) = msg.get("id").and_then(Value::as_u64) {
                        if msg.get("method").is_none() {
                            if let Some(tx) = c.pending.lock().ok().and_then(|mut p| p.remove(&id)) {
                                let _ = tx.send(msg);
                            }
                        }
                    }
                    // Notifications and server-to-client requests from the apps are not forwarded (phase 1).
                }
                c.alive.store(false, Ordering::SeqCst);
                let err = errbuf.lock().map(|b| b.trim().to_string()).unwrap_or_default();
                let fail = json!({"error": {"code": -32000, "message": if err.is_empty() { "the app's MCP server exited".to_string() } else { err }}});
                if let Ok(mut p) = c.pending.lock() {
                    for (_, tx) in p.drain() {
                        let _ = tx.send(fail.clone());
                    }
                }
            });
        }
        child.request("initialize", json!({"protocolVersion": SUPPORTED[0], "capabilities": {},
            "clientInfo": {"name": "studio-hub", "version": env!("CARGO_PKG_VERSION")}}), Duration::from_secs(20))?;
        child.notify("notifications/initialized");
        Ok(child)
    }

    fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        if !self.alive.load(Ordering::SeqCst) {
            return Err("the app's MCP server is not running".into());
        }
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = channel();
        self.pending.lock().map_err(|_| "lock")?.insert(id, tx);
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}).to_string();
        {
            let mut w = self.stdin.lock().map_err(|_| "lock")?;
            writeln!(w, "{line}").and_then(|_| w.flush()).map_err(|e| format!("could not reach the app's MCP server: {e}"))?;
        }
        let msg = rx.recv_timeout(timeout).map_err(|_| {
            if let Ok(mut p) = self.pending.lock() {
                p.remove(&id);
            }
            format!("no reply from the app within {} s", timeout.as_secs())
        })?;
        if let Some(e) = msg.get("error") {
            return Err(e.get("message").and_then(Value::as_str).unwrap_or("error").to_string());
        }
        Ok(msg.get("result").cloned().unwrap_or(Value::Null))
    }

    fn notify(&self, method: &str) {
        if let Ok(mut w) = self.stdin.lock() {
            let _ = writeln!(w, "{}", json!({"jsonrpc": "2.0", "method": method}));
            let _ = w.flush();
        }
    }

    fn tools(&self) -> Result<Vec<Value>, String> {
        if let Some(t) = self.tools.lock().map_err(|_| "lock")?.clone() {
            return Ok(t);
        }
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;
        loop {
            let params = match &cursor { Some(c) => json!({"cursor": c}), None => json!({}) };
            let r = self.request("tools/list", params, Duration::from_secs(30))?;
            all.extend(r.get("tools").and_then(Value::as_array).cloned().unwrap_or_default());
            cursor = r.get("nextCursor").and_then(Value::as_str).map(String::from);
            if cursor.is_none() {
                break;
            }
        }
        *self.tools.lock().map_err(|_| "lock")? = Some(all.clone());
        Ok(all)
    }

    fn kill(&self) {
        self.alive.store(false, Ordering::SeqCst);
        if let Ok(mut p) = self.proc.lock() {
            let _ = p.kill();
            let _ = p.wait();
        }
    }
}

struct Gateway {
    full: bool,
    out: Mutex<std::io::Stdout>,
    children: Mutex<HashMap<&'static str, (Arc<Child>, bool)>>, // (child, app port was open when spawned)
    client: Mutex<String>,
}

impl Gateway {
    fn send(&self, v: &Value) {
        if let Ok(mut o) = self.out.lock() {
            let _ = writeln!(o, "{v}");
            let _ = o.flush();
        }
    }

    /// The app's MCP child, (re)started when needed. Apps with a live window must be open first.
    fn child(&self, app: &'static App, cfg: &Config) -> Result<Arc<Child>, String> {
        let live = apps::live(app, cfg);
        if app.port.is_some() && !live {
            return Err(format!("{} is not open. Call suite_launch_app with app \"{}\" first (or open it from {PRODUCT}).", app.name, app.id));
        }
        let mut map = self.children.lock().map_err(|_| "lock")?;
        if let Some((c, was_live)) = map.get(app.id) {
            if c.alive.load(Ordering::SeqCst) && *was_live == live {
                return Ok(c.clone());
            }
            c.kill();
        }
        let c = Child::spawn(app, cfg)?;
        map.insert(app.id, (c.clone(), live));
        Ok(c)
    }

    fn drop_child(&self, app: &App) {
        if let Ok(mut map) = self.children.lock() {
            if let Some((c, _)) = map.remove(app.id) {
                c.kill();
            }
        }
    }

    fn log(&self, app: &str, tool: &str, ok: bool, t0: Instant, note: &str) {
        activity::record(&Entry {
            ts: activity::now(),
            client: self.client.lock().map(|c| c.clone()).unwrap_or_default(),
            app: app.into(),
            tool: tool.into(),
            ok,
            ms: t0.elapsed().as_millis() as u64,
            note: note.chars().take(200).collect(),
        });
    }

    fn suite_tools(&self) -> Vec<Value> {
        let ids: Vec<&str> = APPS.iter().map(|a| a.id).collect();
        let app_prop = json!({"type": "string", "enum": ids, "description": "App id"});
        vec![
            json!({"name": "suite_status", "description": format!("Which {PRODUCT} apps are installed and open, whether each one's agent channel is ready, whether agents are allowed, and the work folder agents can use. Call this first."),
                   "inputSchema": {"type": "object", "properties": {}}, "annotations": {"readOnlyHint": true}}),
            json!({"name": "suite_launch_app", "description": "Open an app with its agent channel (does nothing if it is already open) and wait until agents can drive it. The app's window appears on the user's screen.",
                   "inputSchema": {"type": "object", "properties": {"app": app_prop}, "required": ["app"]}}),
            json!({"name": "suite_list_tools", "description": "List an app's own tools (name and one-line description). Use query to filter by words in the name or description. The app must be open (PdfCraft works without its window).",
                   "inputSchema": {"type": "object", "properties": {"app": app_prop, "query": {"type": "string", "description": "Optional filter, e.g. \"export\" or \"layer\""}}, "required": ["app"]},
                   "annotations": {"readOnlyHint": true}}),
            json!({"name": "suite_describe_tool", "description": "The full description and JSON input schema of one app tool, so you can call it correctly.",
                   "inputSchema": {"type": "object", "properties": {"app": app_prop, "tool": {"type": "string"}}, "required": ["app", "tool"]},
                   "annotations": {"readOnlyHint": true}}),
            json!({"name": "suite_call_tool", "description": "Call one of an app's tools with its arguments and return the app's result unchanged (text, JSON and images such as screenshots).",
                   "inputSchema": {"type": "object", "properties": {"app": app_prop, "tool": {"type": "string"},
                                   "arguments": {"type": "object", "description": "The tool's arguments, as in its input schema"}}, "required": ["app", "tool"]}}),
        ]
    }

    fn handle(&self, msg: Value) {
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("").to_string();
        let params = msg.get("params").cloned().unwrap_or(json!({}));
        let result: Result<Value, (i64, String)> = match method.as_str() {
            "initialize" => {
                let asked = params.get("protocolVersion").and_then(Value::as_str).unwrap_or(SUPPORTED[0]);
                let version = SUPPORTED.iter().find(|v| **v == asked).copied().unwrap_or(SUPPORTED[0]);
                if let Some(name) = params.pointer("/clientInfo/name").and_then(Value::as_str) {
                    if let Ok(mut c) = self.client.lock() {
                        *c = name.to_string();
                    }
                }
                Ok(json!({"protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": self.full}},
                    "serverInfo": {"name": "studio-hub", "title": PRODUCT, "version": env!("CARGO_PKG_VERSION")},
                    "instructions": format!("{PRODUCT} controls seven desktop apps: PhotoCraft (like Photoshop), VectorCraft (Illustrator), LightCraft (Lightroom), DesignCraft (InDesign), FilmCraft (Premiere Pro), EffectCraft (After Effects) and PdfCraft (Acrobat). Workflow: suite_status -> suite_launch_app if the app is closed -> suite_list_tools (with a query) -> suite_describe_tool -> suite_call_tool. Files must be inside the work folder that suite_status reports. The user sees the app windows change as you work.")}))
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": self.list_tools()})),
            "tools/call" => Ok(self.call(&params)),
            "resources/list" => Ok(json!({"resources": []})),
            "resources/templates/list" => Ok(json!({"resourceTemplates": []})),
            "prompts/list" => Ok(json!({"prompts": []})),
            m if m.starts_with("notifications/") => return,
            _ => Err((-32601, format!("method not found: {method}"))),
        };
        if let Some(id) = id {
            match result {
                Ok(r) => self.send(&json!({"jsonrpc": "2.0", "id": id, "result": r})),
                Err((code, m)) => self.send(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": m}})),
            }
        }
    }

    fn list_tools(&self) -> Vec<Value> {
        let mut tools = self.suite_tools();
        if self.full {
            let cfg = Config::load();
            for app in APPS.iter().filter(|a| a.installed(&cfg) && apps::live(a, &cfg)) {
                if let Ok(child) = self.child(app, &cfg) {
                    for mut t in child.tools().unwrap_or_default() {
                        let name = t.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                        let full = format!("{}__{}", app.id, name);
                        if full.len() > 64 {
                            continue;
                        }
                        t["name"] = json!(full);
                        let d = t.get("description").and_then(Value::as_str).unwrap_or("").to_string();
                        t["description"] = json!(format!("[{}] {d}", app.name));
                        tools.push(t);
                    }
                }
            }
        }
        tools
    }

    fn call(&self, params: &Value) -> Value {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        let args = params.get("arguments").cloned().unwrap_or(json!({}));
        let t0 = Instant::now();
        let cfg = Config::load();
        let text_err = |m: String| json!({"content": [{"type": "text", "text": m}], "isError": true});
        let ok_json = |v: Value| json!({"content": [{"type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default()}], "structuredContent": v});

        if name == "suite_status" {
            let st = apps::statuses(&cfg);
            let list: Vec<Value> = APPS.iter().zip(st).map(|(a, s)| json!({
                "app": a.id, "name": a.name, "like": a.like, "installed": s.installed, "window_open": s.window_open,
                "agent_ready": s.agent_ready, "agents_allowed": cfg.app_allowed(a.id),
                "note": if a.port.is_none() { "PdfCraft's agent works on files headlessly; it does not drive the open window" } else { "" }})).collect();
            self.log("suite", name, true, t0, "");
            return ok_json(json!({"agents_enabled": cfg.agents_enabled, "work_folder": cfg.work_dir, "apps": list}));
        }

        let (app_id, tool, inner_args) = if let Some((a, t)) = name.split_once("__") {
            (a.to_string(), t.to_string(), args.clone())
        } else {
            (args.get("app").and_then(Value::as_str).unwrap_or("").to_string(),
             args.get("tool").and_then(Value::as_str).unwrap_or("").to_string(),
             args.get("arguments").cloned().unwrap_or(json!({})))
        };
        let Some(app) = apps::find(&app_id) else {
            return text_err(format!("unknown app \"{app_id}\"; use one of: {}", APPS.iter().map(|a| a.id).collect::<Vec<_>>().join(", ")));
        };
        let shown = if tool.is_empty() { name } else { tool.as_str() };
        if !cfg.agents_enabled {
            self.log(app.id, shown, false, t0, "refused: agents off");
            return text_err(format!("Agent access is turned off in {PRODUCT}. Ask the user to turn it on."));
        }
        if !cfg.app_allowed(app.id) {
            self.log(app.id, shown, false, t0, "refused: app disabled");
            return text_err(format!("The user has turned off agent access for {} in {PRODUCT}.", app.name));
        }

        let result = match name {
            "suite_launch_app" => match apps::launch(app, &cfg) {
                Ok(what) => Ok(ok_json(json!({"app": app.id, "result": what, "agent_ready": apps::live(app, &cfg)}))),
                Err(e) => Err(e.to_string()),
            },
            "suite_list_tools" => self.child(app, &cfg).and_then(|c| c.tools()).map(|tools| {
                let q = args.get("query").and_then(Value::as_str).unwrap_or("").to_lowercase();
                let words: Vec<&str> = q.split_whitespace().collect();
                let list: Vec<Value> = tools.iter().filter(|t| {
                    let hay = format!("{} {}", t.get("name").and_then(Value::as_str).unwrap_or(""), t.get("description").and_then(Value::as_str).unwrap_or("")).to_lowercase();
                    words.iter().all(|w| hay.contains(w))
                }).map(|t| {
                    let d = t.get("description").and_then(Value::as_str).unwrap_or("");
                    let first: String = d.split(". ").next().unwrap_or(d).chars().take(160).collect();
                    json!({"name": t.get("name"), "description": first})
                }).collect();
                ok_json(json!({"app": app.id, "total": tools.len(), "matching": list.len(), "tools": list}))
            }),
            "suite_describe_tool" => self.child(app, &cfg).and_then(|c| c.tools()).and_then(|tools| {
                tools.into_iter().find(|t| t.get("name").and_then(Value::as_str) == Some(tool.as_str()))
                    .map(ok_json).ok_or(format!("{} has no tool \"{tool}\" (use suite_list_tools)", app.name))
            }),
            _ => self.forward(app, &cfg, &tool, inner_args),
        };
        // What the activity log shows: the app tool that ran, or what the agent looked up.
        let label = match name {
            "suite_list_tools" => "list tools".to_string(),
            "suite_describe_tool" => format!("describe {tool}"),
            "suite_launch_app" => "open app".to_string(),
            _ if tool.is_empty() => name.to_string(),
            _ => tool.clone(),
        };
        match result {
            Ok(v) => {
                let is_err = v.get("isError").and_then(Value::as_bool).unwrap_or(false);
                self.log(app.id, &label, !is_err, t0, "");
                v
            }
            Err(e) => {
                self.log(app.id, &label, false, t0, &e);
                text_err(e)
            }
        }
    }

    fn forward(&self, app: &'static App, cfg: &Config, tool: &str, args: Value) -> Result<Value, String> {
        if tool.is_empty() {
            return Err("tool is required".into());
        }
        let params = json!({"name": tool, "arguments": args});
        let child = self.child(app, cfg)?;
        match child.request("tools/call", params.clone(), CALL_TIMEOUT) {
            Ok(v) => Ok(v),
            // The app's MCP process died (e.g. the app was closed and reopened): restart it once and retry.
            Err(e) if !child.alive.load(Ordering::SeqCst) => {
                self.drop_child(app);
                let child = self.child(app, cfg).map_err(|e2| format!("{e}; restart failed: {e2}"))?;
                child.request("tools/call", params, CALL_TIMEOUT)
            }
            Err(e) => Err(e),
        }
    }
}

/// Run the gateway on stdin/stdout until the client closes the stream.
pub fn run(full: bool) {
    let gw = Arc::new(Gateway { full, out: Mutex::new(std::io::stdout()), children: Mutex::new(HashMap::new()), client: Mutex::new(String::new()) });
    let stdin = std::io::stdin();
    for line in stdin.lock().lines().map_while(Result::ok) {
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                gw.send(&json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": format!("parse error: {e}")}}));
                continue;
            }
        };
        let gw = gw.clone();
        // Each request on its own thread: a long export in one app does not block calls to another.
        std::thread::spawn(move || gw.handle(msg));
    }
    if let Ok(map) = gw.children.lock() {
        for (c, _) in map.values() {
            c.kill();
        }
    }
}
