use chrono::{Local, TimeZone};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Stroke, Ui, Vec2, epaint,
};
use git_forker::tr;
use git_forker::{ChangeKind, EdgeKind, GraphRow, RefKind, RefLabel};

use super::icons::{self, Icon};

pub const ROW_HEIGHT: f32 = 22.0;
pub const LANE_WIDTH: f32 = 11.0;

const LANE_COLORS: [Color32; 8] = [
    Color32::from_rgb(0xf0, 0x8c, 0x2e),
    Color32::from_rgb(0x3a, 0x9b, 0xf0),
    Color32::from_rgb(0xb0, 0x6c, 0xe8),
    Color32::from_rgb(0x4c, 0xbf, 0x6e),
    Color32::from_rgb(0xe8, 0x56, 0x8a),
    Color32::from_rgb(0x2c, 0xc0, 0xc0),
    Color32::from_rgb(0xe0, 0xc0, 0x30),
    Color32::from_rgb(0xe0, 0x50, 0x48),
];

pub const ADDED_BG: Color32 = Color32::from_rgba_unmultiplied_const(60, 170, 90, 45);
pub const REMOVED_BG: Color32 = Color32::from_rgba_unmultiplied_const(220, 70, 80, 45);
pub const SELECTED_LINE_BG: Color32 = Color32::from_rgba_unmultiplied_const(70, 130, 240, 90);

pub fn lane_color(index: usize) -> Color32 {
    LANE_COLORS[index % LANE_COLORS.len()]
}

pub fn format_time(seconds: i64) -> String {
    Local
        .timestamp_opt(seconds, 0)
        .single()
        .map(|time| time.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

/// 今天、昨天的提交只显示时刻，其余显示完整日期。
pub fn relative_time(seconds: i64) -> String {
    let Some(time) = Local.timestamp_opt(seconds, 0).single() else {
        return String::new();
    };
    let today = Local::now().date_naive();
    let date = time.date_naive();
    if date == today {
        time.format(tr!("今天 %H:%M")).to_string()
    } else if today.pred_opt() == Some(date) {
        time.format(tr!("昨天 %H:%M")).to_string()
    } else {
        time.format("%Y/%m/%d %H:%M").to_string()
    }
}

/// 不联网的头像：按邮箱取色，画出姓名首字母。
pub fn paint_avatar(painter: &egui::Painter, center: Pos2, radius: f32, name: &str, email: &str) {
    const AVATAR_COLORS: [Color32; 8] = [
        Color32::from_rgb(0x5b, 0x8d, 0xd9),
        Color32::from_rgb(0xd9, 0x7b, 0x5b),
        Color32::from_rgb(0x6c, 0xb0, 0x6c),
        Color32::from_rgb(0xa8, 0x72, 0xd0),
        Color32::from_rgb(0xd0, 0x6c, 0x92),
        Color32::from_rgb(0x4f, 0xaa, 0xb0),
        Color32::from_rgb(0xc2, 0x9a, 0x3e),
        Color32::from_rgb(0x7d, 0x86, 0x96),
    ];
    let key = if email.is_empty() { name } else { email };
    let hash = key.to_lowercase().bytes().fold(0u32, |hash, byte| {
        hash.wrapping_mul(31).wrapping_add(byte as u32)
    });
    painter.circle_filled(
        center,
        radius,
        AVATAR_COLORS[hash as usize % AVATAR_COLORS.len()],
    );
    let initials: String = name
        .split_whitespace()
        .filter_map(|word| word.chars().next())
        .take(2)
        .flat_map(char::to_uppercase)
        .collect();
    painter.text(
        center,
        Align2::CENTER_CENTER,
        if initials.is_empty() {
            "?".into()
        } else {
            initials
        },
        FontId::proportional(radius * 0.9),
        Color32::WHITE,
    );
}

/// 圆角分段控件，类似 macOS 的 NSSegmentedControl。
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str)]) {
    let visuals = ui.visuals().clone();
    let font = FontId::proportional(12.5);
    let widths: Vec<f32> = options
        .iter()
        .map(|(_, text)| {
            ui.painter()
                .layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE)
                .size()
                .x
                + 28.0
        })
        .collect();
    let total = widths.iter().sum::<f32>();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, 22.0), egui::Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(6), visuals.extreme_bg_color);
    painter.rect_stroke(
        rect,
        CornerRadius::same(6),
        Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color),
        egui::StrokeKind::Inside,
    );
    let mut left = rect.left();
    for ((option, text), width) in options.iter().zip(widths) {
        let segment = Rect::from_min_size(Pos2::new(left, rect.top()), Vec2::new(width, 22.0));
        let response = ui.interact(segment, ui.scope_id().with(text), egui::Sense::click());
        let selected = *value == *option;
        if selected {
            ui.painter().rect_filled(
                segment.shrink(2.0),
                CornerRadius::same(5),
                visuals.widgets.active.weak_bg_fill,
            );
        }
        let color = if selected {
            visuals.strong_text_color()
        } else {
            visuals.text_color()
        };
        ui.painter().text(
            segment.center(),
            Align2::CENTER_CENTER,
            *text,
            font.clone(),
            color,
        );
        if response.clicked() {
            *value = *option;
        }
        left += width;
    }
}

pub fn change_color(kind: ChangeKind) -> Color32 {
    match kind {
        ChangeKind::Added | ChangeKind::Untracked => Color32::from_rgb(0x4c, 0xb7, 0x82),
        ChangeKind::Deleted => Color32::from_rgb(0xe8, 0x5d, 0x75),
        ChangeKind::Renamed | ChangeKind::TypeChange => Color32::from_rgb(0xa3, 0x71, 0xf7),
        ChangeKind::Conflicted => Color32::from_rgb(0xf2, 0x6b, 0x3a),
        ChangeKind::Modified => Color32::from_rgb(0xf2, 0xa9, 0x3b),
    }
}

pub fn change_name(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::Added => tr!("新增"),
        ChangeKind::Modified => tr!("修改"),
        ChangeKind::Deleted => tr!("删除"),
        ChangeKind::Renamed => tr!("重命名"),
        ChangeKind::TypeChange => tr!("类型变更"),
        ChangeKind::Untracked => tr!("未跟踪"),
        ChangeKind::Conflicted => tr!("冲突"),
    }
}

/// 画出文件状态徽标。
pub fn paint_change_badge(ui: &Ui, rect: Rect, kind: ChangeKind) {
    icons::paint_change(ui.painter(), rect.center(), kind, change_color(kind));
}

/// 在一行里画出提交图。
pub fn paint_graph_row(ui: &Ui, rect: Rect, row: &GraphRow, merge: bool, is_head: bool) {
    let painter = ui.painter().with_clip_rect(rect);
    let x = |column: usize| rect.left() + LANE_WIDTH * column as f32 + LANE_WIDTH / 2.0;
    let top = rect.top();
    let mid = rect.center().y;
    let bottom = rect.bottom();
    let width = 1.2;

    for edge in &row.edges {
        let stroke = Stroke::new(width, lane_color(edge.color));
        let (start, end) = match edge.kind {
            EdgeKind::Through => (Pos2::new(x(edge.from), top), Pos2::new(x(edge.to), bottom)),
            EdgeKind::TopToNode => (Pos2::new(x(edge.from), top), Pos2::new(x(edge.to), mid)),
            EdgeKind::NodeToBottom => (Pos2::new(x(edge.from), mid), Pos2::new(x(edge.to), bottom)),
        };
        if (start.x - end.x).abs() < 0.5 {
            painter.line_segment([start, end], stroke);
        } else {
            let half = (end.y - start.y) / 2.0;
            let curve = epaint::CubicBezierShape::from_points_stroke(
                [
                    start,
                    Pos2::new(start.x, start.y + half),
                    Pos2::new(end.x, end.y - half),
                    end,
                ],
                false,
                Color32::TRANSPARENT,
                stroke,
            );
            painter.add(curve);
        }
    }

    let center = Pos2::new(x(row.node), mid);
    let color = lane_color(row.color);
    if merge {
        painter.circle(
            center,
            3.0,
            ui.visuals().panel_fill,
            Stroke::new(1.2, color),
        );
    } else {
        painter.circle(
            center,
            3.4,
            color,
            Stroke::new(1.0, ui.visuals().panel_fill),
        );
    }
    if is_head {
        painter.circle_stroke(center, 5.5, Stroke::new(1.0, color));
    }
}

/// 从 `left` 开始画引用标签（描边、图标、跟随泳道颜色），返回标签之后的横坐标。
pub fn paint_ref_labels(
    ui: &Ui,
    clip: Rect,
    mut left: f32,
    labels: &[RefLabel],
    lane: Color32,
) -> f32 {
    let painter = ui.painter().with_clip_rect(clip);
    let text_color = ui.visuals().strong_text_color();
    for label in labels {
        let (icon, color) = match label.kind {
            RefKind::Head => (Some(Icon::Check), lane),
            RefKind::LocalBranch => (None, lane),
            RefKind::RemoteBranch => (Some(Icon::Remote), lane.gamma_multiply(0.75)),
            RefKind::Tag => (Some(Icon::Tag), Color32::from_rgb(0x9a, 0x9a, 0x9a)),
            RefKind::Stash => (Some(Icon::Stash), Color32::from_rgb(0x9a, 0x9a, 0x9a)),
        };
        let galley = painter.layout_no_wrap(
            middle_ellipsis(&label.name, MAX_LABEL_CHARS),
            FontId::proportional(11.5),
            text_color,
        );
        let icon_w = if icon.is_some() { 13.0 } else { 0.0 };
        let size = Vec2::new(galley.size().x + icon_w + 10.0, 17.0);
        let chip = Rect::from_min_size(Pos2::new(left, clip.center().y - size.y / 2.0), size);
        painter.rect_filled(chip, CornerRadius::same(4), color.gamma_multiply(0.35));
        painter.rect_stroke(
            chip,
            CornerRadius::same(4),
            Stroke::new(1.0, color),
            egui::StrokeKind::Inside,
        );
        if let Some(icon) = icon {
            let center = Pos2::new(chip.left() + 5.0 + icons::SMALL / 2.0, chip.center().y);
            icons::paint(&painter, center, icons::SMALL, icon, text_color);
        }
        let text_pos = Pos2::new(
            chip.left() + 5.0 + icon_w,
            chip.center().y - galley.size().y / 2.0,
        );
        painter.galley(text_pos, galley, text_color);
        left = chip.right() + 4.0;
        if left > clip.right() {
            break;
        }
    }
    left + 2.0
}

/// 引用标签最多显示的字符数，更长的名字从中间省略。
const MAX_LABEL_CHARS: usize = 30;

fn middle_ellipsis(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        return text.to_string();
    }
    let head = max / 2;
    let tail = max - head - 1;
    let start: String = text.chars().take(head).collect();
    let end: String = text.chars().skip(count - tail).collect();
    format!("{start}…{end}")
}

/// 只画单行文本，超出 `rect` 的部分被裁掉。
pub fn paint_clipped_text(
    ui: &Ui,
    rect: Rect,
    text: &str,
    font: FontId,
    color: Color32,
    align_right: bool,
) {
    let painter = ui.painter().with_clip_rect(rect);
    let (pos, align) = if align_right {
        (
            Pos2::new(rect.right(), rect.center().y),
            Align2::RIGHT_CENTER,
        )
    } else {
        (Pos2::new(rect.left(), rect.center().y), Align2::LEFT_CENTER)
    };
    painter.text(pos, align, text, font, color);
}
