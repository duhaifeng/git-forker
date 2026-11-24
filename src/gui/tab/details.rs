use eframe::egui::{self, Align, Layout, RichText, Ui, pos2, vec2};
use git_forker::tr;
use git_forker::{CommitDetails, Person, Repo};

use super::{Action, DetailsTab, RepoTab, TabData, file_row, split_message};
use crate::gui::diff_view::{self, DiffMode, DiffViewOptions};
use crate::gui::icons::{self, Icon};
use crate::gui::widgets;

impl RepoTab {
    pub(super) fn details_pane(&mut self, ui: &mut Ui, data: &TabData) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let before = self.details_tab;
            widgets::segmented(
                ui,
                &mut self.details_tab,
                &[
                    (DetailsTab::Commit, tr!("提交")),
                    (DetailsTab::Changes, tr!("更改")),
                    (DetailsTab::Tree, tr!("文件树")),
                ],
            );
            if before != self.details_tab
                && self.details_tab == DetailsTab::Tree
                && let Ok(repo) = Repo::discover(&self.root)
            {
                self.load_tree(&repo);
            }
        });
        ui.add_space(2.0);
        ui.separator();
        let details = match &self.details {
            None => {
                ui.centered_and_justified(|ui| ui.weak(tr!("选择一个提交查看详情")));
                return;
            }
            Some(Err(err)) => {
                ui.colored_label(ui.visuals().error_fg_color, err.clone());
                return;
            }
            Some(Ok(details)) => details.clone(),
        };
        match self.details_tab {
            DetailsTab::Commit => self.commit_summary(ui, data, &details),
            DetailsTab::Changes => self.commit_changes(ui, &details),
            DetailsTab::Tree => self.tree_view(ui),
        }
    }

    fn commit_summary(&mut self, ui: &mut Ui, data: &TabData, details: &CommitDetails) {
        egui::ScrollArea::vertical()
            .id_salt("commit-summary")
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    person_card(ui, tr!("作者"), &details.author);
                    let same = details.committer.name == details.author.name
                        && details.committer.email == details.author.email;
                    if !same || details.committer.time != details.author.time {
                        ui.add_space(40.0);
                        person_card(ui, tr!("提交者"), &details.committer);
                    }
                });
                ui.add_space(10.0);
                egui::Grid::new("commit-facts")
                    .num_columns(2)
                    .spacing([12.0, 6.0])
                    .show(ui, |ui| {
                        if let Some(labels) = data.history.labels.get(&details.id) {
                            fact_label(ui, tr!("引用"));
                            ui.horizontal(|ui| {
                                for label in labels {
                                    egui::Frame::new()
                                        .fill(ui.visuals().widgets.inactive.weak_bg_fill)
                                        .corner_radius(4)
                                        .inner_margin(egui::Margin::symmetric(5, 1))
                                        .show(ui, |ui| {
                                            ui.label(RichText::new(&label.name).size(12.0))
                                        });
                                }
                            });
                            ui.end_row();
                        }
                        fact_label(ui, "SHA");
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(details.id.to_string()).monospace().size(12.0));
                            if icons::icon_button(ui, Icon::Copy, tr!("复制 SHA")).clicked() {
                                ui.ctx().copy_text(details.id.to_string());
                            }
                        });
                        ui.end_row();
                        fact_label(ui, tr!("父提交"));
                        ui.horizontal(|ui| {
                            for parent in &details.parents {
                                if ui
                                    .link(
                                        RichText::new(git_forker::short_id(parent))
                                            .monospace()
                                            .size(12.0),
                                    )
                                    .clicked()
                                {
                                    self.actions.push(Action::RevealCommit(*parent));
                                }
                            }
                            if details.parents.is_empty() {
                                ui.weak(tr!("无（根提交）"));
                            }
                        });
                        ui.end_row();
                    });
                ui.add_space(12.0);
                let (subject, body) = split_message(&details.message);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.add(egui::Label::new(RichText::new(subject).size(15.0).strong()).wrap());
                });
                if !body.is_empty() {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        ui.add(egui::Label::new(RichText::new(body).monospace().size(12.5)).wrap());
                    });
                }
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.label(
                        RichText::new(tr!("{count} 个文件已更改", count = details.files.len()))
                            .size(12.0)
                            .color(ui.visuals().weak_text_color()),
                    );
                });
                for file in &details.files {
                    let selected = self.details_file.as_deref() == Some(file.path.as_str());
                    let response = ui
                        .horizontal(|ui| {
                            ui.add_space(10.0);
                            file_row(ui, file, selected)
                        })
                        .inner;
                    if response.clicked() {
                        self.actions
                            .push(Action::SelectDetailsFile(file.path.clone()));
                        self.details_tab = DetailsTab::Changes;
                    }
                    response.context_menu(|ui| self.file_menu(ui, &file.path, true));
                }
                ui.add_space(12.0);
            });
    }

    fn commit_changes(&mut self, ui: &mut Ui, details: &CommitDetails) {
        egui::Panel::left("details-files")
            .resizable(true)
            .default_size(300.0)
            .min_size(200.0)
            .show(ui, |ui| {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(tr!("{count} 个文件", count = details.files.len()))
                        .size(12.0)
                        .color(ui.visuals().weak_text_color()),
                );
                egui::ScrollArea::vertical()
                    .id_salt("details-files")
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        for file in &details.files {
                            let selected = self.details_file.as_deref() == Some(file.path.as_str());
                            let response = file_row(ui, file, selected);
                            if response.clicked() {
                                self.actions
                                    .push(Action::SelectDetailsFile(file.path.clone()));
                            }
                            response.context_menu(|ui| self.file_menu(ui, &file.path, true));
                        }
                    });
            });
        egui::CentralPanel::default().show(ui, |ui| match self.details_diff.as_ref() {
            Some(Ok(diff)) => {
                let diff = diff.clone();
                self.diff_header(ui, &diff.path, None);
                diff_view::show(
                    ui,
                    &diff,
                    &DiffViewOptions {
                        mode: DiffMode::ReadOnly,
                        hunk_ops: false,
                        wrap: self.wrap,
                        syntax: self.syntax,
                    },
                    &mut self.diff_selection,
                );
            }
            Some(Err(err)) => {
                ui.colored_label(ui.visuals().error_fg_color, err);
            }
            None => {
                ui.centered_and_justified(|ui| ui.weak(tr!("没有选中的文件")));
            }
        });
    }
}

fn fact_label(ui: &mut Ui, text: &str) {
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.set_min_width(90.0);
        ui.label(
            RichText::new(text)
                .size(11.0)
                .color(ui.visuals().weak_text_color()),
        );
    });
}

/// 头像、角色、姓名、邮箱和时间组成的卡片。
fn person_card(ui: &mut Ui, role: &str, person: &Person) {
    let (rect, _) = ui.allocate_exact_size(vec2(40.0, 40.0), egui::Sense::hover());
    widgets::paint_avatar(
        ui.painter(),
        pos2(rect.center().x, rect.center().y),
        19.0,
        &person.name,
        &person.email,
    );
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.label(
            RichText::new(role)
                .size(10.5)
                .color(ui.visuals().weak_text_color()),
        );
        ui.horizontal(|ui| {
            ui.label(RichText::new(&person.name).strong());
            ui.label(
                RichText::new(&person.email)
                    .size(12.0)
                    .color(ui.visuals().weak_text_color()),
            );
        });
        ui.label(
            RichText::new(widgets::format_time(person.time))
                .size(12.0)
                .color(ui.visuals().weak_text_color()),
        );
    });
}
