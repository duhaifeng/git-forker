use git_forker::tr;
use std::collections::BTreeSet;

use eframe::egui::{
    self, Align, Color32, FontId, Layout, Pos2, Sense, TextFormat, Ui, UiBuilder, Vec2,
    text::LayoutJob,
};
use git_forker::{DiffLine, FileDiff, LineKind};

use super::syntax::{self, Span};
use super::widgets::{ADDED_BG, REMOVED_BG, SELECTED_LINE_BG};

const ADDED_WORD_BG: Color32 = Color32::from_rgba_unmultiplied_const(70, 190, 100, 140);
const REMOVED_WORD_BG: Color32 = Color32::from_rgba_unmultiplied_const(230, 80, 90, 140);

const LINE_HEIGHT: f32 = 18.0;
const HEADER_HEIGHT: f32 = 24.0;

/// 差异所在的位置，决定可以执行哪些区块操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffMode {
    ReadOnly,
    Unstaged,
    Staged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HunkOp {
    Stage,
    Unstage,
    Discard,
}

/// 用户在差异里请求的区块或行操作。`lines` 为 `None` 表示整个区块。
pub struct DiffRequest {
    pub hunk: usize,
    pub lines: Option<BTreeSet<usize>>,
    pub op: HunkOp,
}

#[derive(Default)]
pub struct DiffSelection {
    lines: BTreeSet<(usize, usize)>,
    anchor: Option<(usize, usize)>,
}

impl DiffSelection {
    pub fn clear(&mut self) {
        self.lines.clear();
        self.anchor = None;
    }

    fn in_hunk(&self, hunk: usize) -> BTreeSet<usize> {
        self.lines
            .iter()
            .filter(|(h, _)| *h == hunk)
            .map(|(_, line)| *line)
            .collect()
    }
}

#[derive(Clone, Copy)]
enum Row {
    Header(usize),
    Line(usize, usize),
}

pub struct DiffViewOptions {
    pub mode: DiffMode,
    /// 新文件、删除或二进制文件只支持整文件操作。
    pub hunk_ops: bool,
    pub wrap: bool,
    /// 按文件类型给关键字、字符串、注释和数字上色。
    pub syntax: bool,
}

pub fn show(
    ui: &mut Ui,
    diff: &FileDiff,
    options: &DiffViewOptions,
    selection: &mut DiffSelection,
) -> Option<DiffRequest> {
    if diff.binary {
        ui.centered_and_justified(|ui| ui.label(tr!("二进制文件，无法显示文本差异")));
        return None;
    }
    if diff.hunks.is_empty() {
        ui.centered_and_justified(|ui| ui.weak(tr!("没有文本差异")));
        return None;
    }

    let rows: Vec<Row> = diff
        .hunks
        .iter()
        .enumerate()
        .flat_map(|(h, hunk)| {
            std::iter::once(Row::Header(h))
                .chain((0..hunk.lines.len()).map(move |l| Row::Line(h, l)))
        })
        .collect();

    let max_line = diff
        .hunks
        .iter()
        .flat_map(|hunk| hunk.lines.iter())
        .flat_map(|line| [line.old_no, line.new_no])
        .flatten()
        .max()
        .unwrap_or(1);
    let digits = max_line.to_string().len().max(3) as f32;
    let font = FontId::monospace(12.5);
    let char_width = ui.ctx().fonts_mut(|fonts| fonts.glyph_width(&font, '0'));
    let gutter = char_width * digits;

    let styles = prepare(diff, options.syntax);
    let mut request = None;
    let mut ctx = RowContext {
        diff,
        options,
        selection,
        styles: &styles,
        font,
        gutter,
        request: &mut request,
    };

    if options.wrap {
        egui::ScrollArea::vertical()
            .id_salt("diff-wrap")
            .auto_shrink(false)
            .show(ui, |ui| {
                for row in &rows {
                    ctx.row(ui, *row);
                }
            });
    } else {
        // 无换行时每行高度固定，只布局可见行。
        egui::ScrollArea::both()
            .id_salt("diff-nowrap")
            .auto_shrink(false)
            .show_rows(ui, LINE_HEIGHT, rows.len(), |ui, range| {
                for row in &rows[range] {
                    ctx.row(ui, *row);
                }
            });
    }

    if let Some(text) = diff_footer(diff) {
        ui.weak(text);
    }
    request
}

fn diff_footer(diff: &FileDiff) -> Option<String> {
    (diff.omitted_lines > 0).then(|| tr!("差异过大，已省略 {count} 行", count = diff.omitted_lines))
}

struct LineStyle {
    spans: Vec<Span>,
    /// 这一行里真正发生变化的字节区间，相对于展开制表符之后的文本。
    word: Option<(usize, usize)>,
}

struct RowContext<'a> {
    diff: &'a FileDiff,
    options: &'a DiffViewOptions,
    selection: &'a mut DiffSelection,
    styles: &'a [Vec<LineStyle>],
    font: FontId,
    gutter: f32,
    request: &'a mut Option<DiffRequest>,
}

impl RowContext<'_> {
    fn row(&mut self, ui: &mut Ui, row: Row) {
        match row {
            Row::Header(h) => self.header(ui, h),
            Row::Line(h, l) => self.line(ui, h, l),
        }
    }

    fn header(&mut self, ui: &mut Ui, h: usize) {
        let hunk = &self.diff.hunks[h];
        let height = if self.options.wrap {
            HEADER_HEIGHT
        } else {
            LINE_HEIGHT
        };
        let width = ui.available_width().max(400.0);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
        ui.painter()
            .rect_filled(rect, 0.0, ui.visuals().faint_bg_color);
        let mut child = ui.new_child(
            UiBuilder::new()
                .max_rect(rect.shrink2(Vec2::new(6.0, 0.0)))
                .layout(Layout::left_to_right(Align::Center)),
        );
        child.label(
            egui::RichText::new(&hunk.header)
                .monospace()
                .size(11.5)
                .color(child.visuals().weak_text_color()),
        );
        if self.options.mode == DiffMode::ReadOnly || !self.options.hunk_ops {
            return;
        }
        let selected = self.selection.in_hunk(h);
        child.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let mut push = |op: HunkOp, lines: bool| {
                *self.request = Some(DiffRequest {
                    hunk: h,
                    lines: lines.then(|| selected.clone()),
                    op,
                });
            };
            match self.options.mode {
                DiffMode::Unstaged => {
                    if !selected.is_empty() {
                        if ui.small_button(tr!("放弃所选行")).clicked() {
                            push(HunkOp::Discard, true);
                        }
                        if ui.small_button(tr!("暂存所选行")).clicked() {
                            push(HunkOp::Stage, true);
                        }
                    }
                    if ui.small_button(tr!("放弃区块")).clicked() {
                        push(HunkOp::Discard, false);
                    }
                    if ui.small_button(tr!("暂存区块")).clicked() {
                        push(HunkOp::Stage, false);
                    }
                }
                DiffMode::Staged => {
                    if !selected.is_empty() && ui.small_button(tr!("取消暂存所选行")).clicked()
                    {
                        push(HunkOp::Unstage, true);
                    }
                    if ui.small_button(tr!("取消暂存区块")).clicked() {
                        push(HunkOp::Unstage, false);
                    }
                }
                DiffMode::ReadOnly => {}
            }
        });
    }

    fn line(&mut self, ui: &mut Ui, h: usize, l: usize) {
        let line = &self.diff.hunks[h].lines[l];
        let text_color = ui.visuals().text_color();
        let weak = ui.visuals().weak_text_color();
        let (sign, bg) = match line.kind {
            LineKind::Added => ('+', ADDED_BG),
            LineKind::Removed => ('-', REMOVED_BG),
            LineKind::Context => (' ', Color32::TRANSPARENT),
        };
        let text = displayed(line);
        let style = &self.styles[h][l];
        let gutter_width = self.gutter * 2.0 + 30.0;
        let available = ui.available_width().max(400.0);

        let mut job = LayoutJob::default();
        if self.options.wrap {
            job.wrap.max_width = (available - gutter_width).max(100.0);
        }
        let plain = TextFormat::simple(self.font.clone(), text_color);
        job.append(&format!("{sign} "), 0.0, plain.clone());
        let mut cursor = 0;
        let dark = ui.visuals().dark_mode;
        for span in &style.spans {
            let start = span.start.max(cursor).min(text.len());
            let end = span.end.max(start).min(text.len());
            if start > cursor {
                job.append(&text[cursor..start], 0.0, plain.clone());
            }
            let mut format = plain.clone();
            format.color = syntax::color(span.kind, dark);
            job.append(&text[start..end], 0.0, format);
            cursor = end;
        }
        if cursor < text.len() {
            job.append(&text[cursor..], 0.0, plain);
        }
        let galley = ui.ctx().fonts_mut(|fonts| fonts.layout_job(job));
        let height = if self.options.wrap {
            galley.size().y.max(LINE_HEIGHT)
        } else {
            LINE_HEIGHT
        };
        let width = available.max(gutter_width + galley.size().x + 8.0);
        let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());

        let key = (h, l);
        let selectable = self.options.mode != DiffMode::ReadOnly
            && self.options.hunk_ops
            && line.kind != LineKind::Context;
        let selected = self.selection.lines.contains(&key);
        ui.painter().rect_filled(rect, 0.0, bg);
        if selected {
            ui.painter().rect_filled(rect, 0.0, SELECTED_LINE_BG);
        } else if selectable && response.hovered() {
            ui.painter()
                .rect_filled(rect, 0.0, ui.visuals().widgets.hovered.weak_bg_fill);
        }

        let number = |value: Option<u32>| value.map(|n| n.to_string()).unwrap_or_default();
        let y = rect.top() + (LINE_HEIGHT - 14.0) / 2.0;
        let old_right = rect.left() + 6.0 + self.gutter;
        let new_right = old_right + 8.0 + self.gutter;
        ui.painter().text(
            Pos2::new(old_right, y),
            egui::Align2::RIGHT_TOP,
            number(line.old_no),
            self.font.clone(),
            weak,
        );
        ui.painter().text(
            Pos2::new(new_right, y),
            egui::Align2::RIGHT_TOP,
            number(line.new_no),
            self.font.clone(),
            weak,
        );
        let text_left = rect.left() + gutter_width;
        let text_origin = Pos2::new(text_left, rect.top() + (LINE_HEIGHT - 15.0) / 2.0);
        if let Some((start, end)) = style.word {
            let word_bg = match line.kind {
                LineKind::Added => ADDED_WORD_BG,
                LineKind::Removed => REMOVED_WORD_BG,
                LineKind::Context => Color32::TRANSPARENT,
            };
            // 画廊文本以「+ 」或「- 」开头，着色区间是后面的行内容。
            paint_word(
                ui.painter(),
                &galley,
                text_origin,
                start + 2,
                end + 2,
                word_bg,
            );
        }
        ui.painter().galley(text_origin, galley, text_color);
        let separator_x = text_left - 6.0;
        ui.painter().line_segment(
            [
                Pos2::new(separator_x, rect.top()),
                Pos2::new(separator_x, rect.bottom()),
            ],
            egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        );

        if selectable && response.clicked() {
            let shift = ui.input(|input| input.modifiers.shift);
            if shift
                && let Some((anchor_h, anchor_l)) = self.selection.anchor
                && anchor_h == h
            {
                let (from, to) = (anchor_l.min(l), anchor_l.max(l));
                for index in from..=to {
                    if self.diff.hunks[h].lines[index].kind != LineKind::Context {
                        self.selection.lines.insert((h, index));
                    }
                }
            } else if !self.selection.lines.remove(&key) {
                self.selection.lines.insert(key);
            }
            self.selection.anchor = Some(key);
        }
    }
}

fn displayed(line: &DiffLine) -> String {
    line.text().replace('\t', "    ")
}

fn prepare(diff: &FileDiff, syntax: bool) -> Vec<Vec<LineStyle>> {
    let language = syntax.then(|| syntax::language_for(&diff.path)).flatten();
    diff.hunks
        .iter()
        .map(|hunk| {
            let words = word_ranges(&hunk.lines);
            let mut carry = syntax::Carry::default();
            hunk.lines
                .iter()
                .enumerate()
                .map(|(index, line)| {
                    let text = displayed(line);
                    let spans = language
                        .map(|language| syntax::highlight(&text, language, &mut carry))
                        .unwrap_or_default();
                    LineStyle {
                        spans,
                        word: words[index],
                    }
                })
                .collect()
        })
        .collect()
}

/// 把成对的删除行和新增行里真正不同的那一段标出来。整行都不同时不再叠加，行底色已经说明了。
fn word_ranges(lines: &[DiffLine]) -> Vec<Option<(usize, usize)>> {
    let mut ranges = vec![None; lines.len()];
    let mut removed = Vec::new();
    let mut cursor = 0;
    for (index, line) in lines.iter().enumerate() {
        match line.kind {
            LineKind::Removed => removed.push(index),
            LineKind::Added => {
                if cursor < removed.len() {
                    let old = removed[cursor];
                    cursor += 1;
                    let (before, after) = changed_span(&displayed(&lines[old]), &displayed(line));
                    ranges[old] = before;
                    ranges[index] = after;
                }
            }
            LineKind::Context => {
                removed.clear();
                cursor = 0;
            }
        }
    }
    ranges
}

type ByteRange = Option<(usize, usize)>;

fn changed_span(before: &str, after: &str) -> (ByteRange, ByteRange) {
    let before_chars: Vec<(usize, char)> = before.char_indices().collect();
    let after_chars: Vec<(usize, char)> = after.char_indices().collect();
    let mut prefix = 0;
    while prefix < before_chars.len()
        && prefix < after_chars.len()
        && before_chars[prefix].1 == after_chars[prefix].1
    {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < before_chars.len() - prefix
        && suffix < after_chars.len() - prefix
        && before_chars[before_chars.len() - 1 - suffix].1
            == after_chars[after_chars.len() - 1 - suffix].1
    {
        suffix += 1;
    }
    (
        partial(before, &before_chars, prefix, suffix),
        partial(after, &after_chars, prefix, suffix),
    )
}

fn partial(
    text: &str,
    chars: &[(usize, char)],
    prefix: usize,
    suffix: usize,
) -> Option<(usize, usize)> {
    if prefix + suffix >= chars.len() {
        return None;
    }
    let start = chars[prefix].0;
    let end = if suffix == 0 {
        text.len()
    } else {
        chars[chars.len() - suffix].0
    };
    (start > 0 || end < text.len()).then_some((start, end))
}

fn paint_word(
    painter: &egui::Painter,
    galley: &egui::Galley,
    origin: Pos2,
    start: usize,
    end: usize,
    color: Color32,
) {
    if start >= end || color == Color32::TRANSPARENT {
        return;
    }
    let mut byte = 0;
    for row in &galley.rows {
        let mut run: Option<egui::Rect> = None;
        let flush = |painter: &egui::Painter, run: &mut Option<egui::Rect>| {
            if let Some(rect) = run.take() {
                painter.rect_filled(rect, 0.0, color);
            }
        };
        for glyph in &row.row.glyphs {
            let next = byte + glyph.chr.len_utf8();
            if next > start && byte < end {
                let x = origin.x + row.pos.x + glyph.pos.x;
                let top = origin.y + row.pos.y + glyph.pos.y - glyph.font_ascent;
                let rect = egui::Rect::from_min_max(
                    Pos2::new(x, top),
                    Pos2::new(x + glyph.advance_width, top + glyph.font_height),
                );
                run = Some(match run {
                    Some(previous) if (previous.top() - rect.top()).abs() < 0.5 => {
                        previous.union(rect)
                    }
                    Some(previous) => {
                        painter.rect_filled(previous, 0.0, color);
                        rect
                    }
                    None => rect,
                });
            } else if byte >= end {
                break;
            }
            byte = next;
        }
        flush(painter, &mut run);
        if byte >= end {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(kind: LineKind, text: &str) -> DiffLine {
        DiffLine {
            kind,
            old_no: None,
            new_no: None,
            raw: format!("{text}\n"),
        }
    }

    #[test]
    fn word_highlight_covers_only_the_changed_middle() {
        let lines = vec![
            line(LineKind::Removed, "hello"),
            line(LineKind::Added, "hello!"),
            line(LineKind::Context, "next"),
            line(LineKind::Removed, "abcdef"),
            line(LineKind::Added, "abXYef"),
            line(LineKind::Removed, "foo"),
            line(LineKind::Added, "bar"),
        ];
        let ranges = word_ranges(&lines);
        assert_eq!(ranges[0], None);
        assert_eq!(ranges[1], Some((5, 6)));
        assert_eq!(ranges[3], Some((2, 4)));
        assert_eq!(ranges[4], Some((2, 4)));
        assert_eq!(ranges[5], None);
        assert_eq!(ranges[6], None);
    }
}
