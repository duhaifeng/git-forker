use eframe::egui::{self, Color32, FontId, Key, Rect, Sense, Ui, pos2, vec2};
use git_forker::tr;

use super::{Action, RepoTab, SearchKind, TabData};
use crate::gui::theme;
use crate::gui::widgets::{self, ROW_HEIGHT};

impl RepoTab {
    pub(super) fn history_view(&mut self, ui: &mut Ui, data: &TabData) {
        if self.show_details {
            egui::Panel::bottom("details-pane")
                .resizable(true)
                .default_size(ui.available_height() * 0.48)
                .min_size(140.0)
                .frame(egui::Frame::new().fill(ui.visuals().panel_fill))
                .show(ui, |ui| self.details_pane(ui, data));
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(ui.visuals().panel_fill))
            .show(ui, |ui| self.history_list(ui, data));
    }

    /// 搜索框里的条件匹配到的提交下标。
    pub(super) fn search_matches(&self, data: &TabData) -> Vec<usize> {
        let query = self.search.trim().to_lowercase();
        data.history
            .commits
            .iter()
            .enumerate()
            .filter(|(_, commit)| match self.search_kind {
                SearchKind::Message => commit.summary.to_lowercase().contains(&query),
                SearchKind::Author => {
                    commit.author.to_lowercase().contains(&query)
                        || commit.email.to_lowercase().contains(&query)
                }
                SearchKind::Sha => commit.id.to_string().starts_with(&query),
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn history_list(&mut self, ui: &mut Ui, data: &TabData) {
        let history = &data.history;
        let searching = !self.search.trim().is_empty();
        let filtered: Option<Vec<usize>> = searching.then(|| self.search_matches(data));
        let count = filtered.as_ref().map_or(history.commits.len(), Vec::len);
        let index_at = |row: usize| filtered.as_ref().map_or(row, |rows| rows[row]);

        if count == 0 {
            ui.centered_and_justified(|ui| {
                ui.weak(if searching {
                    tr!("没有匹配的提交")
                } else {
                    tr!("尚无提交")
                })
            });
            return;
        }

        let typing = ui.ctx().memory(|memory| memory.focused().is_some());
        if !typing && !self.has_modal() {
            let current = self
                .selected_commit
                .and_then(|id| (0..count).find(|row| history.commits[index_at(*row)].id == id));
            let step = ui.input_mut(|input| {
                if input.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                    1i64
                } else if input.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                    -1
                } else {
                    0
                }
            });
            if step != 0 {
                let next = current
                    .map(|row| (row as i64 + step).clamp(0, count as i64 - 1) as usize)
                    .unwrap_or(0);
                self.actions
                    .push(Action::SelectCommit(history.commits[index_at(next)].id));
                let (offset, height) = self.history_viewport;
                let top = next as f32 * ROW_HEIGHT;
                if top < offset || top + ROW_HEIGHT > offset + height {
                    self.scroll_to = Some(index_at(next));
                }
            }
        }

        let graph_width = if searching { 8.0 } else { data.graph_width };
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt("history")
            .auto_shrink(false);
        if let Some(target) = self.scroll_to.take() {
            let row = filtered.as_ref().map_or(Some(target), |rows| {
                rows.iter().position(|index| *index == target)
            });
            if let Some(row) = row {
                let height = self.history_viewport.1.max(ROW_HEIGHT);
                scroll = scroll
                    .vertical_scroll_offset((row as f32 * ROW_HEIGHT - height / 2.0).max(0.0));
            }
        }
        let output = scroll.show_rows(ui, ROW_HEIGHT, count, |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for row in range {
                self.history_row(ui, data, index_at(row), graph_width, searching);
            }
        });
        self.history_viewport = (output.state.clamped_offset().y, output.inner_rect.height());
    }

    fn history_row(
        &mut self,
        ui: &mut Ui,
        data: &TabData,
        index: usize,
        graph_width: f32,
        searching: bool,
    ) {
        let commit = &data.history.commits[index];
        let graph = &data.history.graph[index];
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), Sense::click());
        let dark = ui.visuals().dark_mode;
        let selected = self.selected_commit == Some(commit.id);
        if selected {
            ui.painter()
                .rect_filled(rect, 0.0, theme::history_selection(dark));
        } else if response.hovered() {
            ui.painter().rect_filled(
                rect,
                0.0,
                ui.visuals()
                    .widgets
                    .hovered
                    .weak_bg_fill
                    .gamma_multiply(0.5),
            );
        }
        let text_color = if selected && !dark {
            Color32::BLACK
        } else if selected {
            Color32::WHITE
        } else {
            ui.visuals().text_color()
        };
        let weak = if selected {
            text_color.gamma_multiply(0.8)
        } else {
            ui.visuals().weak_text_color()
        };

        if !searching {
            widgets::paint_graph_row(
                ui,
                Rect::from_min_size(rect.min + vec2(4.0, 0.0), vec2(graph_width, ROW_HEIGHT)),
                graph,
                commit.parents.len() > 1,
                data.head == Some(commit.id),
            );
        }

        let date_w = 120.0;
        let sha_w = 64.0;
        let author_w = (width * 0.17).clamp(110.0, 200.0);
        let right = rect.right() - 10.0;
        let date_rect =
            Rect::from_min_max(pos2(right - date_w, rect.top()), pos2(right, rect.bottom()));
        let sha_rect = Rect::from_min_max(
            pos2(date_rect.left() - sha_w - 10.0, rect.top()),
            pos2(date_rect.left() - 10.0, rect.bottom()),
        );
        let author_rect = Rect::from_min_max(
            pos2(sha_rect.left() - author_w - 10.0, rect.top()),
            pos2(sha_rect.left() - 10.0, rect.bottom()),
        );
        let summary_rect = Rect::from_min_max(
            pos2(rect.left() + graph_width + 10.0, rect.top()),
            pos2(author_rect.left() - 12.0, rect.bottom()),
        );

        let mut left = summary_rect.left();
        if let Some(labels) = data.history.labels.get(&commit.id) {
            left = widgets::paint_ref_labels(
                ui,
                summary_rect,
                left,
                labels,
                widgets::lane_color(graph.color),
            );
        }
        let summary_clip = Rect::from_min_max(pos2(left, rect.top()), summary_rect.max);
        paint_summary(
            ui,
            summary_clip,
            &commit.summary,
            text_color,
            commit.parents.len() > 1,
        );

        widgets::paint_avatar(
            ui.painter(),
            pos2(author_rect.left() + 8.0, rect.center().y),
            8.0,
            &commit.author,
            &commit.email,
        );
        let author_text =
            Rect::from_min_max(pos2(author_rect.left() + 22.0, rect.top()), author_rect.max);
        let font = FontId::proportional(13.0);
        widgets::paint_clipped_text(ui, author_text, &commit.author, font.clone(), weak, false);
        widgets::paint_clipped_text(
            ui,
            sha_rect,
            &commit.short[..commit.short.len().min(7)],
            FontId::monospace(12.0),
            weak,
            false,
        );
        widgets::paint_clipped_text(
            ui,
            date_rect,
            &widgets::relative_time(commit.time),
            font,
            weak,
            false,
        );

        if response.clicked() {
            self.actions.push(Action::SelectCommit(commit.id));
        }
        if response.double_clicked() {
            let branch = data.history.labels.get(&commit.id).and_then(|labels| {
                labels
                    .iter()
                    .find(|label| label.kind == git_forker::RefKind::LocalBranch)
            });
            if let Some(branch) = branch {
                self.checkout_branch(&branch.name.clone());
            }
        }
        response.context_menu(|ui| {
            self.actions.push(Action::SelectCommit(commit.id));
            self.commit_menu(ui, data, index);
        });
    }
}

/// 约定式提交的类型前缀（例如 `fix(ci):`）加粗显示；合并提交末尾加 `↩`。
fn paint_summary(ui: &Ui, clip: Rect, summary: &str, color: Color32, merge: bool) {
    let font = FontId::proportional(13.0);
    let painter = ui.painter().with_clip_rect(clip);
    let prefix_end = summary
        .find(": ")
        .filter(|end| *end > 0 && *end <= 32 && !summary[..*end].contains(' '));
    let mut x = clip.left();
    let mut paint = |text: &str, color: Color32, bold: bool| {
        let galley = painter.layout_no_wrap(text.to_string(), font.clone(), color);
        let pos = pos2(x, clip.center().y - galley.size().y / 2.0);
        x += galley.size().x;
        if bold {
            // 系统字体没有单独的粗体文件时，用错位重绘模拟粗体。
            painter.galley(pos + vec2(0.6, 0.0), galley.clone(), color);
            x += 0.6;
        }
        painter.galley(pos, galley, color);
    };
    match prefix_end {
        Some(end) => {
            paint(&summary[..=end], color, true);
            paint(&summary[end + 1..], color, false);
        }
        None => paint(summary, color, false),
    }
    if merge {
        paint(" ↩", color.gamma_multiply(0.6), false);
    }
}
