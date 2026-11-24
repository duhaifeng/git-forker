use std::path::PathBuf;
use std::process::Command;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Painter, Rect, RichText, Sense, Stroke,
    StrokeKind, Ui, pos2, vec2,
};
use git_forker::i18n::Language;
use git_forker::{git_output, git_program, set_git_program, tr};

use super::icons::{self, Icon};
use super::platform;
use super::state::{AppState, Appearance};

const WIDTH: f32 = 640.0;
const HEIGHT: f32 = 440.0;
/// 左侧标签列的宽度，标签右对齐到这条线。
const LABEL_WIDTH: f32 = 160.0;
const FIELD_WIDTH: f32 = 280.0;
const ACCENT: Color32 = Color32::from_rgb(10, 132, 255);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    General,
    Git,
    Diff,
    Integration,
}

/// 设置窗口。语言、外观等都写回 `AppState`，由调用方同步到已打开的仓库页。
pub struct SettingsWindow {
    page: Page,
    /// 上次检测版本时用的 git 路径和结果，路径变了才重新检测。
    git_version: Option<(String, Result<String, String>)>,
    identity: Identity,
    terminal_error: Option<String>,
}

/// 全局 git 配置里的提交者身份。
struct Identity {
    name: String,
    email: String,
    saved_name: String,
    saved_email: String,
    message: Option<(String, bool)>,
}

impl Identity {
    fn load() -> Self {
        let name = global_config("user.name");
        let email = global_config("user.email");
        Self {
            saved_name: name.clone(),
            saved_email: email.clone(),
            name,
            email,
            message: None,
        }
    }

    fn dirty(&self) -> bool {
        self.name.trim() != self.saved_name || self.email.trim() != self.saved_email
    }

    fn save(&mut self) {
        let name = self.name.trim().to_string();
        let email = self.email.trim().to_string();
        let result = set_global_config("user.name", &name)
            .and_then(|()| set_global_config("user.email", &email));
        self.message = Some(match result {
            Ok(()) => {
                self.saved_name = name;
                self.saved_email = email;
                (tr!("已保存到全局 git 配置").to_string(), false)
            }
            Err(err) => (err, true),
        });
    }
}

fn config_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(std::env::temp_dir)
}

fn global_config(key: &str) -> String {
    git_output(&config_dir(), &["config", "--global", "--get", key])
        .map(|value| value.trim().to_string())
        .unwrap_or_default()
}

fn set_global_config(key: &str, value: &str) -> Result<(), String> {
    let args: &[&str] = if value.is_empty() {
        &["config", "--global", "--unset", key]
    } else {
        &["config", "--global", key, value]
    };
    match git_output(&config_dir(), args) {
        Ok(_) => Ok(()),
        // 删除本来就不存在的键时 git 返回失败，这种情况等同成功。
        Err(_) if value.is_empty() && global_config(key).is_empty() => Ok(()),
        Err(err) => Err(err),
    }
}

fn detect_git_version() -> Result<String, String> {
    let output = Command::new(git_program())
        .arg("--version")
        .output()
        .map_err(|err| tr!("无法启动 git: {err}", err = err))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

impl SettingsWindow {
    pub fn new() -> Self {
        Self {
            page: Page::General,
            git_version: None,
            identity: Identity::load(),
            terminal_error: None,
        }
    }

    /// 显示窗口。返回 false 表示用户关闭了窗口。
    pub fn show(&mut self, ctx: &egui::Context, state: &mut AppState) -> bool {
        // 有下拉菜单打开时，Esc 只关闭菜单。
        let mut open = egui::Popup::is_any_open(ctx)
            || !ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        egui::Window::new(tr!("设置"))
            .id(egui::Id::unique("settings-window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(vec2(WIDTH, HEIGHT))
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                self.page_bar(ui);
                ui.add_space(2.0);
                ui.separator();
                let footer = 26.0;
                egui::ScrollArea::vertical()
                    .auto_shrink(false)
                    .max_height(ui.available_height() - footer)
                    .show(ui, |ui| {
                        ui.add_space(8.0);
                        match self.page {
                            Page::General => general_page(ui, state),
                            Page::Git => self.git_page(ui, state),
                            Page::Diff => diff_page(ui, state),
                            Page::Integration => self.integration_page(ui, state),
                        }
                        ui.add_space(8.0);
                    });
                ui.separator();
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new(tr!("设置会立即生效并在下次启动时保留。"))
                            .size(11.5)
                            .color(ui.visuals().weak_text_color()),
                    );
                });
            });
        open
    }

    /// 顶部的图标分页，样式参考 macOS 的偏好设置工具栏。
    fn page_bar(&mut self, ui: &mut Ui) {
        let pages = [
            (Page::General, Icon::Settings, tr!("通用")),
            (Page::Git, Icon::Branch, "Git"),
            (Page::Diff, Icon::Diff, tr!("差异")),
            (Page::Integration, Icon::Terminal, tr!("集成")),
        ];
        let item = vec2(84.0, 56.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let total = item.x * pages.len() as f32 + 4.0 * (pages.len() - 1) as f32;
            ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
            for (page, icon, label) in pages {
                let (rect, response) = ui.allocate_exact_size(item, Sense::click());
                let selected = self.page == page;
                let visuals = ui.visuals();
                if selected {
                    ui.painter()
                        .rect_filled(rect, 8.0, visuals.widgets.active.weak_bg_fill);
                } else if response.hovered() {
                    ui.painter()
                        .rect_filled(rect, 8.0, visuals.widgets.hovered.weak_bg_fill);
                }
                let (icon_color, text_color) = if selected {
                    (ACCENT, visuals.strong_text_color())
                } else {
                    (visuals.text_color(), visuals.text_color())
                };
                icons::paint(
                    ui.painter(),
                    pos2(rect.center().x, rect.top() + 21.0),
                    22.0,
                    icon,
                    icon_color,
                );
                ui.painter().text(
                    pos2(rect.center().x, rect.bottom() - 11.0),
                    Align2::CENTER_CENTER,
                    label,
                    FontId::proportional(12.0),
                    text_color,
                );
                if response.clicked() {
                    self.page = page;
                }
            }
        });
    }

    fn git_page(&mut self, ui: &mut Ui, state: &mut AppState) {
        section(ui, tr!("Git 程序"));
        row(ui, tr!("可执行文件"), |ui| {
            let edit = ui.add(
                egui::TextEdit::singleline(&mut state.preferences.git_path)
                    .hint_text(tr!("使用 PATH 里的 git"))
                    .desired_width(FIELD_WIDTH - 70.0),
            );
            if edit.changed() {
                set_git_program(Some(&state.preferences.git_path));
            }
            if ui.button(tr!("浏览…")).clicked()
                && let Some(file) = rfd::FileDialog::new()
                    .set_title(tr!("选择 git 可执行文件"))
                    .pick_file()
            {
                state.preferences.git_path = file.display().to_string();
                set_git_program(Some(&state.preferences.git_path));
            }
        });
        let path = state.preferences.git_path.clone();
        if self
            .git_version
            .as_ref()
            .is_none_or(|(checked, _)| *checked != path)
        {
            self.git_version = Some((path, detect_git_version()));
        }
        if let Some((_, version)) = &self.git_version {
            match version {
                Ok(version) => status(ui, &format!("{} {version}", Icon::Check.glyph()), false),
                Err(err) => status(ui, &format!("{} {err}", Icon::Alert.glyph()), true),
            }
        }

        section(ui, tr!("提交者身份"));
        row(ui, tr!("姓名"), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.identity.name)
                    .hint_text(tr!("例如 Zhang San"))
                    .desired_width(FIELD_WIDTH),
            );
        });
        row(ui, tr!("邮箱"), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.identity.email)
                    .hint_text("name@example.com")
                    .desired_width(FIELD_WIDTH),
            );
        });
        row(ui, "", |ui| {
            let dirty = self.identity.dirty();
            if ui
                .add_enabled(dirty, egui::Button::new(tr!("保存")))
                .clicked()
            {
                self.identity.save();
            }
            if ui
                .add_enabled(dirty, egui::Button::new(tr!("还原")))
                .clicked()
            {
                self.identity.name = self.identity.saved_name.clone();
                self.identity.email = self.identity.saved_email.clone();
                self.identity.message = None;
            }
        });
        if let Some((message, error)) = &self.identity.message {
            status(ui, message, *error);
        }
        hint(
            ui,
            tr!("写入全局 git 配置（~/.gitconfig），没有单独设置身份的仓库都会使用它。"),
        );
    }

    fn integration_page(&mut self, ui: &mut Ui, state: &mut AppState) {
        section(ui, tr!("终端"));
        row(ui, tr!("终端程序"), |ui| {
            ui.add(
                egui::TextEdit::singleline(&mut state.preferences.terminal)
                    .hint_text(tr!("系统默认"))
                    .desired_width(FIELD_WIDTH - 34.0),
            );
            ui.menu_button(Icon::ChevronDown.glyph().to_string(), |ui| {
                if ui.button(tr!("系统默认")).clicked() {
                    state.preferences.terminal.clear();
                    ui.close();
                }
                ui.separator();
                for preset in platform::TERMINAL_PRESETS {
                    if ui.button(*preset).clicked() {
                        state.preferences.terminal = preset.to_string();
                        ui.close();
                    }
                }
            });
        });
        hint(
            ui,
            if cfg!(target_os = "macos") {
                tr!("填写应用名称，例如 iTerm；留空时使用系统自带的「终端」。")
            } else {
                tr!("填写终端命令，例如 gnome-terminal；留空时自动查找可用的终端。")
            },
        );
        row(ui, "", |ui| {
            if ui.button(tr!("在主目录中试用")).clicked() {
                self.terminal_error =
                    platform::open_terminal(&config_dir(), &state.preferences.terminal).err();
            }
        });
        if let Some(err) = &self.terminal_error {
            status(ui, err, true);
        }
    }
}

fn general_page(ui: &mut Ui, state: &mut AppState) {
    section(ui, tr!("界面"));
    row(ui, tr!("语言"), |ui| {
        egui::ComboBox::from_id_salt("settings-language")
            .selected_text(state.language.native_name())
            .width(200.0)
            .show_ui(ui, |ui| {
                for option in Language::ALL {
                    ui.selectable_value(&mut state.language, option, option.native_name());
                }
            });
    });
    row(ui, tr!("外观"), |ui| {
        appearance_picker(ui, &mut state.appearance)
    });

    section(ui, tr!("行为"));
    row(ui, tr!("启动"), |ui| {
        ui.checkbox(
            &mut state.preferences.restore_tabs,
            tr!("重新打开上次的仓库标签页"),
        );
    });
    row(ui, tr!("刷新"), |ui| {
        ui.checkbox(
            &mut state.preferences.refresh_on_focus,
            tr!("窗口回到前台时刷新当前仓库"),
        );
    });
}

fn diff_page(ui: &mut Ui, state: &mut AppState) {
    let preferences = &mut state.preferences;
    section(ui, tr!("差异显示"));
    row(ui, tr!("上下文"), |ui| {
        ui.add(
            egui::DragValue::new(&mut preferences.context_lines)
                .range(0..=100)
                .speed(0.2),
        );
        ui.label(tr!("行"));
    });
    row(ui, tr!("空白"), |ui| {
        ui.checkbox(&mut preferences.ignore_whitespace, tr!("忽略空白变化"));
    });
    row(ui, tr!("换行"), |ui| {
        ui.checkbox(&mut preferences.wrap_lines, tr!("长行自动换行"));
    });
    row(ui, tr!("着色"), |ui| {
        ui.checkbox(&mut preferences.syntax_highlighting, tr!("语法着色"));
    });
    hint(
        ui,
        tr!(
            "这些是仓库页的默认值，修改后会应用到所有已打开的仓库页；在仓库页里临时调整只影响当前页。"
        ),
    );
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.add_space(24.0);
        ui.label(RichText::new(title).size(12.0).strong());
    });
    ui.add_space(4.0);
}

/// 一行设置：右对齐的标签加右侧控件，参考 macOS 偏好设置的排版。
fn row(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(LABEL_WIDTH, 22.0), Sense::hover());
        if !label.is_empty() {
            ui.painter().text(
                pos2(rect.right(), rect.center().y),
                Align2::RIGHT_CENTER,
                format!("{label}:"),
                FontId::proportional(13.0),
                ui.visuals().text_color(),
            );
        }
        ui.add_space(6.0);
        add(ui);
    });
    ui.add_space(4.0);
}

/// 控件下方的说明文字，与控件左边缘对齐。
fn hint(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(LABEL_WIDTH + 6.0 + ui.spacing().item_spacing.x);
        ui.add(
            egui::Label::new(
                RichText::new(text)
                    .size(11.5)
                    .color(ui.visuals().weak_text_color()),
            )
            .wrap(),
        );
    });
    ui.add_space(4.0);
}

fn status(ui: &mut Ui, text: &str, error: bool) {
    ui.horizontal(|ui| {
        ui.add_space(LABEL_WIDTH + 6.0 + ui.spacing().item_spacing.x);
        let color = if error {
            ui.visuals().error_fg_color
        } else {
            Color32::from_rgb(0x4c, 0xb7, 0x82)
        };
        ui.add(egui::Label::new(RichText::new(text).size(12.0).color(color)).wrap());
    });
    ui.add_space(4.0);
}

fn appearance_picker(ui: &mut Ui, value: &mut Appearance) {
    ui.spacing_mut().item_spacing.x = 14.0;
    for (option, label) in [
        (Appearance::System, tr!("跟随系统")),
        (Appearance::Light, tr!("浅色")),
        (Appearance::Dark, tr!("深色")),
    ] {
        let (rect, response) = ui.allocate_exact_size(vec2(92.0, 82.0), Sense::click());
        let thumbnail = Rect::from_min_size(rect.min, vec2(rect.width(), 60.0));
        paint_thumbnail(ui.painter(), thumbnail, option);
        let selected = *value == option;
        let visuals = ui.visuals();
        let stroke = if selected {
            Stroke::new(2.5, ACCENT)
        } else if response.hovered() {
            Stroke::new(1.0, visuals.weak_text_color())
        } else {
            visuals.widgets.noninteractive.bg_stroke
        };
        ui.painter()
            .rect_stroke(thumbnail, 7.0, stroke, StrokeKind::Outside);
        ui.painter().text(
            pos2(rect.center().x, rect.bottom() - 9.0),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(12.0),
            if selected {
                visuals.strong_text_color()
            } else {
                visuals.text_color()
            },
        );
        if response.clicked() {
            *value = option;
        }
    }
}

/// 外观选项的缩略图：一个迷你窗口，跟随系统时左半浅色、右半深色。
fn paint_thumbnail(painter: &Painter, rect: Rect, appearance: Appearance) {
    let draw = |painter: &Painter, dark: bool| {
        let rgb = Color32::from_rgb;
        let (window, toolbar, sidebar, line) = if dark {
            (
                rgb(37, 35, 36),
                rgb(44, 41, 42),
                rgb(52, 38, 40),
                rgb(88, 85, 86),
            )
        } else {
            (
                rgb(250, 250, 250),
                rgb(232, 230, 230),
                rgb(226, 220, 221),
                rgb(204, 202, 203),
            )
        };
        painter.rect_filled(rect, 7.0, window);
        let toolbar_rect = Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 12.0));
        painter.rect_filled(
            toolbar_rect,
            CornerRadius {
                nw: 7,
                ne: 7,
                sw: 0,
                se: 0,
            },
            toolbar,
        );
        let sidebar_rect = Rect::from_min_max(
            pos2(rect.left(), rect.top() + 12.0),
            pos2(rect.left() + 26.0, rect.bottom()),
        );
        painter.rect_filled(
            sidebar_rect,
            CornerRadius {
                nw: 0,
                ne: 0,
                sw: 7,
                se: 0,
            },
            sidebar,
        );
        for (index, color) in [rgb(255, 95, 87), rgb(254, 188, 46), rgb(40, 200, 64)]
            .into_iter()
            .enumerate()
        {
            painter.circle_filled(
                pos2(rect.left() + 7.0 + index as f32 * 6.0, rect.top() + 6.0),
                2.0,
                color,
            );
        }
        painter.rect_filled(
            Rect::from_min_size(pos2(rect.left() + 4.0, rect.top() + 18.0), vec2(18.0, 5.0)),
            2.5,
            ACCENT,
        );
        for (index, width) in [48.0, 38.0, 52.0, 30.0].into_iter().enumerate() {
            let top = rect.top() + 19.0 + index as f32 * 9.0;
            painter.rect_filled(
                Rect::from_min_size(pos2(rect.left() + 32.0, top), vec2(width, 4.0)),
                2.0,
                line,
            );
        }
    };
    match appearance {
        Appearance::Light => draw(painter, false),
        Appearance::Dark => draw(painter, true),
        Appearance::System => {
            let middle = rect.center().x;
            draw(
                &painter.with_clip_rect(Rect::from_min_max(rect.min, pos2(middle, rect.bottom()))),
                false,
            );
            draw(
                &painter.with_clip_rect(Rect::from_min_max(pos2(middle, rect.top()), rect.max)),
                true,
            );
        }
    }
}
