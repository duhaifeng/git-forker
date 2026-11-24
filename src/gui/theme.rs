use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke, Theme, Visuals,
};

/// 工具栏和仓库管理器所在区域的底色。
pub fn chrome_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(44, 41, 42)
    } else {
        Color32::from_rgb(232, 230, 230)
    }
}

/// 仓库管理器侧栏的底色。
pub fn manager_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(52, 38, 40)
    } else {
        Color32::from_rgb(226, 220, 221)
    }
}

/// 标签栏整条胶囊的底色。
pub fn tab_strip_fill(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(56, 53, 54)
    } else {
        Color32::from_rgb(220, 218, 218)
    }
}

/// 列表里选中行的底色。
pub fn row_selection(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(72, 69, 70)
    } else {
        Color32::from_rgb(208, 206, 207)
    }
}

/// 提交历史里选中行的底色。
pub fn history_selection(dark: bool) -> Color32 {
    if dark {
        Color32::from_rgb(38, 79, 140)
    } else {
        Color32::from_rgb(196, 218, 250)
    }
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    ctx.set_visuals_of(Theme::Dark, visuals(true));
    ctx.set_visuals_of(Theme::Light, visuals(false));
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(8.0, 3.0);
        style.spacing.interact_size.y = 22.0;
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(13.0, FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new(13.0, FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::new(12.5, FontFamily::Monospace),
        );
    });
}

fn visuals(dark: bool) -> Visuals {
    let mut visuals = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let (panel, extreme, faint, text) = if dark {
        (
            Color32::from_rgb(37, 35, 36),
            Color32::from_rgb(30, 28, 29),
            Color32::from_rgb(41, 39, 40),
            Color32::from_rgb(222, 220, 221),
        )
    } else {
        (
            Color32::from_rgb(250, 250, 250),
            Color32::WHITE,
            Color32::from_rgb(244, 244, 244),
            Color32::from_rgb(30, 30, 30),
        )
    };
    visuals.panel_fill = panel;
    visuals.window_fill = if dark {
        Color32::from_rgb(46, 44, 45)
    } else {
        Color32::from_rgb(246, 246, 246)
    };
    visuals.extreme_bg_color = extreme;
    visuals.faint_bg_color = faint;
    visuals.override_text_color = Some(text);
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.menu_corner_radius = CornerRadius::same(7);
    let accent = Color32::from_rgb(10, 132, 255);
    visuals.selection.bg_fill = if dark {
        Color32::from_rgb(38, 79, 140)
    } else {
        Color32::from_rgb(180, 210, 250)
    };
    visuals.selection.stroke = Stroke::new(1.0, if dark { Color32::WHITE } else { Color32::BLACK });
    visuals.hyperlink_color = accent;
    let border = if dark {
        Color32::from_rgb(64, 61, 62)
    } else {
        Color32::from_rgb(205, 203, 204)
    };
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, border);
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(6);
    }
    if dark {
        visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(62, 59, 60);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(62, 59, 60);
        visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(70, 67, 68);
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(76, 73, 74);
        visuals.widgets.active.weak_bg_fill = Color32::from_rgb(84, 81, 82);
    } else {
        visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(232, 231, 231);
        visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(222, 221, 221);
        visuals.widgets.active.weak_bg_fill = Color32::from_rgb(210, 209, 209);
    }
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, border);
    visuals.widgets.hovered.bg_stroke = Stroke::NONE;
    visuals
}

/// 优先使用系统界面字体，找不到时保留 egui 自带字体；图标字体总是随程序内置。
fn install_fonts(ctx: &egui::Context) {
    let candidates: &[&str] = if cfg!(target_os = "macos") {
        &[
            "/System/Library/Fonts/SFNS.ttf",
            "/System/Library/Fonts/Helvetica.ttc",
        ]
    } else if cfg!(target_os = "windows") {
        &["C:\\Windows\\Fonts\\segoeui.ttf"]
    } else {
        &[
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/noto/NotoSans-Regular.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        ]
    };
    let mut fonts = FontDefinitions::default();
    if let Some(bytes) = candidates.iter().find_map(|path| std::fs::read(path).ok()) {
        fonts
            .font_data
            .insert("system-ui".into(), Arc::new(FontData::from_owned(bytes)));
        if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
            family.insert(0, "system-ui".into());
        }
    }
    super::icons::install(&mut fonts);
    ctx.set_fonts(fonts);
}
