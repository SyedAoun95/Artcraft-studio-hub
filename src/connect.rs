//! Register the gateway with AI tools. Each writer backs up the file it changes and only touches the
//! `studio-hub` entry. Paths are relative to a home directory so tests can use a throwaway one.

use crate::config::SERVER_NAME;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Client {
    ClaudeCode,
    Codex,
    ClaudeDesktop,
}

pub const CLIENTS: [Client; 3] = [Client::ClaudeCode, Client::Codex, Client::ClaudeDesktop];

impl Client {
    pub fn label(&self) -> &'static str {
        match self {
            Client::ClaudeCode => "Claude Code",
            Client::Codex => "Codex",
            Client::ClaudeDesktop => "Claude Desktop",
        }
    }
}

/// Is this copy of Studio Hub running from a mounted disk image (the .dmg window) rather than installed?
pub fn running_from_disk_image() -> bool {
    cfg!(target_os = "macos") && std::env::current_exe().map(|p| p.starts_with("/Volumes/")).unwrap_or(false)
}

/// Where the installer puts studio-hub on this system (used only when the running copy can't be named).
fn default_install() -> String {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| crate::config::home().join("AppData/Local"))
            .join("Programs/Studio Hub/studio-hub.exe").display().to_string()
    } else {
        "/Applications/Studio Hub.app/Contents/MacOS/studio-hub".into()
    }
}

/// The command an AI tool runs to start the gateway. Always the installed copy: a copy running from the
/// disk image would disappear when the image is ejected, so connections point at Applications instead.
pub fn gateway_command() -> Result<(String, Vec<String>), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if running_from_disk_image() {
        let installed = [PathBuf::from("/Applications"), crate::config::home().join("Applications")]
            .into_iter()
            .map(|d| d.join("Studio Hub.app/Contents/MacOS/studio-hub"))
            .find(|p| p.is_file())
            .ok_or("Studio Hub is running from the disk image. Drag it into Applications, open it from there, and connect again.")?;
        return Ok((installed.display().to_string(), vec!["mcp".into()]));
    }
    Ok((exe.display().to_string(), vec!["mcp".into()]))
}

/// Generic snippet for any other MCP client (Cursor, Windsurf, VS Code, …).
pub fn generic_snippet() -> String {
    let (cmd, args) = gateway_command().unwrap_or_else(|_| (default_install(), vec!["mcp".into()]));
    serde_json::to_string_pretty(&json!({"mcpServers": {SERVER_NAME: {"command": cmd, "args": args}}})).unwrap_or_default()
}

/// `claude` is often not on a Finder-launched app's PATH, so look in the usual install places too.
#[cfg(not(windows))]
fn claude_bin(home: &Path) -> Option<PathBuf> {
    let mut c: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).map(|d| d.join("claude")).collect())
        .unwrap_or_default();
    c.extend([home.join(".local/bin/claude"), home.join(".claude/local/claude"),
              PathBuf::from("/usr/local/bin/claude"), PathBuf::from("/opt/homebrew/bin/claude")]);
    c.into_iter().find(|p| p.is_file())
}

/// Windows: the native installer's claude.exe, or npm's claude.cmd shim.
#[cfg(windows)]
fn claude_bin(home: &Path) -> Option<PathBuf> {
    let mut c: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).flat_map(|d| [d.join("claude.exe"), d.join("claude.cmd")]).collect())
        .unwrap_or_default();
    c.extend([home.join(".local/bin/claude.exe"), home.join(".claude/local/claude.exe"),
              home.join("AppData/Roaming/npm/claude.cmd")]);
    c.into_iter().find(|p| p.is_file())
}

/// Run `claude …` without a console window flashing up from the (GUI) hub.
fn claude_cmd(bin: &Path) -> Command {
    #[allow(unused_mut)]
    let mut c = Command::new(bin);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(crate::apps::win::NO_WINDOW);
    }
    c
}

fn codex_path(home: &Path) -> PathBuf {
    home.join(".codex/config.toml")
}

/// Claude Desktop's settings folder: ~/Library/Application Support/Claude, or %APPDATA%\Claude on Windows.
fn desktop_dir(home: &Path) -> PathBuf {
    if cfg!(windows) { home.join("AppData/Roaming/Claude") } else { home.join("Library/Application Support/Claude") }
}

fn desktop_path(home: &Path) -> PathBuf {
    desktop_dir(home).join("claude_desktop_config.json")
}

fn backup(path: &Path) -> std::io::Result<()> {
    if path.is_file() {
        std::fs::copy(path, path.with_extension(format!("{}.studio-hub-backup",
            path.extension().and_then(|e| e.to_str()).unwrap_or("bak"))))?;
    }
    Ok(())
}

fn atomic_write(path: &Path, data: &[u8]) -> std::io::Result<()> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("studio-hub-tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(tmp, path)
}

/// Is the client installed on this machine at all?
pub fn available(c: Client, home: &Path) -> bool {
    match c {
        Client::ClaudeCode => claude_bin(home).is_some(),
        Client::Codex => home.join(".codex").is_dir(),
        Client::ClaudeDesktop => desktop_dir(home).is_dir(),
    }
}

/// The command the client is set up to run for `studio-hub`, if it is set up at all.
pub fn registered_command(c: Client, home: &Path) -> Option<String> {
    match c {
        Client::ClaudeCode => {
            // Reading ~/.claude.json avoids spawning `claude mcp get`, which is slow.
            let j: Value = std::fs::read(home.join(".claude.json")).ok().and_then(|b| serde_json::from_slice(&b).ok())?;
            let e = j.pointer(&format!("/mcpServers/{SERVER_NAME}"))?;
            Some(e.get("command").and_then(Value::as_str).unwrap_or("").to_string())
        }
        Client::Codex => {
            let d = std::fs::read_to_string(codex_path(home)).ok()?.parse::<toml_edit::DocumentMut>().ok()?;
            let e = d.get("mcp_servers")?.get(SERVER_NAME)?;
            Some(e.get("command").and_then(|v| v.as_str()).unwrap_or("").to_string())
        }
        Client::ClaudeDesktop => {
            let j: Value = serde_json::from_slice(&std::fs::read(desktop_path(home)).ok()?).ok()?;
            let e = j.pointer(&format!("/mcpServers/{SERVER_NAME}"))?;
            Some(e.get("command").and_then(Value::as_str).unwrap_or("").to_string())
        }
    }
}

#[cfg(test)]
pub fn is_connected(c: Client, home: &Path) -> bool {
    registered_command(c, home).is_some()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Link {
    NotConnected,
    Connected,
    /// Set up, but pointing at a copy of Studio Hub that is gone, on a disk image, or not this one.
    Stale,
}

pub fn link(c: Client, home: &Path) -> Link {
    match registered_command(c, home) {
        None => Link::NotConnected,
        Some(cmd) => {
            let target_ok = Path::new(&cmd).is_file() && !cmd.starts_with("/Volumes/");
            let same = gateway_command().map(|(want, _)| want == cmd).unwrap_or(true);
            if target_ok && same { Link::Connected } else { Link::Stale }
        }
    }
}

pub fn connect(c: Client, home: &Path) -> Result<String, String> {
    let (cmd, args) = gateway_command()?;
    match c {
        Client::ClaudeCode => {
            let bin = claude_bin(home).ok_or("Claude Code (the `claude` command) was not found")?;
            let _ = claude_cmd(&bin).args(["mcp", "remove", SERVER_NAME, "-s", "user"]).output();
            let mut a = vec!["mcp".to_string(), "add".into(), "--scope".into(), "user".into(), SERVER_NAME.into(), "--".into(), cmd];
            a.extend(args);
            let out = claude_cmd(&bin).args(&a).output().map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
            }
            Ok("Added for all projects. Start a new Claude Code session to use it.".into())
        }
        Client::Codex => {
            let path = codex_path(home);
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| format!("{} is not valid TOML: {e}", path.display()))?;
            if doc.get("mcp_servers").is_none() {
                let mut t = toml_edit::Table::new();
                t.set_implicit(true);
                doc["mcp_servers"] = toml_edit::Item::Table(t);
            }
            let mut entry = toml_edit::Table::new();
            entry["command"] = toml_edit::value(cmd);
            let mut arr = toml_edit::Array::new();
            for a in args {
                arr.push(a);
            }
            entry["args"] = toml_edit::value(arr);
            entry["startup_timeout_sec"] = toml_edit::value(30);
            entry["tool_timeout_sec"] = toml_edit::value(300);
            doc["mcp_servers"][SERVER_NAME] = toml_edit::Item::Table(entry);
            backup(&path).map_err(|e| e.to_string())?;
            atomic_write(&path, doc.to_string().as_bytes()).map_err(|e| e.to_string())?;
            Ok("Added to ~/.codex/config.toml. Start a new Codex session to use it.".into())
        }
        Client::ClaudeDesktop => {
            let path = desktop_path(home);
            let mut j: Value = match std::fs::read(&path) {
                Ok(b) => serde_json::from_slice(&b).map_err(|e| format!("{} is not valid JSON: {e}", path.display()))?,
                Err(_) => json!({}),
            };
            if !j.is_object() {
                return Err("Claude Desktop's config is not a JSON object".into());
            }
            if !j.get("mcpServers").is_some_and(Value::is_object) {
                j["mcpServers"] = json!({});
            }
            j["mcpServers"][SERVER_NAME] = json!({"command": cmd, "args": args});
            backup(&path).map_err(|e| e.to_string())?;
            atomic_write(&path, serde_json::to_string_pretty(&j).unwrap_or_default().as_bytes()).map_err(|e| e.to_string())?;
            Ok("Added. Quit and reopen Claude Desktop to use it.".into())
        }
    }
}

pub fn disconnect(c: Client, home: &Path) -> Result<String, String> {
    match c {
        Client::ClaudeCode => {
            let bin = claude_bin(home).ok_or("Claude Code was not found")?;
            let out = claude_cmd(&bin).args(["mcp", "remove", SERVER_NAME, "-s", "user"]).output().map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
            }
            Ok("Removed from Claude Code.".into())
        }
        Client::Codex => {
            let path = codex_path(home);
            let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let mut doc: toml_edit::DocumentMut = text.parse().map_err(|e| format!("{e}"))?;
            if let Some(t) = doc.get_mut("mcp_servers").and_then(|m| m.as_table_like_mut()) {
                t.remove(SERVER_NAME);
            }
            backup(&path).map_err(|e| e.to_string())?;
            atomic_write(&path, doc.to_string().as_bytes()).map_err(|e| e.to_string())?;
            Ok("Removed from Codex.".into())
        }
        Client::ClaudeDesktop => {
            let path = desktop_path(home);
            let mut j: Value = serde_json::from_slice(&std::fs::read(&path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
            if let Some(m) = j.get_mut("mcpServers").and_then(Value::as_object_mut) {
                m.remove(SERVER_NAME);
            }
            backup(&path).map_err(|e| e.to_string())?;
            atomic_write(&path, serde_json::to_string_pretty(&j).unwrap_or_default().as_bytes()).map_err(|e| e.to_string())?;
            Ok("Removed from Claude Desktop. Restart it to apply.".into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_home(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("studio-hub-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn codex_keeps_existing_settings() {
        let home = tmp_home("codex");
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        let original = "model = \"gpt-5\"\n# my comment\n\n[mcp_servers.other]\ncommand = \"x\"\n";
        std::fs::write(home.join(".codex/config.toml"), original).unwrap();
        connect(Client::Codex, &home).unwrap();
        let t = std::fs::read_to_string(home.join(".codex/config.toml")).unwrap();
        assert!(t.contains("model = \"gpt-5\"") && t.contains("# my comment") && t.contains("[mcp_servers.other]"));
        assert!(t.contains("[mcp_servers.studio-hub]"), "{t}");
        assert!(is_connected(Client::Codex, &home));
        assert_eq!(std::fs::read_to_string(home.join(".codex/config.toml.studio-hub-backup")).unwrap(), original);
        disconnect(Client::Codex, &home).unwrap();
        assert!(!is_connected(Client::Codex, &home));
        assert!(std::fs::read_to_string(home.join(".codex/config.toml")).unwrap().contains("[mcp_servers.other]"));
    }

    #[test]
    fn claude_desktop_keeps_other_servers() {
        let home = tmp_home("desktop");
        let dir = desktop_dir(&home);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("claude_desktop_config.json"), r#"{"mcpServers":{"other":{"command":"x"}},"theme":"dark"}"#).unwrap();
        connect(Client::ClaudeDesktop, &home).unwrap();
        let j: Value = serde_json::from_slice(&std::fs::read(dir.join("claude_desktop_config.json")).unwrap()).unwrap();
        assert_eq!(j["theme"], "dark");
        assert!(j["mcpServers"]["other"].is_object() && j["mcpServers"]["studio-hub"]["args"][0] == "mcp");
        disconnect(Client::ClaudeDesktop, &home).unwrap();
        assert!(!is_connected(Client::ClaudeDesktop, &home));
    }

    #[test]
    fn connection_to_a_disk_image_or_missing_copy_is_stale() {
        let home = tmp_home("stale");
        let dir = desktop_dir(&home);
        std::fs::create_dir_all(&dir).unwrap();
        for cmd in ["/Volumes/Studio Suite 1/Studio Hub.app/Contents/MacOS/studio-hub", "/nowhere/studio-hub"] {
            let j = json!({"mcpServers": {"studio-hub": {"command": cmd, "args": ["mcp"]}}});
            std::fs::write(dir.join("claude_desktop_config.json"), j.to_string()).unwrap();
            assert_eq!(link(Client::ClaudeDesktop, &home), Link::Stale, "{cmd}");
        }
        connect(Client::ClaudeDesktop, &home).unwrap();
        assert_eq!(link(Client::ClaudeDesktop, &home), Link::Connected);
    }

    #[test]
    fn claude_desktop_created_when_missing() {
        let home = tmp_home("desktop-new");
        connect(Client::ClaudeDesktop, &home).unwrap();
        assert!(is_connected(Client::ClaudeDesktop, &home));
    }
}
