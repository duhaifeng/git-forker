mod dialogs;
mod diff_view;
mod icons;
mod menu;
mod platform;
mod settings;
mod state;
mod syntax;
mod tab;
mod theme;
mod widgets;

use git_forker::tr;
use std::path::{Path, PathBuf};

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Key, Layout, Modifiers, Rect, RichText,
    Sense, Stroke, Vec2, pos2, vec2,
};
use git_forker::i18n;
use git_forker::{JobHandle, Repo};

use dialogs::{CloneDialog, CloneOutcome};
use icons::Icon;
use menu::{Command, Menu, MenuState};
use settings::SettingsWindow;
use state::{AppState, Appearance};
use tab::{RepoTab, TabRequest};

pub fn run(path: &Path) -> eframe::Result {
    let path = path.to_path_buf();
    let options = eframe::NativeOptions {
        viewport: {
            let viewport = egui::ViewportBuilder::default()
                .with_inner_size([1360.0, 860.0])
                .with_min_inner_size([900.0, 560.0])
                .with_title("git-forker")
                .with_fullsize_content_view(MAC)
                .with_title_shown(!MAC)
                .with_titlebar_shown(!MAC);
            // macOS 的 Dock 不留白，图案缩到 80% 并使用透明底，避免白底把图标撑满。
            #[cfg(target_os = "macos")]
            let viewport = viewport.with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!(
                    "../../assets/git-forker-mac.png"
                ))
                .expect("应用图标 PNG 无效"),
            );
            viewport
        },
        ..Default::default()
    };
    eframe::run_native(
        "git-forker",
        options,
        Box::new(move |creation| {
            theme::apply(&creation.egui_ctx);
            Ok(Box::new(App::new(&creation.egui_ctx, &path)))
        }),
    )
}

/// macOS 上内容延伸到标题栏，左上角给红绿灯按钮留出位置。
const MAC: bool = cfg!(target_os = "macos");
const TRAFFIC_LIGHTS: f32 = if MAC { 78.0 } else { 0.0 };
const TOOLBAR_HEIGHT: f32 = 52.0;

struct App {
    state: AppState,
    tabs: Vec<RepoTab>,
    active: Option<usize>,
    clone: Option<CloneDialog>,
    clone_job: Option<(JobHandle, PathBuf)>,
    notice: Option<(String, bool)>,
    manager_filter: String,
    show_manager: bool,
    settings: Option<SettingsWindow>,
    show_about: bool,
    show_shortcuts: bool,
    /// 当前帧的菜单模型，工具栏按钮的快捷键提示也从这里取。
    menus: Vec<Menu>,
    /// 工具栏等界面元素发出、在本帧结束前执行的命令。
    queued: Vec<Command>,
    /// 菜单里的编辑命令要在下一帧作为输入事件交给输入框。
    pending_events: Vec<egui::Event>,
    #[cfg(target_os = "macos")]
    native_menu: Option<menu::NativeMenu>,
    was_focused: bool,
    dirty: bool,
}

impl App {
    fn new(ctx: &egui::Context, path: &Path) -> Self {
        let state = AppState::load();
        i18n::set_language(state.language);
        apply_appearance(ctx, state.appearance);
        git_forker::set_git_program(Some(&state.preferences.git_path));
        let mut app = Self {
            tabs: Vec::new(),
            active: None,
            clone: None,
            clone_job: None,
            notice: None,
            manager_filter: String::new(),
            show_manager: true,
            settings: None,
            show_about: false,
            show_shortcuts: false,
            menus: Vec::new(),
            queued: Vec::new(),
            pending_events: Vec::new(),
            #[cfg(target_os = "macos")]
            native_menu: Some(menu::NativeMenu::new(ctx)),
            was_focused: true,
            dirty: false,
            state,
        };
        let restore = if app.state.preferences.restore_tabs {
            app.state.open_tabs.clone()
        } else {
            Vec::new()
        };
        for root in restore {
            if root.exists() {
                let tab = app.new_tab(root);
                if tab.is_valid() {
                    app.tabs.push(tab);
                }
            }
        }
        if !app.tabs.is_empty() {
            app.active = Some(app.state.active_tab.min(app.tabs.len() - 1));
        }
        if let Ok(repo) = Repo::discover(path) {
            app.open_repository(repo.root());
        }
        app
    }

    fn open_repository(&mut self, path: PathBuf) {
        let root = match Repo::discover(&path) {
            Ok(repo) => repo.root(),
            Err(err) => {
                self.notice = Some((tr!("{path}：{err}", path = path.display(), err = err), true));
                return;
            }
        };
        let root = root.canonicalize().unwrap_or(root);
        if let Some(index) = self.tabs.iter().position(|tab| tab.root == root) {
            self.active = Some(index);
        } else {
            let tab = self.new_tab(root.clone());
            self.tabs.push(tab);
            self.active = Some(self.tabs.len() - 1);
        }
        self.state.remember_repository(&root);
        self.dirty = true;
    }

    fn new_tab(&self, root: PathBuf) -> RepoTab {
        let mut tab = RepoTab::open(root);
        tab.set_diff_defaults(&self.state.preferences);
        tab.set_terminal(&self.state.preferences.terminal);
        tab
    }

    fn close_tab(&mut self, index: usize) {
        self.tabs.remove(index);
        self.active = match self.active {
            _ if self.tabs.is_empty() => None,
            Some(active) if active > index => Some(active - 1),
            Some(active) => Some(active.min(self.tabs.len() - 1)),
            None => None,
        };
        self.dirty = true;
    }

    fn active_tab(&mut self) -> Option<&mut RepoTab> {
        self.active.and_then(|index| self.tabs.get_mut(index))
    }

    fn pick_and_open(&mut self) {
        if let Some(folder) = rfd::FileDialog::new()
            .set_title(tr!("打开仓库"))
            .pick_folder()
        {
            self.open_repository(folder);
        }
    }

    fn init_repository(&mut self) {
        let Some(folder) = rfd::FileDialog::new()
            .set_title(tr!("选择新仓库的目录"))
            .pick_folder()
        else {
            return;
        };
        match Repo::init(&folder) {
            Ok(_) => self.open_repository(folder),
            Err(err) => self.notice = Some((err.to_string(), true)),
        }
    }

    fn save_state(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        self.state.open_tabs = self.tabs.iter().map(|tab| tab.root.clone()).collect();
        self.state.active_tab = self.active.unwrap_or(0);
        if let Err(err) = self.state.save() {
            self.notice = Some((tr!("无法保存界面状态：{err}", err = err), true));
        }
    }

    fn menu_state(&self) -> MenuState {
        MenuState {
            repository: self
                .active
                .and_then(|index| self.tabs.get(index))
                .and_then(RepoTab::menu_state),
            recent: self
                .state
                .repositories
                .iter()
                .map(|path| path.display().to_string())
                .collect(),
            show_manager: self.show_manager || self.active.is_none(),
        }
    }

    fn modal_open(&self) -> bool {
        self.clone.is_some()
            || self
                .active
                .and_then(|index| self.tabs.get(index))
                .is_some_and(RepoTab::has_modal)
    }

    /// 本帧来自原生菜单、窗口菜单栏和快捷键的命令。
    fn collect_commands(&mut self, ctx: &egui::Context, menus: &[Menu]) -> Vec<Command> {
        let mut commands = Vec::new();
        #[cfg(target_os = "macos")]
        if let Some(native) = &mut self.native_menu {
            native.update(menus);
            commands.extend(native.take_commands());
        }
        if !self.modal_open() {
            commands.extend(menu::pressed(ctx, menus));
            if ctx.input_mut(|input| input.consume_key(Modifiers::COMMAND, Key::Enter))
                && let Some(tab) = self.active_tab()
            {
                tab.request_commit();
            }
        }
        commands
    }

    fn run_command(&mut self, ctx: &egui::Context, command: Command) {
        if command.is_text_edit() {
            self.pending_events.push(match command {
                Command::Copy => egui::Event::Copy,
                Command::Cut => egui::Event::Cut,
                Command::Paste => match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                    Ok(text) => egui::Event::Paste(text),
                    Err(_) => return,
                },
                Command::Undo => key_event(Key::Z, Modifiers::COMMAND),
                Command::Redo => key_event(Key::Z, Modifiers::COMMAND | Modifiers::SHIFT),
                _ => key_event(Key::A, Modifiers::COMMAND),
            });
            ctx.request_repaint();
            return;
        }
        // 对话框打开时，只响应不会和对话框冲突的窗口级命令。
        if self.modal_open()
            && !matches!(
                command,
                Command::Minimize | Command::Zoom | Command::FullScreen | Command::Quit
            )
        {
            return;
        }
        match command {
            Command::About => self.show_about = true,
            Command::Settings => {
                if self.settings.is_none() {
                    self.settings = Some(SettingsWindow::new());
                }
            }
            Command::Shortcuts => self.show_shortcuts = true,
            Command::GitDocs => {
                if let Err(err) = platform::open_url("https://git-scm.com/docs") {
                    self.notice = Some((err, true));
                }
            }
            Command::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Command::Minimize => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
            Command::Zoom => {
                let maximized = ctx.input(|input| input.viewport().maximized == Some(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            Command::FullScreen => {
                let fullscreen = ctx.input(|input| input.viewport().fullscreen == Some(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!fullscreen));
            }
            Command::NewTab => self.active = None,
            Command::Open => self.pick_and_open(),
            Command::Clone => self.clone = Some(CloneDialog::new()),
            Command::InitRepository => self.init_repository(),
            Command::OpenRecent(index) => {
                if let Some(path) = self.state.repositories.get(index).cloned() {
                    self.open_repository(path);
                }
            }
            Command::CloseTab => {
                if let Some(index) = self.active {
                    self.close_tab(index);
                }
            }
            Command::ToggleManager => {
                self.show_manager = !(self.show_manager || self.active.is_none());
            }
            command => {
                if let Some(tab) = self.active_tab() {
                    tab.run(command);
                }
            }
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        let Some(window) = &mut self.settings else {
            return;
        };
        let language = self.state.language;
        let appearance = self.state.appearance;
        let before = self.state.preferences.clone();
        let open = window.show(ctx, &mut self.state);
        if !open {
            self.settings = None;
        }
        let after = &self.state.preferences;
        if language != self.state.language {
            i18n::set_language(self.state.language);
        }
        if appearance != self.state.appearance {
            apply_appearance(ctx, self.state.appearance);
        }
        if (
            before.context_lines,
            before.ignore_whitespace,
            before.wrap_lines,
            before.syntax_highlighting,
        ) != (
            after.context_lines,
            after.ignore_whitespace,
            after.wrap_lines,
            after.syntax_highlighting,
        ) {
            for tab in &mut self.tabs {
                tab.set_diff_defaults(after);
            }
        }
        if before.terminal != after.terminal {
            for tab in &mut self.tabs {
                tab.set_terminal(&after.terminal);
            }
        }
        if language != self.state.language
            || appearance != self.state.appearance
            || before != *after
        {
            self.dirty = true;
        }
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_about;
        egui::Window::new(tr!("关于 git-forker"))
            .id(egui::Id::unique("about-window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(6.0);
                    ui.label(RichText::new("git-forker").size(22.0).strong());
                    ui.label(tr!("版本 {version}", version = env!("CARGO_PKG_VERSION")));
                    ui.add_space(4.0);
                    ui.weak(tr!("跨平台的本地 Git 客户端"));
                    ui.add_space(6.0);
                });
            });
        self.show_about = open;
    }

    fn shortcuts_window(&mut self, ctx: &egui::Context, menus: &[Menu]) {
        let mut open = self.show_shortcuts;
        egui::Window::new(tr!("键盘快捷键"))
            .id(egui::Id::unique("shortcuts-window"))
            .open(&mut open)
            .default_size([460.0, 520.0])
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for menu in menus {
                        let mut rows = Vec::new();
                        menu::flatten(&menu.entries, &mut rows);
                        if rows.is_empty() {
                            continue;
                        }
                        ui.label(RichText::new(&menu.title).strong());
                        egui::Grid::new(("shortcuts", &menu.title))
                            .num_columns(2)
                            .spacing([24.0, 4.0])
                            .striped(true)
                            .show(ui, |ui| {
                                for (label, shortcut) in rows {
                                    ui.label(label);
                                    ui.label(
                                        RichText::new(ctx.format_shortcut(&shortcut)).monospace(),
                                    );
                                    ui.end_row();
                                }
                            });
                        ui.add_space(8.0);
                    }
                });
            });
        self.show_shortcuts = open;
    }

    /// 和标题栏合为一体的大工具栏。
    fn toolbar(&mut self, ui: &mut egui::Ui, manager_visible: bool) {
        let rect = ui.max_rect();
        let drag = ui.interact(
            rect,
            ui.scope_id().with("window-drag"),
            Sense::click_and_drag(),
        );
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            let maximized = ui
                .ctx()
                .input(|input| input.viewport().maximized == Some(true));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }

        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.add_space(if manager_visible {
                8.0
            } else {
                TRAFFIC_LIGHTS + 4.0
            });
            let menus = std::mem::take(&mut self.menus);
            let mut queue = Vec::new();
            let ctx = ui.ctx().clone();
            let tip = |label: &str, command| tooltip(&ctx, &menus, label, command);
            if icons::toolbar_button(ui, Icon::Sidebar, tr!("仓库列表"), true, None, false)
                .on_hover_text(tip(tr!("显示或隐藏仓库管理器"), Command::ToggleManager))
                .clicked()
            {
                self.show_manager = !manager_visible;
            }
            toolbar_divider(ui);
            let active = self.active.filter(|index| *index < self.tabs.len());
            let enabled = active.is_some();
            let (ahead, behind, has_stash, changes) = active
                .map(|index| {
                    let tab = &self.tabs[index];
                    let (ahead, behind) = tab.current_counts();
                    (ahead, behind, tab.has_stash(), tab.change_count())
                })
                .unwrap_or_default();

            if icons::toolbar_button(ui, Icon::QuickLaunch, tr!("快速启动"), enabled, None, false)
                .on_hover_text(tip(tr!("快速启动"), Command::QuickLaunch))
                .clicked()
            {
                queue.push(Command::QuickLaunch);
            }
            if icons::toolbar_button(ui, Icon::Fetch, tr!("抓取"), enabled, None, false)
                .on_hover_text(tip(tr!("抓取"), Command::Fetch))
                .clicked()
            {
                queue.push(Command::Fetch);
            }
            toolbar_divider(ui);
            if icons::toolbar_button(ui, Icon::Pull, tr!("拉取"), enabled, Some(behind), false)
                .on_hover_text(tip(tr!("拉取"), Command::Pull))
                .clicked()
            {
                queue.push(Command::Pull);
            }
            toolbar_divider(ui);
            if icons::toolbar_button(ui, Icon::Push, tr!("推送"), enabled, Some(ahead), false)
                .on_hover_text(tip(tr!("推送"), Command::Push))
                .clicked()
            {
                queue.push(Command::Push);
            }
            let stash = icons::toolbar_button(ui, Icon::Stash, tr!("贮藏"), enabled, None, true);
            egui::Popup::menu(&stash).show(|ui| {
                command_item(
                    ui,
                    &menus,
                    Command::Stash,
                    tr!("贮藏更改…"),
                    changes > 0,
                    &mut queue,
                );
                command_item(
                    ui,
                    &menus,
                    Command::QuickStash,
                    tr!("快速贮藏"),
                    changes > 0,
                    &mut queue,
                );
                command_item(
                    ui,
                    &menus,
                    Command::PopStash,
                    tr!("弹出最近的贮藏"),
                    has_stash,
                    &mut queue,
                );
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(10.0);
                let open_in =
                    icons::toolbar_button(ui, Icon::OpenIn, tr!("打开方式"), enabled, None, true);
                if icons::toolbar_button(ui, Icon::Activity, tr!("活动"), enabled, None, false)
                    .on_hover_text(tr!("活动日志"))
                    .clicked()
                {
                    queue.push(Command::ToggleActivity);
                }
                let branch =
                    icons::toolbar_button(ui, Icon::Branch, tr!("分支"), enabled, None, true);
                egui::Popup::menu(&open_in).show(|ui| {
                    command_item(
                        ui,
                        &menus,
                        Command::OpenTerminal,
                        tr!("终端"),
                        true,
                        &mut queue,
                    );
                    command_item(
                        ui,
                        &menus,
                        Command::RevealRepository,
                        tr!("文件管理器"),
                        true,
                        &mut queue,
                    );
                    if let Some(tab) = active.map(|index| &self.tabs[index])
                        && ui.button(tr!("复制仓库路径")).clicked()
                    {
                        ui.ctx().copy_text(tab.root.display().to_string());
                        ui.close();
                    }
                });
                egui::Popup::menu(&branch).show(|ui| {
                    command_item(
                        ui,
                        &menus,
                        Command::NewBranch,
                        tr!("新建分支…"),
                        true,
                        &mut queue,
                    );
                    command_item(
                        ui,
                        &menus,
                        Command::CheckoutBranch,
                        tr!("检出分支…"),
                        true,
                        &mut queue,
                    );
                    ui.separator();
                    command_item(
                        ui,
                        &menus,
                        Command::NewTag,
                        tr!("新建标签…"),
                        true,
                        &mut queue,
                    );
                    command_item(
                        ui,
                        &menus,
                        Command::AddRemote,
                        tr!("添加远程仓库…"),
                        true,
                        &mut queue,
                    );
                });
                if let Some(index) = active {
                    let width = (ui.available_width() - 24.0).clamp(160.0, 460.0);
                    ui.add_space(((ui.available_width() - width) / 2.0).max(0.0));
                    let hint = tip(tr!("检出分支…"), Command::CheckoutBranch);
                    if status_box(ui, &self.tabs[index], width)
                        .on_hover_text(hint)
                        .clicked()
                    {
                        queue.push(Command::CheckoutBranch);
                    }
                }
            });
            self.menus = menus;
            self.queued.extend(queue);
        });
    }

    /// 等宽的胶囊形标签页，右侧是新建标签页按钮。
    fn tab_strip(&mut self, ui: &mut egui::Ui) {
        let height = 26.0;
        let full = ui.available_width();
        let plus_width = 30.0;
        let count = self.tabs.len().max(1);
        let tab_width = ((full - plus_width - 4.0) / count as f32).min(full);
        let mut close = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let dark = ui.visuals().dark_mode;
            let strip =
                Rect::from_min_size(ui.cursor().min, vec2(tab_width * count as f32, height));
            ui.painter()
                .rect_filled(strip, CornerRadius::same(7), theme::tab_strip_fill(dark));
            for (index, tab) in self.tabs.iter().enumerate() {
                let selected = self.active == Some(index);
                let (rect, response) =
                    ui.allocate_exact_size(vec2(tab_width, height), Sense::click());
                if index > 0 && !selected && self.active != Some(index - 1) {
                    ui.painter().line_segment(
                        [
                            pos2(rect.left(), rect.top() + 6.0),
                            pos2(rect.left(), rect.bottom() - 6.0),
                        ],
                        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
                    );
                }
                let rect = rect.shrink(1.5);
                let fill = if selected {
                    theme::row_selection(dark)
                } else if response.hovered() {
                    theme::row_selection(dark).gamma_multiply(0.4)
                } else {
                    Color32::TRANSPARENT
                };
                ui.painter().rect_filled(rect, CornerRadius::same(6), fill);
                let color = if selected {
                    ui.visuals().strong_text_color()
                } else {
                    ui.visuals().weak_text_color()
                };
                ui.painter()
                    .with_clip_rect(rect.shrink2(vec2(26.0, 0.0)))
                    .text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        &tab.name,
                        FontId::proportional(12.5),
                        color,
                    );
                if tab.is_busy() || tab.is_reading() {
                    egui::Spinner::new().paint_at(
                        ui,
                        Rect::from_center_size(
                            pos2(rect.right() - 14.0, rect.center().y),
                            Vec2::splat(10.0),
                        ),
                    );
                } else if tab.change_count() > 0 {
                    ui.painter().circle_filled(
                        pos2(rect.right() - 14.0, rect.center().y),
                        3.0,
                        ui.visuals().weak_text_color(),
                    );
                }
                if response.hovered() {
                    let button = Rect::from_center_size(
                        pos2(rect.left() + 13.0, rect.center().y),
                        Vec2::splat(16.0),
                    );
                    let hit = ui.interact(
                        button,
                        ui.scope_id().with(("close-tab", index)),
                        Sense::click(),
                    );
                    if hit.hovered() {
                        ui.painter().rect_filled(
                            button,
                            4.0,
                            ui.visuals().widgets.hovered.weak_bg_fill,
                        );
                    }
                    icons::paint(
                        ui.painter(),
                        button.center(),
                        icons::SMALL,
                        Icon::Close,
                        color,
                    );
                    if hit.clicked() {
                        close = Some(index);
                    }
                }
                let response = response.on_hover_text(tab.root.display().to_string());
                if response.clicked() && close.is_none() {
                    self.active = Some(index);
                    self.dirty = true;
                }
                if response.middle_clicked() {
                    close = Some(index);
                }
            }
            if self.tabs.is_empty() {
                ui.add_space(tab_width);
            }
            ui.add_space(4.0);
            let (rect, response) = ui.allocate_exact_size(vec2(height, height), Sense::click());
            if response.hovered() || self.active.is_none() {
                ui.painter().circle_filled(
                    rect.center(),
                    height / 2.0 - 1.0,
                    ui.visuals().widgets.hovered.weak_bg_fill,
                );
            }
            icons::paint(
                ui.painter(),
                rect.center(),
                icons::INLINE,
                Icon::Plus,
                ui.visuals().text_color(),
            );
            if response.on_hover_text(tr!("打开仓库")).clicked() {
                self.active = None;
            }
        });
        if let Some(index) = close {
            self.close_tab(index);
        }
    }

    /// 最左侧的仓库管理器侧栏。
    fn manager_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(if MAC { 40.0 } else { 10.0 });
        ui.add(
            egui::TextEdit::singleline(&mut self.manager_filter)
                .hint_text(tr!("过滤仓库"))
                .desired_width(f32::INFINITY),
        );
        ui.add_space(6.0);
        let filter = self.manager_filter.to_lowercase();
        let active_root = self
            .active
            .and_then(|index| self.tabs.get(index))
            .map(|tab| tab.root.clone());
        let mut open = None;
        let mut forget = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .max_height(ui.available_height() - 48.0)
            .show(ui, |ui| {
                ui.label(
                    RichText::new(tr!("仓库"))
                        .size(11.0)
                        .strong()
                        .color(ui.visuals().weak_text_color()),
                );
                for (index, path) in self.state.repositories.iter().enumerate() {
                    let display = path.display().to_string();
                    if !filter.is_empty() && !display.to_lowercase().contains(&filter) {
                        continue;
                    }
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| display.clone());
                    let exists = path.exists();
                    let selected = active_root.as_ref() == Some(path);
                    let dirty = self
                        .tabs
                        .iter()
                        .any(|tab| &tab.root == path && tab.change_count() > 0);
                    let (rect, response) =
                        ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                    let dark = ui.visuals().dark_mode;
                    if selected {
                        ui.painter()
                            .rect_filled(rect, 5.0, theme::row_selection(dark));
                    } else if response.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            5.0,
                            theme::row_selection(dark).gamma_multiply(0.5),
                        );
                    }
                    let color = if exists {
                        ui.visuals().text_color()
                    } else {
                        ui.visuals().weak_text_color()
                    };
                    if dirty {
                        ui.painter().circle_filled(
                            pos2(rect.left() + 12.0, rect.center().y),
                            3.5,
                            ui.visuals().weak_text_color(),
                        );
                    }
                    ui.painter()
                        .with_clip_rect(rect.shrink2(vec2(6.0, 0.0)))
                        .text(
                            pos2(rect.left() + 24.0, rect.center().y),
                            Align2::LEFT_CENTER,
                            &name,
                            FontId::proportional(13.5),
                            color,
                        );
                    let response = response.on_hover_text(&display);
                    if response.clicked() && exists {
                        open = Some(path.clone());
                    }
                    response.context_menu(|ui| {
                        if ui.button(tr!("打开")).clicked() {
                            open = Some(path.clone());
                            ui.close();
                        }
                        if ui.button(tr!("在文件管理器中显示")).clicked() {
                            let _ = platform::reveal(path);
                            ui.close();
                        }
                        if ui.button(tr!("复制路径")).clicked() {
                            ui.ctx().copy_text(display.clone());
                            ui.close();
                        }
                        ui.separator();
                        if ui.button(tr!("从列表中移除")).clicked() {
                            forget = Some(index);
                            ui.close();
                        }
                    });
                }
                if self.state.repositories.is_empty() {
                    ui.weak(tr!("还没有打开过仓库"));
                }
            });
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            ui.add_space(15.0);
            ui.horizontal(|ui| {
                // 侧栏内容区本身离左边缘 8 像素，再补 7 像素后按钮左边缘正好是 15。
                ui.add_space(7.0);
                let add = icons::circle_plus_button(ui, tr!("添加仓库"));
                egui::Popup::menu(&add).show(|ui| {
                    if ui.button(tr!("打开本地仓库…")).clicked() {
                        self.pick_and_open();
                        ui.close();
                    }
                    if ui.button(tr!("克隆…")).clicked() {
                        self.clone = Some(CloneDialog::new());
                        ui.close();
                    }
                    if ui.button(tr!("新建仓库…")).clicked() {
                        self.init_repository();
                        ui.close();
                    }
                });
            });
        });
        if let Some(index) = forget {
            self.state.repositories.remove(index);
            self.dirty = true;
        }
        if let Some(path) = open {
            self.open_repository(path);
        }
    }

    /// 没有打开仓库时的欢迎页。
    fn welcome(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() / 2.0 - 110.0).max(20.0));
            ui.label(RichText::new("git-forker").size(26.0).strong());
            ui.weak(tr!("跨平台的本地 Git 客户端"));
            ui.add_space(22.0);
            for (command, label) in [
                (Command::Open, tr!("打开本地仓库…")),
                (Command::Clone, tr!("克隆远程仓库…")),
                (Command::InitRepository, tr!("新建仓库…")),
            ] {
                let mut button =
                    egui::Button::new(RichText::new(label).size(14.0)).min_size(vec2(260.0, 30.0));
                if let Some(shortcut) = shortcut_text(ui.ctx(), &self.menus, command) {
                    button = button.shortcut_text(shortcut);
                }
                if ui.add(button).clicked() {
                    self.queued.push(command);
                }
                ui.add_space(4.0);
            }
        });
    }

    fn handle_clone(&mut self, ctx: &egui::Context) {
        if let Some(dialog) = &mut self.clone {
            match dialog.show(ctx) {
                CloneOutcome::Keep => {}
                CloneOutcome::Cancel => self.clone = None,
                CloneOutcome::Start(job, target) => {
                    self.clone = None;
                    let ctx = ctx.clone();
                    self.clone_job =
                        Some((JobHandle::spawn(job, move || ctx.request_repaint()), target));
                }
            }
        }
        if let Some((job, target)) = &mut self.clone_job {
            if let Some(ok) = job.poll() {
                let target = target.clone();
                if ok {
                    self.notice = Some((tr!("克隆完成").into(), false));
                    self.clone_job = None;
                    self.open_repository(target);
                } else {
                    self.notice = Some((tr!("克隆失败：{reason}", reason = job.last_line()), true));
                    self.clone_job = None;
                }
            } else {
                let line = job.last_line().to_string();
                egui::Window::new(tr!("正在克隆"))
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(line);
                        });
                    });
            }
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        let focused = ctx.input(|input| input.focused);
        if focused
            && !self.was_focused
            && self.state.preferences.refresh_on_focus
            && let Some(tab) = self.active_tab()
            && !tab.is_busy()
        {
            tab.request_reload();
        }
        self.was_focused = focused;

        let menus = menu::model(&self.menu_state());
        let mut commands = self.collect_commands(&ctx, &menus);
        self.menus = menus.clone();
        if !MAC {
            egui::Panel::top("menu-bar").show(ui, |ui| {
                commands.extend(menu::menu_bar(ui, &menus));
            });
        }
        for command in commands {
            self.run_command(&ctx, command);
        }

        let dark = ui.visuals().dark_mode;
        let manager_visible = self.show_manager || self.active.is_none();
        if manager_visible {
            egui::Panel::left("repository-manager")
                .resizable(true)
                .default_size(220.0)
                .min_size(170.0)
                .frame(
                    egui::Frame::new()
                        .fill(theme::manager_fill(dark))
                        .inner_margin(egui::Margin::symmetric(8, 0)),
                )
                .show(ui, |ui| self.manager_panel(ui));
        }
        egui::Panel::top("toolbar")
            .exact_size(TOOLBAR_HEIGHT)
            .frame(egui::Frame::new().fill(theme::chrome_fill(dark)))
            .show(ui, |ui| self.toolbar(ui, manager_visible));
        egui::Panel::top("tabs")
            .frame(
                egui::Frame::new()
                    .fill(theme::chrome_fill(dark))
                    .inner_margin(egui::Margin {
                        left: 6,
                        right: 6,
                        top: 0,
                        bottom: 5,
                    }),
            )
            .show(ui, |ui| self.tab_strip(ui));
        let notice = self.notice.clone();
        match self.active {
            Some(index) if index < self.tabs.len() => {
                let tab = &mut self.tabs[index];
                if notice.is_some() || tab.has_status() {
                    egui::Panel::bottom("status").show(ui, |ui| {
                        if let Some((text, error)) = &notice {
                            notice_line(ui, text, *error);
                        }
                        tab.status_bar(ui);
                    });
                }
                egui::Panel::left("sidebar")
                    .resizable(true)
                    .default_size(250.0)
                    .min_size(190.0)
                    .frame(
                        egui::Frame::new()
                            .fill(ui.visuals().panel_fill)
                            .inner_margin(egui::Margin::symmetric(6, 4)),
                    )
                    .show(ui, |ui| tab.sidebar(ui));
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(ui.visuals().panel_fill))
                    .show(ui, |ui| tab.main(ui));
            }
            _ => {
                self.active = None;
                if let Some((text, error)) = &notice {
                    egui::Panel::bottom("status").show(ui, |ui| notice_line(ui, text, *error));
                }
                egui::CentralPanel::default().show(ui, |ui| self.welcome(ui));
            }
        }

        let recent = self.state.recent_messages.clone();
        let mut open_requests = Vec::new();
        for (index, tab) in self.tabs.iter_mut().enumerate() {
            if Some(index) == self.active {
                tab.windows(&ctx, &recent);
            }
            tab.update(&ctx, Some(index) == self.active);
            for request in tab.requests.drain(..) {
                match request {
                    TabRequest::OpenRepository(path) => open_requests.push(path),
                    TabRequest::RememberMessage(message) => {
                        self.state.remember_message(&message);
                        self.dirty = true;
                    }
                }
            }
        }
        for path in open_requests {
            self.open_repository(path);
        }
        for command in std::mem::take(&mut self.queued) {
            self.run_command(&ctx, command);
        }
        self.handle_clone(&ctx);
        self.settings_window(&ctx);
        self.about_window(&ctx);
        self.shortcuts_window(&ctx, &menus);
        self.save_state();
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        raw_input.events.append(&mut self.pending_events);
    }
}

fn key_event(key: Key, modifiers: Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

fn apply_appearance(ctx: &egui::Context, appearance: Appearance) {
    ctx.set_theme(match appearance {
        Appearance::System => egui::ThemePreference::System,
        Appearance::Light => egui::ThemePreference::Light,
        Appearance::Dark => egui::ThemePreference::Dark,
    });
}

fn notice_line(ui: &mut egui::Ui, text: &str, error: bool) {
    let color = if error {
        ui.visuals().error_fg_color
    } else {
        ui.visuals().weak_text_color()
    };
    ui.colored_label(color, text);
}

/// 工具栏中央的状态框：仓库名（忙碌时显示进度）和当前分支。
fn status_box(ui: &mut egui::Ui, tab: &RepoTab, width: f32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 38.0), Sense::click());
    let visuals = ui.visuals();
    let fill = if visuals.dark_mode {
        Color32::from_rgb(58, 55, 56)
    } else {
        Color32::from_rgb(246, 245, 245)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(7), fill);
    painter.rect_stroke(
        rect,
        CornerRadius::same(7),
        Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color),
        egui::StrokeKind::Inside,
    );
    let top = pos2(rect.center().x, rect.top() + 12.0);
    if let Some(line) = tab.last_job_line().filter(|_| tab.is_busy()) {
        let spinner = Rect::from_center_size(pos2(rect.left() + 16.0, top.y), Vec2::splat(12.0));
        egui::Spinner::new().paint_at(ui, spinner);
        painter.with_clip_rect(rect.shrink2(vec2(28.0, 0.0))).text(
            top,
            Align2::CENTER_CENTER,
            line,
            FontId::proportional(12.0),
            visuals.text_color(),
        );
    } else {
        painter.text(
            top,
            Align2::CENTER_CENTER,
            &tab.name,
            FontId::proportional(12.5),
            visuals.strong_text_color(),
        );
    }
    let weak = visuals.weak_text_color();
    let galley = painter.layout_no_wrap(tab.head_text(), FontId::proportional(12.0), weak);
    let text_width = galley.size().x;
    let left = rect.center().x - (text_width + 32.0) / 2.0;
    let y = rect.bottom() - 11.0;
    icons::paint(
        painter,
        pos2(left + icons::INLINE / 2.0, y),
        icons::INLINE,
        Icon::Branch,
        weak,
    );
    let text_left = left + icons::INLINE + 4.0;
    painter.galley(pos2(text_left, y - galley.size().y / 2.0), galley, weak);
    icons::paint(
        painter,
        pos2(text_left + text_width + 9.0, y),
        icons::SMALL,
        Icon::ChevronDown,
        weak,
    );
    response
}

fn toolbar_divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(5.0, 46.0), Sense::hover());
    let x = rect.center().x;
    ui.painter().line_segment(
        [pos2(x, rect.top() + 8.0), pos2(x, rect.top() + 24.0)],
        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
    );
}

fn shortcut_text(ctx: &egui::Context, menus: &[Menu], command: Command) -> Option<String> {
    menu::shortcuts(menus)
        .into_iter()
        .find(|(_, candidate, _)| *candidate == command)
        .map(|(shortcut, _, _)| ctx.format_shortcut(&shortcut))
}

fn tooltip(ctx: &egui::Context, menus: &[Menu], label: &str, command: Command) -> String {
    match shortcut_text(ctx, menus, command) {
        Some(shortcut) => format!("{label}  ({shortcut})"),
        None => label.to_string(),
    }
}

/// 弹出菜单里执行命令的一项，右侧显示该命令的快捷键。
fn command_item(
    ui: &mut egui::Ui,
    menus: &[Menu],
    command: Command,
    label: &str,
    enabled: bool,
    queue: &mut Vec<Command>,
) {
    let mut button = egui::Button::new(label);
    if let Some(shortcut) = shortcut_text(ui.ctx(), menus, command) {
        button = button.shortcut_text(shortcut);
    }
    if ui.add_enabled(enabled, button).clicked() {
        queue.push(command);
        ui.close();
    }
}
