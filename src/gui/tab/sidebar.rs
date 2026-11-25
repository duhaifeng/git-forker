use eframe::egui::{self, Color32, FontId, RichText, Sense, Ui, Vec2, pos2, vec2};
use git_forker::tr;
use git_forker::{GitJob, LocalBranch, RemoteInfo, StashInfo, SubmoduleInfo};

use super::{Action, RepoTab, SidebarMode, TabData, TabRequest, View};
use crate::gui::dialogs::{Dialog, LocalChanges, MergeMode};
use crate::gui::icons::{self, Icon};
use crate::gui::theme;

const ROW_HEIGHT: f32 = 24.0;
const INDENT: f32 = 14.0;

/// 按 `/` 分组后的一行。输入需要已排序。
pub(super) enum Node<'a> {
    Folder {
        path: String,
        name: &'a str,
        depth: usize,
    },
    Leaf {
        index: usize,
        name: &'a str,
        depth: usize,
    },
}

/// 把 `feature/a`、`feature/b` 这样的名字整理成文件夹树。输入需要已排序。
pub(super) fn tree<'a>(names: &[&'a str]) -> Vec<Node<'a>> {
    let mut nodes = Vec::new();
    let mut open: Vec<&str> = Vec::new();
    for (index, name) in names.iter().enumerate() {
        let parts: Vec<&str> = name.split('/').collect();
        let folders = &parts[..parts.len() - 1];
        let common = open.iter().zip(folders).take_while(|(a, b)| a == b).count();
        open.truncate(common);
        for (depth, folder) in folders.iter().enumerate().skip(common) {
            open.push(folder);
            nodes.push(Node::Folder {
                path: folders[..=depth].join("/"),
                name: folder,
                depth,
            });
        }
        nodes.push(Node::Leaf {
            index,
            name: parts[parts.len() - 1],
            depth: folders.len(),
        });
    }
    nodes
}

struct Row<'a> {
    depth: usize,
    icon: Option<Icon>,
    icon_color: Option<Color32>,
    text: &'a str,
    strong: bool,
    selected: bool,
    trailing: Option<String>,
    chevron: Option<bool>,
}

impl<'a> Row<'a> {
    fn new(depth: usize, text: &'a str) -> Self {
        Self {
            depth,
            icon: None,
            icon_color: None,
            text,
            strong: false,
            selected: false,
            trailing: None,
            chevron: None,
        }
    }

    fn show(self, ui: &mut Ui) -> egui::Response {
        let width = ui.available_width();
        let (rect, response) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), Sense::click());
        let dark = ui.visuals().dark_mode;
        if self.selected {
            ui.painter().rect_filled(
                rect.shrink2(vec2(4.0, 1.0)),
                6.0,
                theme::row_selection(dark),
            );
        } else if response.hovered() {
            ui.painter().rect_filled(
                rect.shrink2(vec2(4.0, 1.0)),
                6.0,
                ui.visuals()
                    .widgets
                    .hovered
                    .weak_bg_fill
                    .gamma_multiply(0.6),
            );
        }
        let text_color = ui.visuals().text_color();
        let weak = ui.visuals().weak_text_color();
        let mut x = rect.left() + 10.0 + self.depth as f32 * INDENT;
        let center_y = rect.center().y;
        if let Some(open) = self.chevron {
            let chevron = if open {
                Icon::ChevronDown
            } else {
                Icon::ChevronRight
            };
            icons::paint(
                ui.painter(),
                pos2(x + icons::SMALL / 2.0, center_y),
                icons::SMALL,
                chevron,
                weak,
            );
            x += 14.0;
        }
        if let Some(icon) = self.icon {
            icons::paint(
                ui.painter(),
                pos2(x + icons::INLINE / 2.0, center_y),
                icons::INLINE,
                icon,
                self.icon_color.unwrap_or(weak),
            );
            x += 22.0;
        }
        let font = FontId::proportional(13.5);
        let painter = ui.painter().with_clip_rect(rect);
        let mut right = rect.right() - 12.0;
        if let Some(trailing) = &self.trailing {
            let galley = painter.layout_no_wrap(trailing.clone(), FontId::proportional(12.0), weak);
            right -= galley.size().x;
            painter.galley(pos2(right, center_y - galley.size().y / 2.0), galley, weak);
            right -= 8.0;
        }
        let text = if self.strong {
            RichText::new(self.text).strong().size(13.5)
        } else {
            RichText::new(self.text).size(13.5)
        };
        let galley = egui::WidgetText::from(text).into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            right - x,
            font,
        );
        let pos = pos2(x, center_y - galley.size().y / 2.0);
        if self.strong {
            painter.galley(pos + vec2(0.5, 0.0), galley.clone(), text_color);
        }
        painter.galley(pos, galley, text_color);
        response
    }
}

impl RepoTab {
    pub fn sidebar(&mut self, ui: &mut Ui) {
        let Some(data) = self.data.take() else {
            if self.is_loading() {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(RichText::new(&self.name).strong().size(15.0));
                });
                return;
            }
            ui.add_space(12.0);
            ui.colored_label(
                ui.visuals().error_fg_color,
                self.error.as_deref().unwrap_or(tr!("无法读取仓库")),
            );
            if ui.button(tr!("重试")).clicked() {
                self.reload();
            }
            return;
        };
        self.sidebar_with(ui, &data);
        self.data = Some(data);
    }

    fn sidebar_with(&mut self, ui: &mut Ui, data: &TabData) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(RichText::new(&self.name).strong().size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                let response = icons::icon_button(ui, Icon::More, tr!("仓库选项"));
                egui::Popup::menu(&response).show(|ui| self.repository_menu(ui));
            });
        });
        ui.add_space(2.0);
        let changes =
            data.tree.unstaged.len() + data.tree.staged.len() + data.tree.conflicted.len();
        let mut row = Row::new(0, tr!("本地更改"));
        row.icon = Some(Icon::LocalChanges);
        row.selected = self.view == View::LocalChanges;
        row.trailing = (changes > 0).then(|| changes.to_string());
        if row.show(ui).clicked() {
            self.view = View::LocalChanges;
        }
        let mut row = Row::new(0, tr!("全部提交"));
        row.icon = Some(Icon::AllCommits);
        row.selected = self.view == View::History;
        if row.show(ui).clicked() {
            self.view = View::History;
        }
        ui.add_space(6.0);
        ui.separator();

        ui.horizontal(|ui| {
            let spacing = (ui.available_width() - 2.0 * 30.0 - 8.0).max(0.0) / 2.0;
            ui.add_space(spacing);
            let accent = ui.visuals().hyperlink_color;
            for (mode, icon, tip) in [
                (SidebarMode::Branches, Icon::Branch, tr!("分支和引用")),
                (SidebarMode::Search, Icon::Search, tr!("搜索提交")),
            ] {
                let (rect, response) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
                let color = if self.sidebar_mode == mode {
                    accent
                } else {
                    ui.visuals().weak_text_color()
                };
                icons::paint(ui.painter(), rect.center(), icons::INLINE, icon, color);
                if response.on_hover_text(tip).clicked() {
                    self.sidebar_mode = mode;
                    if mode == SidebarMode::Search {
                        self.view = View::History;
                        self.focus_search = true;
                    }
                }
            }
        });

        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let width = ui.available_width() - 8.0;
            match self.sidebar_mode {
                SidebarMode::Branches => {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.sidebar_filter)
                            .hint_text(Icon::Search.with(tr!("过滤")))
                            .desired_width(width),
                    );
                }
                SidebarMode::Search => {
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.search)
                            .hint_text(Icon::Search.with(tr!("搜索说明、作者或 SHA")))
                            .desired_width(width),
                    );
                    if std::mem::take(&mut self.focus_search) {
                        response.request_focus();
                    }
                    if response.changed() {
                        self.view = View::History;
                    }
                }
            }
        });
        ui.add_space(4.0);

        match self.sidebar_mode {
            SidebarMode::Branches => self.reference_tree(ui, data),
            SidebarMode::Search => self.search_panel(ui, data),
        }
    }

    fn search_panel(&mut self, ui: &mut Ui, data: &TabData) {
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.weak(tr!("搜索范围"));
            for (kind, label) in [
                (super::SearchKind::Message, tr!("说明")),
                (super::SearchKind::Author, tr!("作者")),
                (super::SearchKind::Sha, "SHA"),
            ] {
                ui.selectable_value(&mut self.search_kind, kind, label);
            }
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            if self.search.trim().is_empty() {
                ui.weak(tr!("在已加载的提交中搜索"));
            } else {
                let count = self.search_matches(data).len();
                ui.weak(tr!("{count} 个匹配的提交", count = count));
            }
        });
    }

    fn section(&mut self, ui: &mut Ui, id: &str, title: &str) -> bool {
        let open = !self.collapsed.contains(id);
        let mut row = Row::new(0, title);
        row.strong = true;
        row.chevron = Some(open);
        if row.show(ui).clicked() {
            if open {
                self.collapsed.insert(id.to_string());
            } else {
                self.collapsed.remove(id);
            }
        }
        open
    }

    /// 画出按 `/` 分组的名称。返回被点击、双击和需要右键菜单的叶子。
    fn folder_tree(
        &mut self,
        ui: &mut Ui,
        id: &str,
        base_depth: usize,
        names: &[&str],
        mut leaf: impl FnMut(&mut Self, &mut Ui, usize, usize, &str),
    ) {
        let forced_open = !self.sidebar_filter.is_empty();
        let mut hidden_below: Option<usize> = None;
        for node in tree(names) {
            let depth = match &node {
                Node::Folder { depth, .. } | Node::Leaf { depth, .. } => *depth,
            };
            if let Some(limit) = hidden_below {
                if depth > limit {
                    continue;
                }
                hidden_below = None;
            }
            match node {
                Node::Folder { path, name, depth } => {
                    let key = format!("{id}/{path}");
                    let open = forced_open || !self.collapsed.contains(&key);
                    let mut row = Row::new(base_depth + depth, name);
                    row.chevron = Some(open);
                    row.icon = Some(Icon::Folder);
                    if row.show(ui).clicked() {
                        if open {
                            self.collapsed.insert(key);
                        } else {
                            self.collapsed.remove(&key);
                        }
                    }
                    if !open {
                        hidden_below = Some(depth);
                    }
                }
                Node::Leaf { index, name, depth } => {
                    leaf(self, ui, index, base_depth + depth, name);
                }
            }
        }
    }

    fn reference_tree(&mut self, ui: &mut Ui, data: &TabData) {
        let filter = self.sidebar_filter.to_lowercase();
        let matches = |name: &str| filter.is_empty() || name.to_lowercase().contains(&filter);
        egui::ScrollArea::vertical()
            .id_salt("sidebar")
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if self.section(ui, "branches", tr!("分支")) {
                    let branches: Vec<&LocalBranch> = data
                        .refs
                        .local
                        .iter()
                        .filter(|b| matches(&b.name))
                        .collect();
                    let names: Vec<&str> = branches.iter().map(|b| b.name.as_str()).collect();
                    self.folder_tree(ui, "branches", 1, &names, |tab, ui, index, depth, leaf| {
                        tab.branch_row(ui, data, branches[index], depth, leaf);
                    });
                }
                if self.section(ui, "remotes", tr!("远程仓库")) {
                    for remote in &data.refs.remotes {
                        self.remote_rows(ui, data, remote, &matches);
                    }
                    let mut row = Row::new(1, tr!("添加远程仓库…"));
                    row.icon = Some(Icon::Plus);
                    if row.show(ui).clicked() {
                        self.add_remote();
                    }
                }
                if self.section(ui, "tags", tr!("标签")) {
                    for tag in data.refs.tags.iter().filter(|t| matches(&t.name)) {
                        let mut row = Row::new(1, &tag.name);
                        row.icon = Some(Icon::Tag);
                        row.selected = tag.target.is_some() && self.selected_commit == tag.target;
                        let response = row.show(ui);
                        if response.clicked()
                            && let Some(target) = tag.target
                        {
                            self.actions.push(Action::RevealCommit(target));
                        }
                        response.context_menu(|ui| self.tag_menu(ui, data, &tag.name));
                    }
                }
                if self.section(ui, "stashes", tr!("贮藏")) {
                    for stash in &data.refs.stashes {
                        self.stash_row(ui, stash);
                    }
                }
                if self.section(ui, "submodules", tr!("子模块")) {
                    for module in &data.refs.submodules {
                        self.submodule_row(ui, module);
                    }
                }
                ui.add_space(12.0);
            });
    }

    fn branch_row(
        &mut self,
        ui: &mut Ui,
        data: &TabData,
        branch: &LocalBranch,
        depth: usize,
        leaf: &str,
    ) {
        let mut row = Row::new(depth, leaf);
        if branch.is_head {
            row.icon = Some(Icon::Check);
            row.icon_color = Some(ui.visuals().text_color());
            row.strong = true;
        } else {
            row.icon = Some(Icon::Branch);
        }
        row.selected = self.selected_commit == Some(branch.target);
        let mut counts = String::new();
        if branch.ahead > 0 {
            counts.push_str(&format!("{}↑", branch.ahead));
        }
        if branch.behind > 0 {
            if !counts.is_empty() {
                counts.push(' ');
            }
            counts.push_str(&format!("{}↓", branch.behind));
        }
        row.trailing = (!counts.is_empty()).then_some(counts);
        let response = row.show(ui);
        let response = match &branch.upstream {
            Some(upstream) => response.on_hover_text(tr!(
                "{branch}\n跟踪 {upstream}",
                branch = branch.name,
                upstream = upstream
            )),
            None => response.on_hover_text(&branch.name),
        };
        if response.clicked() {
            self.actions.push(Action::RevealCommit(branch.target));
        }
        if response.double_clicked() && !branch.is_head {
            self.checkout_branch(&branch.name);
        }
        response.context_menu(|ui| self.branch_menu(ui, data, branch));
    }

    fn remote_rows(
        &mut self,
        ui: &mut Ui,
        data: &TabData,
        remote: &RemoteInfo,
        matches: &dyn Fn(&str) -> bool,
    ) {
        let key = format!("remote:{}", remote.name);
        let open = !self.sidebar_filter.is_empty() || self.expanded.contains(&key);
        let mut row = Row::new(1, &remote.name);
        row.chevron = Some(open);
        row.icon = Some(Icon::Remote);
        let response = row.show(ui).on_hover_text(&remote.url);
        if response.clicked() {
            if open {
                self.expanded.remove(&key);
            } else {
                self.expanded.insert(key.clone());
            }
        }
        response.context_menu(|ui| self.remote_menu(ui, remote));
        if !open {
            return;
        }
        let branches: Vec<_> = remote
            .branches
            .iter()
            .filter(|b| matches(&b.name))
            .collect();
        let names: Vec<&str> = branches.iter().map(|b| b.branch.as_str()).collect();
        self.folder_tree(ui, &key, 2, &names, |tab, ui, index, depth, leaf| {
            let branch = branches[index];
            let mut row = Row::new(depth, leaf);
            row.icon = Some(Icon::Branch);
            row.selected = tab.selected_commit == Some(branch.target);
            let response = row.show(ui).on_hover_text(&branch.name);
            if response.clicked() {
                tab.actions.push(Action::RevealCommit(branch.target));
            }
            if response.double_clicked() {
                tab.checkout_remote(data, &branch.name);
            }
            response
                .context_menu(|ui| tab.remote_branch_menu(ui, data, &branch.name, &branch.branch));
        });
    }

    fn stash_row(&mut self, ui: &mut Ui, stash: &StashInfo) {
        let message = stash
            .message
            .split_once(": ")
            .map(|(_, rest)| rest)
            .unwrap_or(&stash.message);
        let mut row = Row::new(1, message);
        row.icon = Some(Icon::Stash);
        row.selected = self.selected_commit == Some(stash.id);
        let response = row
            .show(ui)
            .on_hover_text(format!("stash@{{{}}}  {}", stash.index, stash.message));
        if response.clicked() {
            self.view = View::History;
            self.actions.push(Action::SelectCommit(stash.id));
        }
        response.context_menu(|ui| {
            if ui.button(tr!("应用…")).clicked() {
                self.dialog = Some(Dialog::ApplyStash {
                    index: stash.index,
                    message: stash.message.clone(),
                    delete_after: false,
                });
                ui.close();
            }
            if ui.button(tr!("弹出（应用后删除）")).clicked() {
                self.dialog = Some(Dialog::ApplyStash {
                    index: stash.index,
                    message: stash.message.clone(),
                    delete_after: true,
                });
                ui.close();
            }
            ui.separator();
            if ui.button(tr!("删除…")).clicked() {
                self.dialog = Some(Dialog::DropStash {
                    index: stash.index,
                    message: stash.message.clone(),
                });
                ui.close();
            }
        });
    }

    fn submodule_row(&mut self, ui: &mut Ui, module: &SubmoduleInfo) {
        let mut row = Row::new(1, &module.name);
        row.icon = Some(Icon::Submodule);
        let response = row.show(ui).on_hover_text(tr!("双击在新标签页中打开"));
        let path = self.root.join(&module.path);
        if response.double_clicked() {
            self.requests.push(TabRequest::OpenRepository(path.clone()));
        }
        response.context_menu(|ui| {
            if ui.button(tr!("打开")).clicked() {
                self.requests.push(TabRequest::OpenRepository(path.clone()));
                ui.close();
            }
            if ui.button(tr!("更新")).clicked() {
                self.push_job(
                    GitJob::new(tr!("更新子模块 {module}", module = module.name), &self.root).step(
                        [
                            "submodule",
                            "update",
                            "--init",
                            "--recursive",
                            "--",
                            module.path.as_str(),
                        ],
                    ),
                );
                ui.close();
            }
        });
    }

    fn repository_menu(&mut self, ui: &mut Ui) {
        if ui.button(tr!("刷新")).clicked() {
            self.request_reload();
            ui.close();
        }
        if ui.button(tr!("添加远程仓库…")).clicked() {
            self.add_remote();
            ui.close();
        }
        ui.separator();
        let mut options = self.history_options;
        ui.checkbox(&mut options.current_branch_only, tr!("只显示当前分支"));
        ui.checkbox(&mut options.first_parent, tr!("只沿第一父提交"));
        ui.checkbox(&mut options.show_remotes, tr!("显示远程分支"));
        ui.checkbox(&mut options.show_tags, tr!("显示标签"));
        if options != self.history_options {
            self.history_options = options;
            self.request_reload();
        }
        ui.checkbox(&mut self.show_details, tr!("显示详情面板"));
        ui.separator();
        if ui.button(tr!("在终端中打开")).clicked() {
            self.open_terminal();
            ui.close();
        }
        if ui.button(tr!("在文件管理器中显示")).clicked() {
            self.reveal_in_file_manager();
            ui.close();
        }
        if ui.button(tr!("复制仓库路径")).clicked() {
            ui.ctx().copy_text(self.root.display().to_string());
            ui.close();
        }
    }

    fn remote_menu(&mut self, ui: &mut Ui, remote: &RemoteInfo) {
        if ui.button(tr!("抓取")).clicked() {
            self.push_job(
                GitJob::new(tr!("抓取 {remote}", remote = remote.name), &self.root).step([
                    "fetch",
                    "--progress",
                    remote.name.as_str(),
                ]),
            );
            ui.close();
        }
        if ui.button(tr!("编辑…")).clicked() {
            self.dialog = Some(Dialog::EditRemote {
                original: Some(remote.name.clone()),
                name: remote.name.clone(),
                url: remote.url.clone(),
            });
            ui.close();
        }
        if ui.button(tr!("复制地址")).clicked() {
            ui.ctx().copy_text(remote.url.clone());
            ui.close();
        }
        ui.separator();
        if ui.button(tr!("删除…")).clicked() {
            self.dialog = Some(Dialog::DeleteRemote {
                name: remote.name.clone(),
            });
            ui.close();
        }
    }

    fn branch_menu(&mut self, ui: &mut Ui, data: &TabData, branch: &LocalBranch) {
        let current = data.refs.current_branch().map(|b| b.name.clone());
        if !branch.is_head
            && ui
                .button(tr!("检出 {branch}", branch = branch.name))
                .clicked()
        {
            self.checkout_branch(&branch.name);
            ui.close();
        }
        if let Some(current) = &current
            && !branch.is_head
        {
            if ui
                .button(tr!("合并到 {current}…", current = current))
                .clicked()
            {
                self.dialog = Some(Dialog::Merge {
                    branch: branch.name.clone(),
                    current: current.clone(),
                    mode: MergeMode::Default,
                });
                ui.close();
            }
            if ui
                .button(tr!("把 {current} 变基到此分支…", current = current))
                .clicked()
            {
                self.dialog = Some(Dialog::Rebase {
                    onto: branch.name.clone(),
                    current: current.clone(),
                    autostash: true,
                    update_refs: false,
                });
                ui.close();
            }
        }
        ui.separator();
        if branch.is_head {
            if ui.button(tr!("推送…")).clicked() {
                self.open_push();
                ui.close();
            }
            if ui.button(tr!("拉取…")).clicked() {
                self.open_pull();
                ui.close();
            }
        } else if let Some(upstream) = &branch.upstream
            && branch.behind > 0
            && branch.ahead == 0
            && ui
                .button(tr!("快进到 {upstream}", upstream = upstream))
                .clicked()
        {
            self.push_job(
                GitJob::new(tr!("快进 {branch}", branch = branch.name), &self.root).step(vec![
                    "fetch".to_string(),
                    ".".into(),
                    format!("{upstream}:{}", branch.name),
                ]),
            );
            ui.close();
        }
        if ui.button(tr!("从此创建分支…")).clicked() {
            self.dialog = Some(Dialog::CreateBranch {
                start: branch.name.clone(),
                start_label: branch.name.clone(),
                name: String::new(),
                checkout: true,
                changes: LocalChanges::Keep,
            });
            ui.close();
        }
        if ui.button(tr!("重命名…")).clicked() {
            self.dialog = Some(Dialog::RenameBranch {
                old: branch.name.clone(),
                new: branch.name.clone(),
                upstream: branch.upstream.clone(),
                rename_remote: false,
            });
            ui.close();
        }
        if branch.upstream.is_some() && ui.button(tr!("取消跟踪上游")).clicked() {
            self.push_job(GitJob::new(tr!("取消跟踪上游"), &self.root).step([
                "branch",
                "--unset-upstream",
                branch.name.as_str(),
            ]));
            ui.close();
        }
        if ui.button(tr!("复制分支名")).clicked() {
            ui.ctx().copy_text(branch.name.clone());
            ui.close();
        }
        if !branch.is_head {
            ui.separator();
            if ui.button(tr!("删除…")).clicked() {
                self.dialog = Some(Dialog::DeleteBranch {
                    name: branch.name.clone(),
                    upstream: branch.upstream.clone(),
                    delete_remote: false,
                    force: false,
                });
                ui.close();
            }
        }
    }

    fn remote_branch_menu(&mut self, ui: &mut Ui, data: &TabData, name: &str, short: &str) {
        let current = data.refs.current_branch().map(|b| b.name.clone());
        if ui.button(tr!("检出 {name}…", name = name)).clicked() {
            self.checkout_remote(data, name);
            ui.close();
        }
        if let Some(current) = &current {
            if ui
                .button(tr!("合并到 {current}…", current = current))
                .clicked()
            {
                self.dialog = Some(Dialog::Merge {
                    branch: name.to_string(),
                    current: current.clone(),
                    mode: MergeMode::Default,
                });
                ui.close();
            }
            if ui
                .button(tr!("把 {current} 变基到此分支…", current = current))
                .clicked()
            {
                self.dialog = Some(Dialog::Rebase {
                    onto: name.to_string(),
                    current: current.clone(),
                    autostash: true,
                    update_refs: false,
                });
                ui.close();
            }
            if ui
                .button(tr!("设为 {current} 的上游", current = current))
                .clicked()
            {
                self.push_job(GitJob::new(tr!("设置上游"), &self.root).step([
                    "branch",
                    "--set-upstream-to",
                    name,
                ]));
                ui.close();
            }
        }
        if ui.button(tr!("从此创建分支…")).clicked() {
            self.dialog = Some(Dialog::CreateBranch {
                start: name.to_string(),
                start_label: name.to_string(),
                name: short.to_string(),
                checkout: true,
                changes: LocalChanges::Keep,
            });
            ui.close();
        }
        if ui.button(tr!("复制分支名")).clicked() {
            ui.ctx().copy_text(name.to_string());
            ui.close();
        }
        ui.separator();
        if ui.button(tr!("从远程仓库删除…")).clicked() {
            self.dialog = Some(Dialog::DeleteRemoteBranch {
                name: name.to_string(),
            });
            ui.close();
        }
    }

    fn tag_menu(&mut self, ui: &mut Ui, data: &TabData, name: &str) {
        if ui.button(tr!("检出（分离 HEAD）…")).clicked() {
            self.dialog = Some(Dialog::Checkout {
                target: format!("refs/tags/{name}"),
                label: name.to_string(),
                detach: true,
                changes: LocalChanges::Keep,
            });
            ui.close();
        }
        if ui.button(tr!("从此创建分支…")).clicked() {
            self.dialog = Some(Dialog::CreateBranch {
                start: format!("refs/tags/{name}"),
                start_label: name.to_string(),
                name: String::new(),
                checkout: true,
                changes: LocalChanges::Keep,
            });
            ui.close();
        }
        let remote = self.default_remote(data);
        if !remote.is_empty() && ui.button(tr!("推送到 {remote}", remote = remote)).clicked() {
            self.push_job(
                GitJob::new(tr!("推送标签 {name}", name = name), &self.root).step(vec![
                    "push".to_string(),
                    "--progress".into(),
                    remote.clone(),
                    format!("refs/tags/{name}"),
                ]),
            );
            ui.close();
        }
        if ui.button(tr!("复制标签名")).clicked() {
            ui.ctx().copy_text(name.to_string());
            ui.close();
        }
        ui.separator();
        if ui.button(tr!("删除…")).clicked() {
            self.dialog = Some(Dialog::DeleteTag {
                name: name.to_string(),
                delete_remote: false,
                remote,
            });
            ui.close();
        }
    }

    pub(super) fn checkout_branch(&mut self, name: &str) {
        self.dialog = Some(Dialog::Checkout {
            target: name.to_string(),
            label: name.to_string(),
            detach: false,
            changes: LocalChanges::Keep,
        });
    }

    pub(super) fn checkout_remote(&mut self, data: &TabData, name: &str) {
        let short = name.split_once('/').map(|(_, rest)| rest).unwrap_or(name);
        if let Some(local) = data
            .refs
            .local
            .iter()
            .find(|branch| branch.upstream.as_deref() == Some(name))
        {
            if !local.is_head {
                self.checkout_branch(&local.name.clone());
            }
            return;
        }
        let taken = data.refs.local.iter().any(|branch| branch.name == short);
        self.dialog = Some(Dialog::TrackRemote {
            remote_branch: name.to_string(),
            local_name: if taken {
                String::new()
            } else {
                short.to_string()
            },
            changes: LocalChanges::Keep,
        });
    }

    /// 当前分支相对上游的超前和落后提交数。
    pub fn current_counts(&self) -> (usize, usize) {
        self.data
            .as_ref()
            .and_then(|data| data.refs.current_branch())
            .map_or((0, 0), |branch| (branch.ahead, branch.behind))
    }

    pub fn has_stash(&self) -> bool {
        self.data
            .as_ref()
            .is_some_and(|d| !d.refs.stashes.is_empty())
    }

    pub fn last_job_line(&self) -> Option<String> {
        self.job
            .as_ref()
            .filter(|job| job.is_running())
            .map(|job| tr!("{title}：{line}", title = job.title, line = job.last_line()))
    }

    pub fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if let Some(line) = self.last_job_line() {
                ui.spinner();
                ui.label(line);
            } else if let Some((text, error)) = self.notice.clone() {
                let color = if error {
                    ui.visuals().error_fg_color
                } else {
                    ui.visuals().weak_text_color()
                };
                ui.colored_label(color, text);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icons::icon_button(ui, Icon::Close, tr!("关闭")).clicked() {
                        self.notice = None;
                    }
                    if error && ui.small_button(tr!("查看日志")).clicked() {
                        self.show_log = true;
                    }
                });
            }
        });
    }

    pub fn has_status(&self) -> bool {
        self.last_job_line().is_some() || self.notice.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_with_slashes_become_folders() {
        let nodes = tree(&["feature/a", "feature/b", "main", "release/1/x"]);
        let rendered: Vec<String> = nodes
            .iter()
            .map(|node| match node {
                Node::Folder { name, depth, .. } => format!("{depth}:{name}/"),
                Node::Leaf { name, depth, .. } => format!("{depth}:{name}"),
            })
            .collect();
        assert_eq!(
            rendered,
            vec![
                "0:feature/",
                "1:a",
                "1:b",
                "0:main",
                "0:release/",
                "1:1/",
                "2:x"
            ]
        );
    }
}
