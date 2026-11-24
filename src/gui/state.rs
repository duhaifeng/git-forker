use std::path::PathBuf;

use git_forker::i18n::Language;
use git_forker::tr;
use serde::{Deserialize, Serialize};

/// 最近提交说明最多保留的条数。
const MAX_RECENT_MESSAGES: usize = 20;

/// 跨启动保留的界面状态。
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppState {
    pub repositories: Vec<PathBuf>,
    pub open_tabs: Vec<PathBuf>,
    pub active_tab: usize,
    pub recent_messages: Vec<String>,
    pub language: Language,
    pub appearance: Appearance,
    pub preferences: Preferences,
}

/// 设置窗口里除语言和外观以外的选项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    /// 启动时重新打开上次的仓库标签页。
    pub restore_tabs: bool,
    /// 窗口重新获得焦点时刷新当前仓库。
    pub refresh_on_focus: bool,
    /// git 可执行文件路径；为空时使用 PATH 里的 git。
    pub git_path: String,
    /// 新打开的仓库页默认使用的差异上下文行数。
    pub context_lines: u32,
    pub ignore_whitespace: bool,
    pub wrap_lines: bool,
    /// 按文件类型给差异里的代码上色。
    pub syntax_highlighting: bool,
    /// 「在终端中打开」使用的程序；为空时使用系统默认终端。
    pub terminal: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            restore_tabs: true,
            refresh_on_focus: true,
            git_path: String::new(),
            context_lines: 3,
            ignore_whitespace: false,
            wrap_lines: false,
            syntax_highlighting: true,
            terminal: String::new(),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

impl AppState {
    fn file() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("git-forker").join("state.json"))
    }

    /// 改名之前的配置文件。新文件不存在时从这里读取一次。
    fn legacy_file() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("git-fork").join("state.json"))
    }

    pub fn load() -> Self {
        let bytes = Self::file()
            .and_then(|file| std::fs::read(file).ok())
            .or_else(|| Self::legacy_file().and_then(|file| std::fs::read(file).ok()));
        bytes
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let file = Self::file().ok_or(tr!("找不到配置目录"))?;
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let json = serde_json::to_vec_pretty(self).map_err(|err| err.to_string())?;
        let temp = file.with_extension("json.tmp");
        std::fs::write(&temp, json).map_err(|err| err.to_string())?;
        std::fs::rename(temp, file).map_err(|err| err.to_string())
    }

    pub fn remember_repository(&mut self, path: &PathBuf) {
        if !self.repositories.contains(path) {
            self.repositories.push(path.clone());
        }
    }

    pub fn remember_message(&mut self, message: &str) {
        let message = message.trim();
        if message.is_empty() {
            return;
        }
        self.recent_messages.retain(|existing| existing != message);
        self.recent_messages.insert(0, message.to_string());
        self.recent_messages.truncate(MAX_RECENT_MESSAGES);
    }
}
