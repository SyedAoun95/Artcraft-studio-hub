//! Settings shared by the hub window and the MCP gateway (both read the same file, so a change made
//! in the window - turning agents off, say - applies to the next agent call without a restart).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const PRODUCT: &str = "Studio Hub";
/// Name agents and config files use for the gateway (`claude mcp add <SERVER_NAME> …`).
pub const SERVER_NAME: &str = "studio-hub";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Folder holding one sub-folder per app (`<root>/photocraft/target/release/photocraft`).
    pub apps_root: PathBuf,
    /// The folder agents may read from and write to.
    pub work_dir: PathBuf,
    /// Where control tokens are kept (PhotoCraft creates its token file here, mode 0600).
    pub tokens_dir: PathBuf,
    /// Master switch: when false every agent tool except `suite_status` is refused.
    pub agents_enabled: bool,
    /// App ids agents may not touch even when agents are enabled.
    pub disabled_apps: Vec<String>,
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

pub fn support_dir() -> PathBuf {
    home().join("Library/Application Support/Studio Hub")
}

pub fn config_path() -> PathBuf {
    support_dir().join("config.json")
}

pub fn activity_path() -> PathBuf {
    support_dir().join("activity.jsonl")
}

pub fn logs_dir() -> PathBuf {
    support_dir().join("logs")
}

impl Default for Config {
    fn default() -> Self {
        // Development layout: the hub's folder sits next to the app folders.
        let apps_root = std::env::var_os("STUDIO_HUB_APPS_ROOT")
            .map(PathBuf::from)
            .or_else(guess_apps_root)
            .unwrap_or_else(|| home().join("Applications/Studio Hub Apps"));
        // Reuse an existing ArtCraft setup on this machine so already-running apps keep working.
        let legacy_tokens = home().join(".config/artcraft");
        let tokens_dir = if legacy_tokens.is_dir() { legacy_tokens } else { support_dir().join("tokens") };
        let legacy_work = apps_root.join("craft-work");
        let work_dir = if legacy_work.is_dir() { legacy_work } else { home().join("Documents/Studio Hub") };
        Config { apps_root, work_dir, tokens_dir, agents_enabled: true, disabled_apps: Vec::new() }
    }
}

/// Walk up from the executable looking for a folder that contains the app folders.
fn guess_apps_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent();
    while let Some(d) = dir {
        if d.join("photocraft").is_dir() && d.join("pdfcraft").is_dir() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

impl Config {
    pub fn load() -> Config {
        match std::fs::read(config_path()) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => {
                let c = Config::default();
                let _ = c.save();
                c
            }
        }
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = config_path();
        ensure_dir(path.parent().unwrap_or(Path::new("/")))?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).unwrap_or_default())?;
        std::fs::rename(tmp, path)
    }

    pub fn app_allowed(&self, id: &str) -> bool {
        self.agents_enabled && !self.disabled_apps.iter().any(|d| d == id)
    }
}

pub fn ensure_dir(p: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(p)
}
