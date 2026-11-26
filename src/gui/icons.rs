use std::sync::Arc;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Painter,
    Pos2, Rect, Sense, Ui, Vec2, pos2, vec2,
};
use git_forker::ChangeKind;
use lucide_icons::Icon as Lucide;

/// 侧栏行、小按钮、标签页等行内位置的图标尺寸。
pub const INLINE: f32 = 14.0;
/// 工具栏按钮的图标尺寸。
pub const TOOLBAR: f32 = 18.0;
/// 折叠箭头、下拉箭头和引用标签里的小图标尺寸。
pub const SMALL: f32 = 10.0;

const FAMILY: &str = "lucide";

/// 界面用到的图标，统一取自 Lucide 图标集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Sidebar,
    QuickLaunch,
    Fetch,
    Pull,
    Push,
    Stash,
    Branch,
    OpenIn,
    Activity,
    LocalChanges,
    AllCommits,
    Search,
    Check,
    Remote,
    Tag,
    Submodule,
    Folder,
    ChevronRight,
    ChevronDown,
    More,
    Plus,
    Minus,
    Close,
    Revert,
    Copy,
    Pencil,
    ArrowRight,
    Shuffle,
    Alert,
    Settings,
    Diff,
    Terminal,
    History,
    ListTree,
}

impl Icon {
    fn lucide(self) -> Lucide {
        match self {
            Icon::Sidebar => Lucide::PanelLeft,
            Icon::QuickLaunch => Lucide::FolderOpen,
            Icon::Fetch => Lucide::CloudDownload,
            Icon::Pull => Lucide::ArrowDownToLine,
            Icon::Push => Lucide::ArrowUpFromLine,
            Icon::Stash => Lucide::Archive,
            Icon::Branch => Lucide::GitBranch,
            Icon::OpenIn => Lucide::SquareArrowOutUpRight,
            Icon::Activity => Lucide::Activity,
            Icon::LocalChanges => Lucide::FilePen,
            Icon::AllCommits => Lucide::GitCommitHorizontal,
            Icon::Search => Lucide::Search,
            Icon::Check => Lucide::Check,
            Icon::Remote => Lucide::Cloud,
            Icon::Tag => Lucide::Tag,
            Icon::Submodule => Lucide::Package,
            Icon::Folder => Lucide::Folder,
            Icon::ChevronRight => Lucide::ChevronRight,
            Icon::ChevronDown => Lucide::ChevronDown,
            Icon::More => Lucide::Ellipsis,
            Icon::Plus => Lucide::Plus,
            Icon::Minus => Lucide::Minus,
            Icon::Close => Lucide::X,
            Icon::Revert => Lucide::RotateCcw,
            Icon::Copy => Lucide::Copy,
            Icon::Pencil => Lucide::Pencil,
            Icon::ArrowRight => Lucide::ArrowRight,
            Icon::Shuffle => Lucide::Shuffle,
            Icon::Alert => Lucide::TriangleAlert,
            Icon::Settings => Lucide::Settings,
            Icon::Diff => Lucide::FileDiff,
            Icon::Terminal => Lucide::SquareTerminal,
            Icon::History => Lucide::History,
            Icon::ListTree => Lucide::ListTree,
        }
    }

    /// 图标对应的字符。图标字体也是正文字体的后备字体，所以它可以直接拼进普通文字。
    pub fn glyph(self) -> char {
        char::from(self.lucide())
    }

    /// 带图标前缀的文字，例如输入框的提示语。
    pub fn with(self, text: &str) -> String {
        format!("{} {text}", self.glyph())
    }

    pub fn for_change(kind: ChangeKind) -> Self {
        match kind {
            ChangeKind::Added | ChangeKind::Untracked => Icon::Plus,
            ChangeKind::Modified => Icon::Pencil,
            ChangeKind::Deleted => Icon::Minus,
            ChangeKind::Renamed => Icon::ArrowRight,
            ChangeKind::TypeChange => Icon::Shuffle,
            ChangeKind::Conflicted => Icon::Alert,
        }
    }
}

/// 注册图标字体：既是独立的字体族，也紧跟在正文主字体之后作为后备。
/// egui 自带的 emoji 图标字体同样占用私有区码位，必须排在它前面。
pub fn install(fonts: &mut FontDefinitions) {
    fonts.font_data.insert(
        FAMILY.into(),
        Arc::new(FontData::from_static(lucide_icons::LUCIDE_FONT_BYTES)),
    );
    fonts
        .families
        .insert(FontFamily::Name(FAMILY.into()), vec![FAMILY.into()]);
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        if let Some(fonts) = fonts.families.get_mut(&family) {
            fonts.insert(fonts.len().min(1), FAMILY.into());
        }
    }
}

/// 以 `center` 为中心、按 `size` 画图标。按字形的实际笔画居中，不受字体行高影响。
pub fn paint(painter: &Painter, center: Pos2, size: f32, icon: Icon, color: Color32) {
    let galley = painter.layout_no_wrap(
        icon.glyph().to_string(),
        FontId::new(size, FontFamily::Name(FAMILY.into())),
        color,
    );
    let ink = galley
        .rows
        .first()
        .and_then(|row| {
            let glyph = row.row.glyphs.first()?;
            Some(Rect::from_min_size(
                row.pos + glyph.pos.to_vec2() + glyph.uv_rect.offset,
                glyph.uv_rect.size,
            ))
        })
        .filter(|ink| ink.is_positive())
        .unwrap_or(galley.rect);
    painter.galley(center - ink.center().to_vec2(), galley, color);
}

/// 实心小文件夹。线框图标缩到行高里会发虚，实心形状更清楚。
/// 颜色用系统文件夹那种蓝，不用黄文件夹。
pub fn paint_folder(painter: &Painter, center: Pos2, open: bool) {
    let width = 14.0;
    let height = 11.0;
    let top_left = Pos2::new(center.x - width / 2.0, center.y - height / 2.0 + 0.5);
    let tab = Rect::from_min_size(top_left, vec2(6.5, 3.5));
    let body = Rect::from_min_max(
        pos2(top_left.x, top_left.y + 2.0),
        pos2(top_left.x + width, top_left.y + height),
    );
    let tab_color = Color32::from_rgb(0x3e, 0x92, 0xd8);
    let body_color = Color32::from_rgb(0x5a, 0xb6, 0xf6);
    painter.rect_filled(
        tab,
        CornerRadius {
            nw: 2,
            ne: 2,
            sw: 0,
            se: 0,
        },
        tab_color,
    );
    if open {
        painter.rect_filled(
            body,
            CornerRadius {
                nw: 1,
                ne: 2,
                sw: 2,
                se: 2,
            },
            tab_color,
        );
        let flap = Rect::from_min_max(pos2(body.left(), body.top() + 3.5), body.max);
        painter.rect_filled(
            flap,
            CornerRadius {
                nw: 1,
                ne: 1,
                sw: 2,
                se: 2,
            },
            body_color,
        );
    } else {
        painter.rect_filled(
            body,
            CornerRadius {
                nw: 1,
                ne: 2,
                sw: 2,
                se: 2,
            },
            body_color,
        );
    }
}

/// 文件状态徽标：彩色圆角方块里放白色图标，新增是加号，修改是铅笔，删除是减号。
pub fn paint_change(painter: &Painter, center: Pos2, kind: ChangeKind, color: Color32) {
    let badge = Rect::from_center_size(center, Vec2::splat(INLINE + 1.0));
    painter.rect_filled(badge, CornerRadius::same(4), color);
    paint(
        painter,
        center,
        SMALL + 1.0,
        Icon::for_change(kind),
        Color32::WHITE,
    );
}

/// 实心圆底加号。圆一直画出来，用来标出需要比普通图标更醒目的操作。
pub fn circle_plus_button(ui: &mut Ui, tooltip: &str) -> egui::Response {
    let diameter = 24.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(diameter), Sense::click());
    let fill = if response.is_pointer_button_down_on() {
        Color32::from_rgb(0, 112, 230)
    } else if response.hovered() {
        Color32::from_rgb(48, 152, 255)
    } else {
        Color32::from_rgb(10, 132, 255)
    };
    ui.painter()
        .circle_filled(rect.center(), diameter / 2.0, fill);
    paint(
        ui.painter(),
        rect.center(),
        14.0,
        Icon::Plus,
        Color32::WHITE,
    );
    response.on_hover_text(tooltip)
}

/// 只有图标的小按钮。
pub fn icon_button(ui: &mut Ui, icon: Icon, tooltip: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(INLINE + 8.0), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 5.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let color = if response.hovered() {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().text_color()
    };
    paint(ui.painter(), rect.center(), INLINE, icon, color);
    response.on_hover_text(tooltip)
}

/// 工具栏按钮：图标在上，文字在下。`badge` 是右上角的数字角标。
pub fn toolbar_button(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    enabled: bool,
    badge: Option<usize>,
    dropdown: bool,
) -> egui::Response {
    let font = FontId::proportional(11.5);
    let text_width = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), Color32::WHITE)
        .size()
        .x;
    let width = (text_width + 14.0).max(44.0) + if dropdown { 8.0 } else { 0.0 };
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(width, 46.0), sense);
    let visuals = ui.visuals();
    if enabled && response.hovered() {
        ui.painter()
            .rect_filled(rect, 6.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let color = if !enabled {
        visuals.weak_text_color().gamma_multiply(0.6)
    } else {
        visuals.text_color()
    };
    let icon_center = pos2(
        rect.center().x - if dropdown { 4.0 } else { 0.0 },
        rect.top() + 15.0,
    );
    paint(ui.painter(), icon_center, TOOLBAR, icon, color);
    if dropdown {
        paint(
            ui.painter(),
            pos2(icon_center.x + TOOLBAR / 2.0 + 6.0, icon_center.y),
            SMALL,
            Icon::ChevronDown,
            visuals.weak_text_color(),
        );
    }
    ui.painter().text(
        pos2(rect.center().x, rect.bottom() - 8.0),
        Align2::CENTER_CENTER,
        label,
        font,
        if enabled {
            visuals.weak_text_color()
        } else {
            color
        },
    );
    if let Some(count) = badge.filter(|count| *count > 0) {
        let text = if count > 99 {
            "99+".to_string()
        } else {
            count.to_string()
        };
        let galley = ui
            .painter()
            .layout_no_wrap(text, FontId::proportional(9.5), Color32::WHITE);
        let size = vec2(galley.size().x.max(7.0) + 6.0, 13.0);
        let badge = Rect::from_center_size(
            icon_center + vec2(TOOLBAR / 2.0 + 3.0, -TOOLBAR / 2.0 + 1.0),
            size,
        );
        ui.painter().rect_filled(badge, 6.5, Color32::from_gray(95));
        ui.painter()
            .galley(badge.center() - galley.size() / 2.0, galley, Color32::WHITE);
    }
    response
}
