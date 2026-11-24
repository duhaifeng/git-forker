//! 菜单栏的唯一定义。macOS 上渲染成原生菜单，其他平台画在窗口里；
//! 快捷键也从这里分发，避免菜单和快捷键各维护一份。

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers};
use git_forker::HistoryOptions;
use git_forker::tr;

use super::tab::{DetailsTab, View};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Command {
    About,
    Settings,
    Quit,

    NewTab,
    Open,
    QuickLaunch,
    Clone,
    InitRepository,
    OpenRecent(usize),
    CloseTab,

    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Find,

    ShowChanges,
    ShowHistory,
    Details(DetailsTab),
    ToggleDetails,
    ShowHead,
    ToggleTags,
    ToggleRemoteBranches,
    ToggleFirstParent,
    ToggleCurrentBranch,
    ToggleManager,
    ToggleActivity,
    Refresh,
    FullScreen,

    Commit,
    Fetch,
    QuickFetch,
    Pull,
    QuickPull,
    Push,
    QuickPush,
    Stash,
    QuickStash,
    PopStash,
    CheckoutBranch,
    NewBranch,
    NewTag,
    AddRemote,
    ApplyPatch,
    RevealRepository,
    OpenTerminal,
    RepositorySettings,

    Minimize,
    Zoom,

    GitDocs,
    Shortcuts,
}

impl Command {
    /// 文本编辑命令交给输入框处理，不由应用自己消费快捷键。
    pub fn is_text_edit(self) -> bool {
        matches!(
            self,
            Command::Undo
                | Command::Redo
                | Command::Cut
                | Command::Copy
                | Command::Paste
                | Command::SelectAll
        )
    }

    /// 原生菜单里用的稳定 ID。只在 macOS 原生菜单里用到。
    #[cfg(target_os = "macos")]
    pub fn id(self) -> String {
        format!("{self:?}")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    Item {
        command: Command,
        label: String,
        shortcut: Option<KeyboardShortcut>,
        enabled: bool,
        checked: Option<bool>,
    },
    Submenu {
        label: String,
        entries: Vec<Entry>,
    },
    Separator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// macOS 的应用菜单，系统会用应用名做标题。
    App,
    Window,
    Help,
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Menu {
    pub role: Role,
    pub title: String,
    pub entries: Vec<Entry>,
}

/// 生成菜单需要知道的界面状态。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MenuState {
    pub repository: Option<RepositoryState>,
    pub recent: Vec<String>,
    pub show_manager: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RepositoryState {
    pub view: View,
    pub details_tab: DetailsTab,
    pub show_details: bool,
    pub show_log: bool,
    pub options: HistoryOptions,
    pub has_stash: bool,
    pub has_changes: bool,
    pub has_remotes: bool,
    pub on_branch: bool,
}

const MAC: bool = cfg!(target_os = "macos");

fn shortcut(modifiers: Modifiers, key: Key) -> Option<KeyboardShortcut> {
    Some(KeyboardShortcut::new(modifiers, key))
}

fn command(key: Key) -> Option<KeyboardShortcut> {
    shortcut(Modifiers::COMMAND, key)
}

fn command_shift(key: Key) -> Option<KeyboardShortcut> {
    shortcut(Modifiers::COMMAND | Modifiers::SHIFT, key)
}

fn command_alt(key: Key) -> Option<KeyboardShortcut> {
    shortcut(Modifiers::COMMAND | Modifiers::ALT, key)
}

fn command_alt_shift(key: Key) -> Option<KeyboardShortcut> {
    shortcut(Modifiers::COMMAND | Modifiers::ALT | Modifiers::SHIFT, key)
}

/// macOS 上是 ⌃⌘，其他平台上 Ctrl 已经是命令键，所以换成 Ctrl+Alt。
fn command_control(key: Key) -> Option<KeyboardShortcut> {
    if MAC {
        shortcut(Modifiers::COMMAND | Modifiers::CTRL, key)
    } else {
        shortcut(Modifiers::COMMAND | Modifiers::ALT, key)
    }
}

struct Builder {
    entries: Vec<Entry>,
}

impl Builder {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn item(
        mut self,
        command: Command,
        label: impl Into<String>,
        shortcut: Option<KeyboardShortcut>,
        enabled: bool,
    ) -> Self {
        self.entries.push(Entry::Item {
            command,
            label: label.into(),
            shortcut,
            enabled,
            checked: None,
        });
        self
    }

    fn check(
        mut self,
        command: Command,
        label: impl Into<String>,
        shortcut: Option<KeyboardShortcut>,
        enabled: bool,
        checked: bool,
    ) -> Self {
        self.entries.push(Entry::Item {
            command,
            label: label.into(),
            shortcut,
            enabled,
            checked: Some(checked),
        });
        self
    }

    fn submenu(mut self, label: impl Into<String>, entries: Vec<Entry>) -> Self {
        self.entries.push(Entry::Submenu {
            label: label.into(),
            entries,
        });
        self
    }

    fn separator(mut self) -> Self {
        self.entries.push(Entry::Separator);
        self
    }

    fn when(self, condition: bool, add: impl FnOnce(Self) -> Self) -> Self {
        if condition { add(self) } else { self }
    }

    fn build(self) -> Vec<Entry> {
        self.entries
    }
}

pub fn model(state: &MenuState) -> Vec<Menu> {
    let repo = state.repository.as_ref();
    let open = repo.is_some();
    let mut menus = Vec::new();

    if MAC {
        menus.push(Menu {
            role: Role::App,
            title: "git-forker".into(),
            entries: Builder::new()
                .item(Command::About, tr!("关于 git-forker"), None, true)
                .separator()
                .item(Command::Settings, tr!("设置…"), command(Key::Comma), true)
                .build(),
        });
    }

    let recent: Vec<Entry> = state
        .recent
        .iter()
        .take(10)
        .enumerate()
        .map(|(index, name)| Entry::Item {
            command: Command::OpenRecent(index),
            label: name.clone(),
            shortcut: None,
            enabled: true,
            checked: None,
        })
        .collect();
    menus.push(Menu {
        role: Role::Other,
        title: tr!("文件").into(),
        entries: Builder::new()
            .item(Command::NewTab, tr!("新建标签页"), command(Key::T), true)
            .item(Command::Open, tr!("打开仓库…"), command(Key::O), true)
            .item(
                Command::QuickLaunch,
                tr!("快速启动…"),
                command(Key::P),
                open,
            )
            .item(Command::Clone, tr!("克隆…"), command_shift(Key::N), true)
            .item(Command::InitRepository, tr!("新建本地仓库…"), None, true)
            .when(!recent.is_empty(), |menu| {
                menu.separator().submenu(tr!("最近打开"), recent)
            })
            .separator()
            .item(Command::CloseTab, tr!("关闭标签页"), command(Key::W), open)
            .when(!MAC, |menu| {
                menu.separator()
                    .item(Command::Settings, tr!("设置…"), command(Key::Comma), true)
                    .separator()
                    .item(Command::Quit, tr!("退出"), command(Key::Q), true)
            })
            .build(),
    });

    menus.push(Menu {
        role: Role::Other,
        title: tr!("编辑").into(),
        entries: Builder::new()
            .item(Command::Undo, tr!("撤销"), command(Key::Z), true)
            .item(Command::Redo, tr!("重做"), command_shift(Key::Z), true)
            .separator()
            .item(Command::Cut, tr!("剪切"), command(Key::X), true)
            .item(Command::Copy, tr!("拷贝"), command(Key::C), true)
            .item(Command::Paste, tr!("粘贴"), command(Key::V), true)
            .item(Command::SelectAll, tr!("全选"), command(Key::A), true)
            .separator()
            .item(Command::Find, tr!("查找提交…"), command(Key::F), open)
            .build(),
    });

    let view = repo.map(|repo| repo.view);
    let details = repo.map(|repo| repo.details_tab);
    let options = repo.map(|repo| repo.options).unwrap_or_default();
    let history = view == Some(View::History);
    menus.push(Menu {
        role: Role::Other,
        title: tr!("显示").into(),
        entries: Builder::new()
            .check(
                Command::ShowChanges,
                tr!("显示本地更改"),
                command(Key::Num1),
                open,
                view == Some(View::LocalChanges),
            )
            .check(
                Command::ShowHistory,
                tr!("显示全部提交"),
                command(Key::Num2),
                open,
                history,
            )
            .separator()
            .submenu(
                tr!("提交详情"),
                Builder::new()
                    .check(
                        Command::Details(DetailsTab::Commit),
                        tr!("概要"),
                        command_alt(Key::Num1),
                        history,
                        details == Some(DetailsTab::Commit),
                    )
                    .check(
                        Command::Details(DetailsTab::Changes),
                        tr!("更改"),
                        command_alt(Key::Num2),
                        history,
                        details == Some(DetailsTab::Changes),
                    )
                    .check(
                        Command::Details(DetailsTab::Tree),
                        tr!("文件树"),
                        command_alt(Key::Num3),
                        history,
                        details == Some(DetailsTab::Tree),
                    )
                    .separator()
                    .item(
                        Command::ToggleDetails,
                        if repo.is_some_and(|repo| repo.show_details) {
                            tr!("隐藏提交详情")
                        } else {
                            tr!("显示提交详情")
                        },
                        command_shift(Key::D),
                        history,
                    )
                    .build(),
            )
            .separator()
            .item(
                Command::ShowHead,
                tr!("定位到 HEAD"),
                command(Key::Num0),
                open,
            )
            .separator()
            .check(
                Command::ToggleTags,
                tr!("显示标签"),
                None,
                open,
                options.show_tags,
            )
            .check(
                Command::ToggleRemoteBranches,
                tr!("显示远程分支"),
                None,
                open,
                options.show_remotes,
            )
            .check(
                Command::ToggleFirstParent,
                tr!("折叠所有合并（只沿第一父提交）"),
                None,
                open,
                options.first_parent,
            )
            .check(
                Command::ToggleCurrentBranch,
                tr!("只显示当前分支"),
                command_shift(Key::A),
                open,
                options.current_branch_only,
            )
            .separator()
            .check(
                Command::ToggleManager,
                tr!("仓库管理器"),
                None,
                true,
                state.show_manager,
            )
            .check(
                Command::ToggleActivity,
                tr!("活动日志"),
                None,
                open,
                repo.is_some_and(|repo| repo.show_log),
            )
            .separator()
            .item(Command::Refresh, tr!("刷新"), command(Key::R), open)
            .item(
                Command::FullScreen,
                tr!("切换全屏"),
                if MAC {
                    command_control(Key::F)
                } else {
                    shortcut(Modifiers::NONE, Key::F11)
                },
                true,
            )
            .build(),
    });

    let has_remotes = repo.is_some_and(|repo| repo.has_remotes);
    let on_branch = repo.is_some_and(|repo| repo.on_branch);
    let has_changes = repo.is_some_and(|repo| repo.has_changes);
    menus.push(Menu {
        role: Role::Other,
        title: tr!("仓库").into(),
        entries: Builder::new()
            .item(Command::Commit, tr!("提交…"), command_shift(Key::C), open)
            .separator()
            .item(
                Command::Fetch,
                tr!("抓取…"),
                command_shift(Key::F),
                has_remotes,
            )
            .item(
                Command::QuickFetch,
                tr!("快速抓取"),
                command_alt_shift(Key::F),
                has_remotes,
            )
            .item(
                Command::Pull,
                tr!("拉取…"),
                command_shift(Key::L),
                has_remotes,
            )
            .item(
                Command::QuickPull,
                tr!("快速拉取"),
                command_alt_shift(Key::L),
                has_remotes,
            )
            .item(
                Command::Push,
                tr!("推送…"),
                command_shift(Key::P),
                has_remotes && on_branch,
            )
            .item(
                Command::QuickPush,
                tr!("快速推送"),
                command_alt_shift(Key::P),
                has_remotes && on_branch,
            )
            .separator()
            .item(
                Command::Stash,
                tr!("贮藏更改…"),
                command_shift(Key::H),
                has_changes,
            )
            .item(
                Command::QuickStash,
                tr!("快速贮藏"),
                command_alt_shift(Key::H),
                has_changes,
            )
            .item(
                Command::PopStash,
                tr!("弹出贮藏…"),
                None,
                repo.is_some_and(|repo| repo.has_stash),
            )
            .separator()
            .item(
                Command::CheckoutBranch,
                tr!("检出分支…"),
                command(Key::B),
                open,
            )
            .separator()
            .item(
                Command::NewBranch,
                tr!("新建分支…"),
                command_shift(Key::B),
                open,
            )
            .item(
                Command::NewTag,
                tr!("新建标签…"),
                command_shift(Key::T),
                open,
            )
            .item(Command::AddRemote, tr!("添加远程仓库…"), None, open)
            .separator()
            .item(Command::ApplyPatch, tr!("应用补丁…"), None, open)
            .separator()
            .item(
                Command::RevealRepository,
                if MAC {
                    tr!("在访达中显示")
                } else {
                    tr!("在文件管理器中显示")
                },
                command_control(Key::O),
                open,
            )
            .item(
                Command::OpenTerminal,
                tr!("在终端中打开"),
                command_control(Key::T),
                open,
            )
            .separator()
            .item(Command::RepositorySettings, tr!("仓库设置…"), None, open)
            .build(),
    });

    if MAC {
        menus.push(Menu {
            role: Role::Window,
            title: tr!("窗口").into(),
            entries: Builder::new()
                .item(Command::Minimize, tr!("最小化"), command(Key::M), true)
                .item(Command::Zoom, tr!("缩放"), None, true)
                .build(),
        });
    }

    menus.push(Menu {
        role: Role::Help,
        title: tr!("帮助").into(),
        entries: Builder::new()
            .item(
                Command::Shortcuts,
                tr!("键盘快捷键"),
                command(Key::Slash),
                true,
            )
            .item(Command::GitDocs, tr!("Git 文档"), None, true)
            .when(!MAC, |menu| {
                menu.separator()
                    .item(Command::About, tr!("关于 git-forker"), None, true)
            })
            .build(),
    });
    menus
}

/// 菜单里所有带快捷键的命令，修饰键多的排在前面，保证 ⇧⌘F 不会被 ⌘F 抢走。
pub fn shortcuts(menus: &[Menu]) -> Vec<(KeyboardShortcut, Command, bool)> {
    fn collect(entries: &[Entry], out: &mut Vec<(KeyboardShortcut, Command, bool)>) {
        for entry in entries {
            match entry {
                Entry::Item {
                    command,
                    shortcut: Some(shortcut),
                    enabled,
                    ..
                } => out.push((*shortcut, *command, *enabled)),
                Entry::Submenu { entries, .. } => collect(entries, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    for menu in menus {
        collect(&menu.entries, &mut out);
    }
    out.sort_by_key(|(shortcut, _, _)| {
        let modifiers = shortcut.modifiers;
        std::cmp::Reverse(modifiers.shift as u8 + modifiers.alt as u8 + modifiers.ctrl as u8)
    });
    out
}

/// 按菜单顺序列出带快捷键的命令，用于快捷键一览。
pub fn flatten(entries: &[Entry], out: &mut Vec<(String, KeyboardShortcut)>) {
    for entry in entries {
        match entry {
            Entry::Item {
                label,
                shortcut: Some(shortcut),
                ..
            } => out.push((label.clone(), *shortcut)),
            Entry::Submenu { entries, .. } => flatten(entries, out),
            _ => {}
        }
    }
}

/// 消费本帧按下的菜单快捷键。
pub fn pressed(ctx: &egui::Context, menus: &[Menu]) -> Option<Command> {
    shortcuts(menus)
        .into_iter()
        .filter(|(_, command, enabled)| *enabled && !command.is_text_edit())
        .find(|(shortcut, _, _)| ctx.input_mut(|input| input.consume_shortcut(shortcut)))
        .map(|(_, command, _)| command)
}

/// 在窗口里画菜单栏（Windows 和 Linux）。
pub fn menu_bar(ui: &mut egui::Ui, menus: &[Menu]) -> Option<Command> {
    let mut chosen = None;
    egui::MenuBar::new().ui(ui, |ui| {
        for menu in menus {
            ui.menu_button(&menu.title, |ui| {
                entries(ui, &menu.entries, &mut chosen);
            });
        }
    });
    chosen
}

fn entries(ui: &mut egui::Ui, items: &[Entry], chosen: &mut Option<Command>) {
    for entry in items {
        match entry {
            Entry::Separator => {
                ui.separator();
            }
            Entry::Submenu {
                label,
                entries: sub,
            } => {
                ui.menu_button(label, |ui| entries(ui, sub, chosen));
            }
            Entry::Item {
                command,
                label,
                shortcut,
                enabled,
                checked,
            } => {
                let text = match checked {
                    Some(true) => format!("✓ {label}"),
                    Some(false) => format!("    {label}"),
                    None => label.clone(),
                };
                let mut button = egui::Button::new(text);
                if let Some(shortcut) = shortcut {
                    button = button.shortcut_text(ui.ctx().format_shortcut(shortcut));
                }
                if ui.add_enabled(*enabled, button).clicked() {
                    *chosen = Some(*command);
                    ui.close();
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use native::NativeMenu;

#[cfg(target_os = "macos")]
mod native {
    use std::collections::HashMap;
    use std::sync::mpsc::{Receiver, channel};

    use eframe::egui::{self, KeyboardShortcut};
    use git_forker::tr;
    use muda::accelerator::{Accelerator, Code, Modifiers};
    use muda::{CheckMenuItem, IsMenuItem, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

    use super::{Command, Entry, Menu, Role};

    /// macOS 屏幕顶部的原生菜单栏。菜单内容变化时整体重建。
    pub struct NativeMenu {
        current: Option<Vec<Menu>>,
        commands: HashMap<String, Command>,
        events: Receiver<String>,
        _menu: Option<muda::Menu>,
    }

    impl NativeMenu {
        pub fn new(ctx: &egui::Context) -> Self {
            let (sender, events) = channel();
            let ctx = ctx.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                let _ = sender.send(event.id.0);
                ctx.request_repaint();
            }));
            Self {
                current: None,
                commands: HashMap::new(),
                events,
                _menu: None,
            }
        }

        pub fn update(&mut self, menus: &[Menu]) {
            if self.current.as_deref() == Some(menus) {
                return;
            }
            self.commands.clear();
            let bar = muda::Menu::new();
            for menu in menus {
                let submenu = Submenu::new(&menu.title, true);
                if menu.role == Role::App {
                    let _ = submenu.append(&*self.item_for(&menu.entries[0]));
                    let _ = submenu.append(&PredefinedMenuItem::separator());
                    for entry in &menu.entries[2..] {
                        self.append(&submenu, entry);
                    }
                    let _ = submenu.append_items(&[
                        &PredefinedMenuItem::separator(),
                        &PredefinedMenuItem::services(Some(tr!("服务"))),
                        &PredefinedMenuItem::separator(),
                        &PredefinedMenuItem::hide(Some(tr!("隐藏 git-forker"))),
                        &PredefinedMenuItem::hide_others(Some(tr!("隐藏其他"))),
                        &PredefinedMenuItem::show_all(Some(tr!("全部显示"))),
                        &PredefinedMenuItem::separator(),
                        &PredefinedMenuItem::quit(Some(tr!("退出 git-forker"))),
                    ]);
                } else {
                    for entry in &menu.entries {
                        self.append(&submenu, entry);
                    }
                }
                if menu.role == Role::Window {
                    let _ = submenu.append_items(&[
                        &PredefinedMenuItem::separator(),
                        &PredefinedMenuItem::bring_all_to_front(Some(tr!("前置全部窗口"))),
                    ]);
                    submenu.set_as_windows_menu_for_nsapp();
                }
                if menu.role == Role::Help {
                    submenu.set_as_help_menu_for_nsapp();
                }
                let _ = bar.append(&submenu);
            }
            bar.init_for_nsapp();
            self._menu = Some(bar);
            self.current = Some(menus.to_vec());
        }

        fn append(&mut self, submenu: &Submenu, entry: &Entry) {
            match entry {
                Entry::Separator => {
                    let _ = submenu.append(&PredefinedMenuItem::separator());
                }
                Entry::Submenu { label, entries } => {
                    let child = Submenu::new(label, true);
                    for entry in entries {
                        self.append(&child, entry);
                    }
                    let _ = submenu.append(&child);
                }
                Entry::Item { .. } => {
                    let _ = submenu.append(&*self.item_for(entry));
                }
            }
        }

        fn item_for(&mut self, entry: &Entry) -> Box<dyn IsMenuItem> {
            let Entry::Item {
                command,
                label,
                shortcut,
                enabled,
                checked,
            } = entry
            else {
                return Box::new(PredefinedMenuItem::separator());
            };
            let id = command.id();
            self.commands.insert(id.clone(), *command);
            let accelerator = shortcut.as_ref().and_then(accelerator);
            match checked {
                Some(checked) => Box::new(CheckMenuItem::with_id(
                    id,
                    label,
                    *enabled,
                    *checked,
                    accelerator,
                )),
                None => Box::new(MenuItem::with_id(id, label, *enabled, accelerator)),
            }
        }

        /// 取出用户在原生菜单里点过的命令。
        pub fn take_commands(&mut self) -> Vec<Command> {
            self.events
                .try_iter()
                .filter_map(|id| self.commands.get(&id).copied())
                .collect()
        }
    }

    pub(super) fn accelerator(shortcut: &KeyboardShortcut) -> Option<Accelerator> {
        let name = shortcut.logical_key.name();
        let code: Code = match name {
            "Comma" => Code::Comma,
            "Period" => Code::Period,
            "Slash" => Code::Slash,
            "F11" => Code::F11,
            _ if name.len() == 1 && name.as_bytes()[0].is_ascii_digit() => {
                format!("Digit{name}").parse().ok()?
            }
            _ if name.len() == 1 => format!("Key{name}").parse().ok()?,
            _ => return None,
        };
        let modifiers = shortcut.modifiers;
        let mut mods = Modifiers::empty();
        if modifiers.command || modifiers.mac_cmd {
            mods |= Modifiers::SUPER;
        }
        if modifiers.shift {
            mods |= Modifiers::SHIFT;
        }
        if modifiers.alt {
            mods |= Modifiers::ALT;
        }
        if modifiers.ctrl {
            mods |= Modifiers::CONTROL;
        }
        Some(Accelerator::new(Some(mods), code))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_repository() -> MenuState {
        MenuState {
            repository: Some(RepositoryState {
                view: View::History,
                details_tab: DetailsTab::Commit,
                show_details: true,
                show_log: false,
                options: HistoryOptions::default(),
                has_stash: true,
                has_changes: true,
                has_remotes: true,
                on_branch: true,
            }),
            recent: vec!["/tmp/repo".into()],
            show_manager: true,
        }
    }

    #[test]
    fn every_shortcut_is_bound_once() {
        let menus = model(&open_repository());
        let mut seen = std::collections::HashMap::new();
        for (shortcut, command, _) in shortcuts(&menus) {
            let key = (
                shortcut.logical_key,
                shortcut.modifiers.command,
                shortcut.modifiers.shift,
                shortcut.modifiers.alt,
                shortcut.modifiers.ctrl,
            );
            if let Some(previous) = seen.insert(key, command) {
                panic!("{previous:?} and {command:?} share a shortcut");
            }
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn every_shortcut_has_a_native_accelerator() {
        let menus = model(&open_repository());
        for (shortcut, command, _) in shortcuts(&menus) {
            assert!(
                native::accelerator(&shortcut).is_some(),
                "{command:?} has no native accelerator"
            );
        }
    }
}
