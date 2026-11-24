use git_forker::tr;
use std::path::Path;
use std::process::Command;

/// 设置里可以直接选择的终端。macOS 上是应用名，其他系统上是命令名。
pub const TERMINAL_PRESETS: &[&str] = if cfg!(target_os = "macos") {
    &[
        "Terminal",
        "iTerm",
        "Warp",
        "Ghostty",
        "WezTerm",
        "Alacritty",
        "kitty",
    ]
} else if cfg!(target_os = "windows") {
    &["wt", "powershell", "cmd"]
} else {
    &[
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "tilix",
        "kitty",
        "alacritty",
        "xterm",
    ]
};

/// 在终端中打开目录。`terminal` 为空时使用系统默认终端。
pub fn open_terminal(dir: &Path, terminal: &str) -> Result<(), String> {
    let terminal = terminal.trim();
    #[cfg(target_os = "macos")]
    let result = Command::new("open")
        .arg("-a")
        .arg(if terminal.is_empty() {
            "Terminal"
        } else {
            terminal
        })
        .arg(dir)
        .spawn();
    #[cfg(not(target_os = "macos"))]
    if !terminal.is_empty() {
        return Command::new(terminal)
            .current_dir(dir)
            .spawn()
            .map(|_| ())
            .map_err(|err| tr!("无法打开终端: {err}", err = err));
    }
    #[cfg(target_os = "windows")]
    let result = Command::new("cmd")
        .args(["/C", "start", "", "cmd", "/K"])
        .current_dir(dir)
        .spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = {
        let candidates = [
            "x-terminal-emulator",
            "gnome-terminal",
            "konsole",
            "xfce4-terminal",
            "xterm",
        ];
        candidates
            .iter()
            .find_map(|program| Command::new(program).current_dir(dir).spawn().ok())
            .ok_or_else(|| std::io::Error::other(tr!("没有找到可用的终端程序")))
    };
    result
        .map(|_| ())
        .map_err(|err| tr!("无法打开终端: {err}", err = err))
}

/// 在文件管理器中显示文件或目录。
pub fn reveal(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let result = if path.is_dir() {
        Command::new("open").arg(path).spawn()
    } else {
        Command::new("open").arg("-R").arg(path).spawn()
    };
    #[cfg(target_os = "windows")]
    let result = if path.is_dir() {
        Command::new("explorer").arg(path).spawn()
    } else {
        Command::new("explorer")
            .arg(format!("/select,{}", path.display()))
            .spawn()
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = {
        let target = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        Command::new("xdg-open").arg(target).spawn()
    };
    result
        .map(|_| ())
        .map_err(|err| tr!("无法打开文件管理器: {err}", err = err))
}

/// 用系统默认浏览器打开链接。
pub fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = Command::new("explorer").arg(url).spawn();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = Command::new("xdg-open").arg(url).spawn();
    result
        .map(|_| ())
        .map_err(|err| tr!("无法打开链接: {err}", err = err))
}
