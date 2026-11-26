mod commands;
mod details;
mod history;
mod sidebar;

use git_forker::tr;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use eframe::egui::{
    self, Align, Color32, FontId, Key, Layout, Pos2, Rect, RichText, Sense, Ui, Vec2,
};
use git_forker::{
    BlameLine, BlobContent, ChangeKind, CommitDetails, DiffSettings, FileChange, FileDiff, GitJob,
    HeadState, History, HistoryOptions, JobHandle, Oid, PULL_STASH_MESSAGE, PatchDirection,
    RefsSnapshot, Repo, RepoOperation, WorkingTree, git_output, hunk_patch,
};

use super::dialogs::{Dialog, DialogContext, LocalChanges, MergeMode, Outcome, ResetMode};
use super::diff_view::{self, DiffMode, DiffRequest, DiffSelection, DiffViewOptions, HunkOp};
use super::icons::{self, Icon};
use super::platform;
use super::state::Preferences;
use super::widgets::{self, LANE_WIDTH};

/// 提交图最多显示的泳道数，超过部分被裁掉。
const MAX_GRAPH_LANES: usize = 8;
/// 冲突文件预览最多读取的字节数。
const MAX_CONFLICT_PREVIEW: u64 = 2 * 1024 * 1024;
/// 文件历史窗口最多列出的提交数。
const FILE_HISTORY_LIMIT: &str = "1000";
/// 最近说明列表从全部本地和远端引用里读取的提交数。
const MESSAGE_INDEX_LIMIT: &str = "500";
/// 最近说明弹层的固定宽度和列表高度。
const MESSAGE_MENU_WIDTH: f32 = 440.0;
const MESSAGE_MENU_HEIGHT: f32 = 300.0;
/// 列表里一次画出的行数，其余靠关键字继续缩小。
const MESSAGE_PICK_LIMIT: usize = 100;
/// 未暂存和已暂存之间分割条的高度，也是鼠标可拖动的范围。
const CHANGES_SPLIT_HANDLE: f32 = 8.0;
/// 分割之后每一侧至少留下的高度，避免标题被拖没。
const CHANGES_SECTION_MIN: f32 = 72.0;
/// 树状视图里每一级目录的缩进。
const CHANGES_TREE_INDENT: f32 = 14.0;
const CHANGES_TREE_ROW: f32 = 22.0;
/// 未暂存文件行右侧删除按钮占的宽度。
const DISCARD_SLOT: f32 = 22.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    LocalChanges,
    History,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DetailsTab {
    Commit,
    Changes,
    Tree,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarMode {
    Branches,
    Search,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchKind {
    Message,
    Author,
    Sha,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Area {
    Unstaged,
    Staged,
    Conflicted,
}

/// 本地更改里文件的排列方式，对应 Fork 的 View as Tree / List / Combined List。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum ChangesLayout {
    List,
    Tree,
    Combined,
}

#[derive(Clone, Copy)]
struct ListedChange<'a> {
    change: &'a FileChange,
    area: Area,
}

struct ChangeRow {
    response: egui::Response,
    /// 指针停在右侧删除按钮上。
    over_discard: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SelectedChange {
    area: Area,
    path: String,
    kind: ChangeKind,
    old_path: Option<String>,
}

/// 文件树页的文件列表，以及是否因数量上限被截断。
type TreeListing = (Vec<String>, bool);

struct TabData {
    tree: WorkingTree,
    refs: RefsSnapshot,
    history: History,
    graph_width: f32,
    head: Option<Oid>,
    identity: Option<(String, String)>,
}

enum PaletteMode {
    Checkout,
    Commands,
}

struct Palette {
    mode: PaletteMode,
    query: String,
    selected: usize,
}

struct BlameWindow {
    path: String,
    lines: Result<Vec<BlameLine>, String>,
}

struct FileHistoryEntry {
    id: Oid,
    author: String,
    time: i64,
    summary: String,
}

struct FileHistoryWindow {
    path: String,
    entries: Result<Vec<FileHistoryEntry>, String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AfterJob {
    Nothing,
    ClearCommitMessage,
}

/// 标签页向应用发出的请求。
pub enum TabRequest {
    OpenRepository(PathBuf),
    RememberMessage(String),
}

enum Action {
    Reload,
    SelectCommit(Oid),
    RevealCommit(Oid),
    SelectDetailsFile(String),
    SelectTreeFile(String),
    SelectChange(SelectedChange),
    Stage(Vec<String>),
    Unstage(Vec<String>),
    StageAll,
    UnstageAll,
    Diff(DiffRequest),
    Job(GitJob, AfterJob),
    Commit,
    ReloadDiffs,
    Blame(String),
    FileHistory(String),
    Ignore(String),
}

pub struct RepoTab {
    pub root: PathBuf,
    pub name: String,
    pub view: View,
    pub show_details: bool,
    pub history_options: HistoryOptions,
    pub show_log: bool,
    data: Option<TabData>,
    error: Option<String>,
    actions: Vec<Action>,
    pub requests: Vec<TabRequest>,

    search: String,
    search_kind: SearchKind,
    selected_commit: Option<Oid>,
    scroll_to: Option<usize>,
    history_viewport: (f32, f32),

    details: Option<Result<CommitDetails, String>>,
    details_tab: DetailsTab,
    details_file: Option<String>,
    details_diff: Option<Result<FileDiff, String>>,
    tree_files: Option<(Oid, Result<TreeListing, String>)>,
    tree_filter: String,
    tree_file: Option<String>,
    tree_content: Option<Result<BlobContent, String>>,

    sidebar_filter: String,
    sidebar_mode: SidebarMode,
    focus_search: bool,
    focus_commit: bool,
    collapsed: HashSet<String>,
    expanded: HashSet<String>,
    /// 本地更改树状视图里收起的目录，键是 `区域/路径`。
    collapsed_changes: HashSet<String>,
    change_filter: String,
    selected_change: Option<SelectedChange>,
    change_diff: Option<Result<FileDiff, String>>,
    conflict_preview: Option<String>,
    diff_selection: DiffSelection,
    diff_settings: DiffSettings,
    full_file: bool,
    wrap: bool,
    syntax: bool,

    message: String,
    message_query: String,
    /// 最近说明弹层用的提交索引。仓库重新读取后清空，下次打开再取。
    message_index: Option<Vec<CommitMessageRef>>,
    amend: bool,

    job: Option<JobHandle>,
    /// 会自动贮藏的任务开始前的贮藏栈顶；任务结束后用来判断重新应用是否留下了贮藏。
    autostash_before: Option<Option<String>>,
    after_job: AfterJob,
    log: Vec<String>,
    notice: Option<(String, bool)>,
    dialog: Option<Dialog>,
    palette: Option<Palette>,
    blame: Option<BlameWindow>,
    file_history: Option<FileHistoryWindow>,
    recent_messages: Vec<String>,
    terminal: String,
    /// 后台线程正在读取的仓库数据。
    loading: Option<Receiver<Result<TabData, String>>>,
    /// 需要重新读取仓库；在 `update` 里开始，正在读取时等这次读完再开始。
    reload_pending: bool,
    loaded_once: bool,
}

/// 在后台线程里读取仓库页需要的全部数据。大仓库可能要几秒钟。
fn load_data(root: &Path, options: HistoryOptions) -> Result<TabData, String> {
    let repo = Repo::discover(root).map_err(|err| err.to_string())?;
    let tree = repo.working_tree().map_err(|err| err.to_string())?;
    let refs = repo.refs().map_err(|err| err.to_string())?;
    let history = repo
        .history(options, &refs)
        .map_err(|err| err.to_string())?;
    let lanes = history
        .graph
        .iter()
        .map(|row| row.width)
        .max()
        .unwrap_or(1)
        .min(MAX_GRAPH_LANES);
    Ok(TabData {
        tree,
        refs,
        graph_width: lanes as f32 * LANE_WIDTH + 6.0,
        history,
        head: repo.head_id(),
        identity: repo.identity(),
    })
}

impl RepoTab {
    /// 应用设置里的终端程序。
    pub fn set_terminal(&mut self, terminal: &str) {
        self.terminal = terminal.to_string();
    }

    /// 应用设置里的差异默认值，覆盖这个仓库页里临时调整过的值。
    pub fn set_diff_defaults(&mut self, preferences: &Preferences) {
        self.diff_settings = DiffSettings {
            context_lines: preferences.context_lines,
            ignore_whitespace: preferences.ignore_whitespace,
        };
        self.wrap = preferences.wrap_lines;
        self.syntax = preferences.syntax_highlighting;
        self.actions.push(Action::ReloadDiffs);
    }

    pub fn open(root: PathBuf) -> Self {
        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string());
        let mut tab = Self {
            root,
            name,
            view: View::History,
            show_details: true,
            history_options: HistoryOptions::default(),
            show_log: false,
            data: None,
            error: None,
            actions: Vec::new(),
            requests: Vec::new(),
            search: String::new(),
            search_kind: SearchKind::Message,
            selected_commit: None,
            scroll_to: None,
            history_viewport: (0.0, 0.0),
            details: None,
            details_tab: DetailsTab::Commit,
            details_file: None,
            details_diff: None,
            tree_files: None,
            tree_filter: String::new(),
            tree_file: None,
            tree_content: None,
            sidebar_filter: String::new(),
            sidebar_mode: SidebarMode::Branches,
            focus_search: false,
            focus_commit: false,
            collapsed: ["tags", "stashes", "submodules"].map(String::from).into(),
            expanded: HashSet::new(),
            collapsed_changes: HashSet::new(),
            change_filter: String::new(),
            selected_change: None,
            change_diff: None,
            conflict_preview: None,
            diff_selection: DiffSelection::default(),
            diff_settings: DiffSettings::default(),
            full_file: false,
            wrap: false,
            syntax: true,
            message: String::new(),
            message_query: String::new(),
            message_index: None,
            amend: false,
            job: None,
            autostash_before: None,
            after_job: AfterJob::Nothing,
            log: Vec::new(),
            notice: None,
            dialog: None,
            palette: None,
            blame: None,
            file_history: None,
            recent_messages: Vec::new(),
            terminal: String::new(),
            loading: None,
            reload_pending: true,
            loaded_once: false,
        };
        if let Err(err) = Repo::discover(&tab.root) {
            tab.error = Some(err.to_string());
            tab.reload_pending = false;
        }
        tab
    }

    pub fn is_valid(&self) -> bool {
        self.data.is_some() || self.error.is_none()
    }

    /// 后台线程正在读取仓库数据。
    pub fn is_reading(&self) -> bool {
        self.loading.is_some()
    }

    /// 仓库数据正在读取，或者等着读取。
    pub fn is_loading(&self) -> bool {
        self.loading.is_some() || self.reload_pending && self.error.is_none()
    }

    pub fn is_busy(&self) -> bool {
        self.job.as_ref().is_some_and(JobHandle::is_running)
    }

    pub fn has_modal(&self) -> bool {
        self.dialog.is_some() || self.palette.is_some()
    }

    pub fn change_count(&self) -> usize {
        self.data.as_ref().map_or(0, |data| {
            data.tree.unstaged.len() + data.tree.staged.len() + data.tree.conflicted.len()
        })
    }

    pub fn head_text(&self) -> String {
        match self.data.as_ref().map(|data| &data.tree.head) {
            Some(HeadState::Branch(name)) => name.clone(),
            Some(HeadState::Detached(id)) => tr!("分离于 {id}", id = id),
            Some(HeadState::Unborn) => tr!("尚无提交").into(),
            None => String::new(),
        }
    }

    // ---- 读取 ----

    /// 请求重新读取仓库。读取在后台进行，界面先继续显示旧数据。
    pub fn reload(&mut self) {
        self.reload_pending = true;
        self.error = None;
    }

    /// 开始后台读取，或收取读取结果。`active` 为 false 的仓库页从没读取过时不读取，
    /// 等用户切换过来再读。
    fn poll_loading(&mut self, ctx: &egui::Context, active: bool) {
        if let Some(receiver) = &self.loading {
            match receiver.try_recv() {
                Ok(result) => {
                    self.loading = None;
                    self.finish_loading(result);
                }
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.loading = None;
                    self.finish_loading(Err(tr!("无法读取仓库").to_string()));
                }
            }
        }
        if self.reload_pending && (active || self.loaded_once) {
            self.reload_pending = false;
            let (sender, receiver) = mpsc::channel();
            let root = self.root.clone();
            let options = self.history_options;
            let ctx = ctx.clone();
            thread::spawn(move || {
                let _ = sender.send(load_data(&root, options));
                ctx.request_repaint();
            });
            self.loading = Some(receiver);
        }
    }

    fn finish_loading(&mut self, result: Result<TabData, String>) {
        self.message_index = None;
        let data = match result {
            Ok(data) => data,
            Err(err) => {
                self.error = Some(err);
                self.data = None;
                return;
            }
        };
        if !self.loaded_once {
            self.loaded_once = true;
            if !data.tree.unstaged.is_empty()
                || !data.tree.staged.is_empty()
                || !data.tree.conflicted.is_empty()
            {
                self.view = View::LocalChanges;
            }
        }
        self.error = None;
        self.data = Some(data);
        match Repo::discover(&self.root) {
            Ok(repo) => self.validate_selection(&repo),
            Err(err) => self.error = Some(err.to_string()),
        }
    }

    fn validate_selection(&mut self, repo: &Repo) {
        let data = self.data.take().expect("刚加载");
        self.validate_selection_with(repo, &data);
        self.data = Some(data);
    }

    fn validate_selection_with(&mut self, repo: &Repo, data: &TabData) {
        if let Some(selected) = &self.selected_change {
            let list = match selected.area {
                Area::Unstaged => &data.tree.unstaged,
                Area::Staged => &data.tree.staged,
                Area::Conflicted => &data.tree.conflicted,
            };
            match list.iter().find(|change| change.path == selected.path) {
                Some(change) => {
                    let selected = SelectedChange {
                        kind: change.kind,
                        old_path: change.old_path.clone(),
                        ..selected.clone()
                    };
                    self.load_change_diff(repo, &selected);
                    self.selected_change = Some(selected);
                }
                None => {
                    self.selected_change = None;
                    self.change_diff = None;
                    self.conflict_preview = None;
                }
            }
        }
        if self.selected_change.is_none() {
            let first = data
                .tree
                .conflicted
                .first()
                .map(|change| (Area::Conflicted, change))
                .or(data
                    .tree
                    .unstaged
                    .first()
                    .map(|change| (Area::Unstaged, change)))
                .or(data
                    .tree
                    .staged
                    .first()
                    .map(|change| (Area::Staged, change)));
            if let Some((area, change)) = first {
                let selected = SelectedChange {
                    area,
                    path: change.path.clone(),
                    kind: change.kind,
                    old_path: change.old_path.clone(),
                };
                self.load_change_diff(repo, &selected);
                self.selected_change = Some(selected);
            }
        }

        let commit_known = self.selected_commit.is_some_and(|id| {
            data.history.commits.iter().any(|commit| commit.id == id)
                || data.refs.stashes.iter().any(|stash| stash.id == id)
        });
        if !commit_known {
            let head = data.head.or(data.history.commits.first().map(|c| c.id));
            self.selected_commit = None;
            if let Some(head) = head {
                self.select_commit(repo, head);
            } else {
                self.details = None;
                self.details_diff = None;
            }
        }
    }

    fn select_commit(&mut self, repo: &Repo, id: Oid) {
        if self.selected_commit == Some(id) {
            return;
        }
        self.selected_commit = Some(id);
        self.details = Some(repo.commit_details(id).map_err(|err| err.to_string()));
        let first = match &self.details {
            Some(Ok(details)) => {
                let keep = self
                    .details_file
                    .as_ref()
                    .filter(|path| details.files.iter().any(|file| &file.path == *path))
                    .cloned();
                keep.or(details.files.first().map(|file| file.path.clone()))
            }
            _ => None,
        };
        self.details_file = first;
        self.load_details_diff(repo);
        self.tree_files = None;
        self.tree_content = None;
        if self.details_tab == DetailsTab::Tree {
            self.load_tree(repo);
        }
    }

    fn load_details_diff(&mut self, repo: &Repo) {
        self.details_diff = match (self.selected_commit, &self.details_file) {
            (Some(id), Some(path)) => Some(
                repo.commit_file_diff(id, path, self.effective_diff_settings())
                    .map_err(|err| err.to_string()),
            ),
            _ => None,
        };
    }

    fn load_tree(&mut self, repo: &Repo) {
        let Some(id) = self.selected_commit else {
            return;
        };
        if self
            .tree_files
            .as_ref()
            .is_some_and(|(tree_id, _)| *tree_id == id)
        {
            return;
        }
        self.tree_files = Some((id, repo.tree_files(id).map_err(|err| err.to_string())));
        if let Some(path) = self.tree_file.clone() {
            self.tree_content = Some(repo.blob_at(id, &path).map_err(|err| err.to_string()));
        }
    }

    fn load_change_diff(&mut self, repo: &Repo, selected: &SelectedChange) {
        self.diff_selection.clear();
        self.conflict_preview = None;
        if selected.area == Area::Conflicted {
            self.change_diff = None;
            let path = self.root.join(&selected.path);
            self.conflict_preview = std::fs::File::open(path).ok().and_then(|file| {
                use std::io::Read;
                let mut text = String::new();
                file.take(MAX_CONFLICT_PREVIEW)
                    .read_to_string(&mut text)
                    .ok()
                    .map(|_| text)
            });
            return;
        }
        self.change_diff = Some(
            repo.worktree_diff(
                &selected.path,
                selected.area == Area::Staged,
                self.effective_diff_settings(),
            )
            .map_err(|err| err.to_string()),
        );
    }

    fn effective_diff_settings(&self) -> DiffSettings {
        DiffSettings {
            context_lines: if self.full_file {
                u32::MAX / 2
            } else {
                self.diff_settings.context_lines
            },
            ..self.diff_settings
        }
    }

    // ---- 动作 ----

    pub fn push_job(&mut self, job: GitJob) {
        self.actions.push(Action::Job(job, AfterJob::Nothing));
    }

    /// 处理本帧积累的动作，收取后台任务输出和后台读取的仓库数据。
    pub fn update(&mut self, ctx: &egui::Context, active: bool) {
        self.poll_loading(ctx, active);
        if let Some(job) = &mut self.job
            && let Some(ok) = job.poll()
        {
            let title = job.title.clone();
            let summary = job.last_line().to_string();
            self.log.push(tr!(
                "── {title}：{result}",
                title = title,
                result = if ok { tr!("完成") } else { tr!("失败") }
            ));
            self.log.extend(
                job.log
                    .iter()
                    .map(|line| line.trim_start_matches('\r').to_string()),
            );
            let autostash_before = self.autostash_before.take();
            if ok {
                // 自动贮藏重新应用时的冲突不会让 git 返回失败，要自己比对贮藏栈。
                let stash_left = autostash_before
                    .is_some_and(|before| autostash_conflicted(&self.root, &before));
                self.notice = Some(if stash_left {
                    (
                        tr!(
                            "{title}完成，但重新应用本地更改时有冲突，你的改动还在贮藏中",
                            title = title
                        ),
                        true,
                    )
                } else {
                    (tr!("{title}完成", title = title), false)
                });
                if self.after_job == AfterJob::ClearCommitMessage {
                    self.message.clear();
                    self.amend = false;
                }
            } else {
                self.notice = Some((
                    tr!("{title}失败：{summary}", title = title, summary = summary),
                    true,
                ));
            }
            self.after_job = AfterJob::Nothing;
            self.actions.push(Action::Reload);
        }

        let actions = std::mem::take(&mut self.actions);
        if actions.is_empty() {
            return;
        }
        let repo = match Repo::discover(&self.root) {
            Ok(repo) => repo,
            Err(err) => {
                self.error = Some(err.to_string());
                return;
            }
        };
        let mut reload = false;
        for action in actions {
            match action {
                Action::Reload => reload = true,
                Action::SelectCommit(id) => self.select_commit(&repo, id),
                Action::RevealCommit(id) => {
                    self.view = View::History;
                    if let Some(data) = &self.data
                        && let Some(index) = data.history.commits.iter().position(|c| c.id == id)
                    {
                        self.scroll_to = Some(index);
                    }
                    self.select_commit(&repo, id);
                }
                Action::SelectDetailsFile(path) => {
                    self.details_file = Some(path);
                    self.load_details_diff(&repo);
                }
                Action::SelectTreeFile(path) => {
                    if let Some(id) = self.selected_commit {
                        self.tree_content =
                            Some(repo.blob_at(id, &path).map_err(|err| err.to_string()));
                    }
                    self.tree_file = Some(path);
                }
                Action::SelectChange(selected) => {
                    self.load_change_diff(&repo, &selected);
                    self.selected_change = Some(selected);
                }
                Action::Stage(paths) => {
                    self.report(repo.stage_paths(&paths));
                    reload = true;
                }
                Action::Unstage(paths) => {
                    self.report(repo.unstage_paths(&paths));
                    reload = true;
                }
                Action::StageAll => {
                    self.report(repo.stage_all());
                    reload = true;
                }
                Action::UnstageAll => {
                    if let Some(data) = &self.data {
                        let paths: Vec<String> =
                            data.tree.staged.iter().map(|c| c.path.clone()).collect();
                        self.report(repo.unstage_paths(&paths));
                    }
                    reload = true;
                }
                Action::Diff(request) => {
                    self.apply_diff_request(&repo, request);
                    reload = true;
                }
                Action::Job(job, after) => self.start_job(ctx, job, after),
                Action::Commit => self.commit(ctx),
                Action::ReloadDiffs => {
                    if let Some(selected) = self.selected_change.clone() {
                        self.load_change_diff(&repo, &selected);
                    }
                    self.load_details_diff(&repo);
                }
                Action::Blame(path) => {
                    self.blame = Some(BlameWindow {
                        lines: repo.blame(&path).map_err(|err| err.to_string()),
                        path,
                    });
                }
                Action::FileHistory(path) => {
                    self.file_history = Some(FileHistoryWindow {
                        entries: file_history(&self.root, &path),
                        path,
                    });
                }
                Action::Ignore(path) => {
                    let result = append_gitignore(&self.root, &path);
                    self.report(result.map_err(git_forker::Error::Io));
                    reload = true;
                }
            }
        }
        if reload {
            self.reload();
            ctx.request_repaint();
        }
    }

    fn report(&mut self, result: Result<(), git_forker::Error>) {
        if let Err(err) = result {
            self.notice = Some((err.to_string(), true));
        }
    }

    fn start_job(&mut self, ctx: &egui::Context, job: GitJob, after: AfterJob) {
        if self.is_busy() {
            self.notice = Some((tr!("已有任务在运行，请稍后再试").into(), true));
            return;
        }
        let ctx = ctx.clone();
        self.after_job = after;
        self.notice = None;
        self.autostash_before = (job.stash_if_blocked
            || job
                .steps
                .iter()
                .any(|step| step.iter().any(|arg| arg == "--autostash")))
        .then(|| stash_top(&self.root));
        self.job = Some(JobHandle::spawn(job, move || ctx.request_repaint()));
    }

    fn apply_diff_request(&mut self, repo: &Repo, request: DiffRequest) {
        let Some(Ok(diff)) = &self.change_diff else {
            return;
        };
        let Some(hunk) = diff.hunks.get(request.hunk) else {
            return;
        };
        let (direction, to_index) = match request.op {
            HunkOp::Stage => (PatchDirection::Forward, true),
            HunkOp::Unstage => (PatchDirection::Reverse, true),
            HunkOp::Discard => (PatchDirection::Reverse, false),
        };
        let Some(patch) = hunk_patch(&diff.path, hunk, request.lines.as_ref(), direction) else {
            self.notice = Some((tr!("所选内容没有可应用的更改").into(), true));
            return;
        };
        self.report(repo.apply_patch(&patch, to_index));
        self.diff_selection.clear();
    }

    fn commit(&mut self, ctx: &egui::Context) {
        let Some(data) = &self.data else {
            return;
        };
        if data.identity.is_none() {
            self.dialog = Some(Dialog::Identity {
                name: String::new(),
                email: String::new(),
            });
            return;
        }
        let message = self.message.trim().to_string();
        let (subject, _) = split_message(&message);
        if subject.is_empty() {
            self.notice = Some((tr!("请填写提交摘要").into(), true));
            return;
        }
        if data.tree.staged.is_empty() && !self.amend {
            self.notice = Some((tr!("没有已暂存的更改").into(), true));
            return;
        }
        if !data.tree.conflicted.is_empty() {
            self.notice = Some((tr!("请先解决冲突").into(), true));
            return;
        }
        let mut args = vec!["commit".to_string(), "-m".into(), message.clone()];
        if self.amend {
            args.push("--amend".into());
        }
        self.requests.push(TabRequest::RememberMessage(message));
        let title = if self.amend {
            tr!("修改上次提交")
        } else {
            tr!("提交")
        };
        let job = GitJob::new(title, &self.root).step(args);
        self.start_job(ctx, job, AfterJob::ClearCommitMessage);
    }

    // ---- 工具栏命令 ----

    fn upstream_remote(&self, data: &TabData) -> Option<(String, String)> {
        let branch = data.refs.current_branch()?;
        let upstream = branch.upstream.as_ref()?;
        data.refs
            .remotes
            .iter()
            .filter(|remote| upstream.starts_with(&format!("{}/", remote.name)))
            .max_by_key(|remote| remote.name.len())
            .map(|remote| {
                (
                    remote.name.clone(),
                    upstream[remote.name.len() + 1..].to_string(),
                )
            })
    }

    fn default_remote(&self, data: &TabData) -> String {
        self.upstream_remote(data)
            .map(|(remote, _)| remote)
            .or_else(|| {
                data.refs
                    .remotes
                    .iter()
                    .find(|r| r.name == "origin")
                    .map(|r| r.name.clone())
            })
            .or_else(|| data.refs.remotes.first().map(|r| r.name.clone()))
            .unwrap_or_default()
    }

    pub fn open_fetch(&mut self) {
        let Some(data) = &self.data else { return };
        self.dialog = Some(Dialog::Fetch {
            remote: self.default_remote(data),
            all: data.refs.remotes.len() > 1,
            prune: false,
        });
    }

    pub fn open_pull(&mut self) {
        let Some(data) = &self.data else { return };
        let (remote, branch) = self.upstream_remote(data).unwrap_or_else(|| {
            let remote = self.default_remote(data);
            let branch = data
                .refs
                .current_branch()
                .map(|b| b.name.clone())
                .unwrap_or_default();
            (remote, branch)
        });
        self.dialog = Some(Dialog::Pull {
            remote,
            branch,
            rebase: false,
            autostash: true,
        });
    }

    pub fn open_push(&mut self) {
        let Some(data) = &self.data else { return };
        let Some(current) = data.refs.current_branch() else {
            self.notice = Some((tr!("当前不在任何分支上，无法推送").into(), true));
            return;
        };
        let upstream = self.upstream_remote(data);
        self.dialog = Some(Dialog::Push {
            local: current.name.clone(),
            remote: upstream
                .as_ref()
                .map(|(remote, _)| remote.clone())
                .unwrap_or_else(|| self.default_remote(data)),
            remote_branch: upstream
                .as_ref()
                .map(|(_, branch)| branch.clone())
                .unwrap_or_else(|| current.name.clone()),
            track: upstream.is_none(),
            force: false,
            tags: false,
        });
    }

    pub fn open_stash(&mut self) {
        let keep_index = self
            .data
            .as_ref()
            .is_some_and(|data| !data.tree.staged.is_empty());
        self.dialog = Some(Dialog::CreateStash {
            message: String::new(),
            include_untracked: true,
            keep_index,
        });
    }

    pub fn pop_stash(&mut self) {
        let Some(data) = &self.data else { return };
        match data.refs.stashes.first() {
            Some(stash) => {
                self.dialog = Some(Dialog::ApplyStash {
                    index: stash.index,
                    message: stash.message.clone(),
                    delete_after: true,
                })
            }
            None => self.notice = Some((tr!("没有贮藏").into(), true)),
        }
    }

    pub fn open_create_branch(&mut self) {
        let (start, label) = self.selected_start_point();
        self.dialog = Some(Dialog::CreateBranch {
            start,
            start_label: label,
            name: String::new(),
            checkout: true,
            changes: LocalChanges::Keep,
        });
    }

    pub fn open_create_tag(&mut self) {
        let Some(data) = &self.data else { return };
        let (target, label) = self.selected_start_point();
        self.dialog = Some(Dialog::CreateTag {
            target,
            target_label: label,
            name: String::new(),
            message: String::new(),
            push: false,
            remote: self.default_remote(data),
        });
    }

    fn selected_start_point(&self) -> (String, String) {
        match (self.view, self.selected_commit, &self.data) {
            (View::History, Some(id), Some(data)) => {
                let summary = data
                    .history
                    .commits
                    .iter()
                    .find(|commit| commit.id == id)
                    .map(|commit| commit.summary.clone())
                    .unwrap_or_default();
                (
                    id.to_string(),
                    format!("{} {summary}", git_forker::short_id(&id)),
                )
            }
            _ => (
                "HEAD".into(),
                tr!("HEAD（{head_text}）", head_text = self.head_text()),
            ),
        }
    }

    pub fn open_quick_checkout(&mut self) {
        self.palette = Some(Palette {
            mode: PaletteMode::Checkout,
            query: String::new(),
            selected: 0,
        });
    }

    pub fn open_command_palette(&mut self) {
        self.palette = Some(Palette {
            mode: PaletteMode::Commands,
            query: String::new(),
            selected: 0,
        });
    }

    pub fn show_head(&mut self) {
        if let Some(head) = self.data.as_ref().and_then(|data| data.head) {
            self.actions.push(Action::RevealCommit(head));
        }
    }

    pub fn open_terminal(&mut self) {
        if let Err(err) = platform::open_terminal(&self.root, &self.terminal) {
            self.notice = Some((err, true));
        }
    }

    pub fn reveal_in_file_manager(&mut self) {
        if let Err(err) = platform::reveal(&self.root) {
            self.notice = Some((err, true));
        }
    }

    pub fn add_remote(&mut self) {
        self.dialog = Some(Dialog::EditRemote {
            original: None,
            name: if self
                .data
                .as_ref()
                .is_some_and(|d| d.refs.remotes.is_empty())
            {
                "origin".into()
            } else {
                String::new()
            },
            url: String::new(),
        });
    }

    pub fn continue_operation(&mut self, abort: bool) {
        let Some(data) = &self.data else { return };
        let command = match data.tree.operation {
            RepoOperation::Merge => "merge",
            RepoOperation::Rebase => "rebase",
            RepoOperation::CherryPick => "cherry-pick",
            RepoOperation::Revert => "revert",
            RepoOperation::ApplyMailbox => "am",
            RepoOperation::Bisect => {
                self.push_job(
                    GitJob::new(tr!("结束二分查找"), &self.root).step(["bisect", "reset"]),
                );
                return;
            }
            RepoOperation::None => return,
        };
        let flag = if abort { "--abort" } else { "--continue" };
        let title = format!("{command} {flag}");
        self.push_job(GitJob::new(title, &self.root).step([command, flag]));
    }

    // ---- 界面 ----

    pub fn main(&mut self, ui: &mut Ui) {
        let Some(data) = self.data.take() else {
            if self.is_loading() {
                ui.centered_and_justified(|ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(tr!("正在读取仓库…"));
                    });
                });
                return;
            }
            ui.centered_and_justified(|ui| {
                ui.label(self.error.as_deref().unwrap_or(tr!("无法读取仓库")));
            });
            return;
        };
        self.operation_banner(ui, &data);
        match self.view {
            View::LocalChanges => self.changes_view(ui, &data),
            View::History => self.history_view(ui, &data),
        }
        self.data = Some(data);
    }

    fn operation_banner(&mut self, ui: &mut Ui, data: &TabData) {
        let text = match data.tree.operation {
            RepoOperation::None => return,
            RepoOperation::Merge => tr!("正在合并。解决冲突并暂存后继续，或中止合并。"),
            RepoOperation::Rebase => tr!("正在变基。解决冲突并暂存后继续，或中止变基。"),
            RepoOperation::CherryPick => tr!("正在拣选。解决冲突并暂存后继续，或中止。"),
            RepoOperation::Revert => tr!("正在还原。解决冲突并暂存后继续，或中止。"),
            RepoOperation::ApplyMailbox => tr!("正在应用补丁。"),
            RepoOperation::Bisect => tr!("正在二分查找。"),
        };
        egui::Frame::new()
            .fill(Color32::from_rgba_unmultiplied(200, 140, 20, 60))
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("⚠ {text}")).strong());
                    if !data.tree.conflicted.is_empty() {
                        ui.label(tr!(
                            "{count} 个冲突文件",
                            count = data.tree.conflicted.len()
                        ));
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button(tr!("中止")).clicked() {
                            self.continue_operation(true);
                        }
                        if data.tree.operation != RepoOperation::Bisect
                            && ui
                                .add_enabled(
                                    data.tree.conflicted.is_empty(),
                                    egui::Button::new(tr!("继续")),
                                )
                                .clicked()
                        {
                            self.continue_operation(false);
                        }
                    });
                });
            });
    }

    // ---- 历史 ----

    fn commit_menu(&mut self, ui: &mut Ui, data: &TabData, index: usize) {
        let commit = &data.history.commits[index];
        let sha = commit.id.to_string();
        let short = commit.short.clone();
        let label = format!("{short} {}", commit.summary);
        let current = data.refs.current_branch().map(|b| b.name.clone());
        let is_head = data.head == Some(commit.id);
        let branches: Vec<String> = data
            .history
            .labels
            .get(&commit.id)
            .map(|labels| {
                labels
                    .iter()
                    .filter(|l| {
                        matches!(
                            l.kind,
                            git_forker::RefKind::LocalBranch | git_forker::RefKind::RemoteBranch
                        )
                    })
                    .map(|l| l.name.clone())
                    .collect()
            })
            .unwrap_or_default();

        for branch in &branches {
            if data
                .refs
                .local
                .iter()
                .any(|b| &b.name == branch && !b.is_head)
            {
                if ui.button(tr!("检出 {branch}", branch = branch)).clicked() {
                    self.checkout_branch(branch);
                    ui.close();
                }
            } else if data
                .refs
                .remotes
                .iter()
                .any(|r| r.branches.iter().any(|b| &b.name == branch))
                && ui.button(tr!("检出 {branch}…", branch = branch)).clicked()
            {
                self.checkout_remote(data, branch);
                ui.close();
            }
        }
        if ui
            .button(tr!("检出 {short}（分离 HEAD）…", short = short))
            .clicked()
        {
            self.dialog = Some(Dialog::Checkout {
                target: sha.clone(),
                label: label.clone(),
                detach: true,
                changes: LocalChanges::Keep,
            });
            ui.close();
        }
        ui.separator();
        if ui.button(tr!("在此创建分支…")).clicked() {
            self.dialog = Some(Dialog::CreateBranch {
                start: sha.clone(),
                start_label: label.clone(),
                name: String::new(),
                checkout: true,
                changes: LocalChanges::Keep,
            });
            ui.close();
        }
        if ui.button(tr!("在此创建标签…")).clicked() {
            self.dialog = Some(Dialog::CreateTag {
                target: sha.clone(),
                target_label: label.clone(),
                name: String::new(),
                message: String::new(),
                push: false,
                remote: self.default_remote(data),
            });
            ui.close();
        }
        ui.separator();
        if let Some(current) = &current {
            let merge_target = branches.first().cloned().unwrap_or_else(|| sha.clone());
            if !is_head
                && ui
                    .button(tr!("合并到 {current}…", current = current))
                    .clicked()
            {
                self.dialog = Some(Dialog::Merge {
                    branch: merge_target.clone(),
                    current: current.clone(),
                    mode: MergeMode::Default,
                });
                ui.close();
            }
            if !is_head
                && ui
                    .button(tr!("把 {current} 变基到此…", current = current))
                    .clicked()
            {
                self.dialog = Some(Dialog::Rebase {
                    onto: merge_target,
                    current: current.clone(),
                    autostash: true,
                    update_refs: false,
                });
                ui.close();
            }
        }
        if !is_head && ui.button(tr!("拣选…")).clicked() {
            self.dialog = Some(Dialog::CherryPick {
                sha: sha.clone(),
                summary: commit.summary.clone(),
                append_origin: false,
                commit: true,
                parents: commit.parents.len(),
                parent: 0,
            });
            ui.close();
        }
        if ui.button(tr!("还原此提交…")).clicked() {
            self.dialog = Some(Dialog::Revert {
                sha: sha.clone(),
                summary: commit.summary.clone(),
                commit: true,
                parents: commit.parents.len(),
                parent: 0,
            });
            ui.close();
        }
        if let Some(current) = &current
            && !is_head
            && ui
                .button(tr!("把 {current} 重置到此…", current = current))
                .clicked()
        {
            self.dialog = Some(Dialog::Reset {
                target: sha.clone(),
                label: label.clone(),
                current: current.clone(),
                mode: ResetMode::Mixed,
            });
            ui.close();
        }
        if is_head && ui.button(tr!("修改提交说明…")).clicked() {
            let message = match &self.details {
                Some(Ok(details)) if details.id == commit.id => details.message.clone(),
                _ => commit.summary.clone(),
            };
            let (subject, description) = split_message(&message);
            self.dialog = Some(Dialog::Reword {
                subject,
                description,
            });
            ui.close();
        }
        ui.separator();
        if ui.button(tr!("复制 SHA")).clicked() {
            ui.ctx().copy_text(sha.clone());
            ui.close();
        }
        if ui.button(tr!("复制说明")).clicked() {
            ui.ctx().copy_text(commit.summary.clone());
            ui.close();
        }
    }

    fn tree_view(&mut self, ui: &mut Ui) {
        let Some((_, files)) = &self.tree_files else {
            ui.weak(tr!("选择一个提交查看文件树"));
            return;
        };
        let (files, truncated) = match files {
            Ok(files) => files.clone(),
            Err(err) => {
                ui.colored_label(ui.visuals().error_fg_color, err.clone());
                return;
            }
        };
        egui::Panel::left("tree-files")
            .resizable(true)
            .default_size(340.0)
            .show(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.tree_filter)
                        .hint_text(Icon::Search.with(tr!("过滤文件")))
                        .desired_width(f32::INFINITY),
                );
                let filter = self.tree_filter.to_lowercase();
                let visible: Vec<&String> = files
                    .iter()
                    .filter(|path| filter.is_empty() || path.to_lowercase().contains(&filter))
                    .collect();
                egui::ScrollArea::vertical()
                    .id_salt("tree-list")
                    .auto_shrink(false)
                    .show_rows(ui, 18.0, visible.len(), |ui, range| {
                        for path in &visible[range] {
                            let selected = self.tree_file.as_deref() == Some(path.as_str());
                            let response = ui.selectable_label(selected, path.as_str());
                            if response.clicked() {
                                self.actions.push(Action::SelectTreeFile((*path).clone()));
                            }
                            response.context_menu(|ui| self.file_menu(ui, path, true));
                        }
                    });
                if truncated {
                    ui.weak(tr!("文件过多，只列出了一部分"));
                }
            });
        egui::CentralPanel::default().show(ui, |ui| match &self.tree_content {
            None => {
                ui.weak(tr!("选择一个文件"));
            }
            Some(Err(err)) => {
                ui.colored_label(ui.visuals().error_fg_color, err);
            }
            Some(Ok(BlobContent::Binary { size })) => {
                ui.weak(tr!("二进制文件，{size} 字节", size = size));
            }
            Some(Ok(BlobContent::TooLarge { size })) => {
                ui.weak(tr!("文件过大（{size} 字节），不显示内容", size = size));
            }
            Some(Ok(BlobContent::Text(text))) => {
                let lines: Vec<&str> = text.lines().collect();
                let digits = lines.len().to_string().len();
                egui::ScrollArea::both()
                    .id_salt("tree-content")
                    .auto_shrink(false)
                    .show_rows(ui, 18.0, lines.len(), |ui, range| {
                        for index in range {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("{:>digits$}", index + 1))
                                        .monospace()
                                        .color(ui.visuals().weak_text_color()),
                                );
                                ui.label(
                                    RichText::new(lines[index].replace('\t', "    ")).monospace(),
                                );
                            });
                        }
                    });
            }
        });
    }

    /// 文件右键菜单。`committed` 表示文件来自某个提交而不是工作区。
    fn file_menu(&mut self, ui: &mut Ui, path: &str, committed: bool) {
        if ui.button(tr!("文件历史")).clicked() {
            self.actions.push(Action::FileHistory(path.to_string()));
            ui.close();
        }
        if ui.button(tr!("追溯（逐行作者）")).clicked() {
            self.actions.push(Action::Blame(path.to_string()));
            ui.close();
        }
        ui.separator();
        if ui.button(tr!("在文件管理器中显示")).clicked() {
            if let Err(err) = platform::reveal(&self.root.join(path)) {
                self.notice = Some((err, true));
            }
            ui.close();
        }
        if ui.button(tr!("复制路径")).clicked() {
            ui.ctx().copy_text(path.to_string());
            ui.close();
        }
        if !committed && ui.button(tr!("添加到 .gitignore")).clicked() {
            self.actions.push(Action::Ignore(path.to_string()));
            ui.close();
        }
    }

    fn diff_header(&mut self, ui: &mut Ui, path: &str, extra: Option<&mut dyn FnMut(&mut Ui)>) {
        // 选项组的宽度随语言变化，只能沿用上一帧量到的宽度；放不下时换到下一行。
        let options_width = egui::Id::unique("diff-header-options-width");
        let needed: f32 = ui.data(|data| data.get_temp(options_width)).unwrap_or(0.0);
        let fits = ui
            .horizontal(|ui| {
                ui.label(RichText::new(path).strong().monospace());
                if let Some(extra) = extra {
                    extra(ui);
                }
                let fits = ui.available_width() >= needed;
                if fits {
                    self.diff_options(ui, options_width);
                }
                fits
            })
            .inner;
        if !fits {
            ui.horizontal(|ui| self.diff_options(ui, options_width));
        }
        ui.separator();
    }

    fn diff_options(&mut self, ui: &mut Ui, width_id: egui::Id) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let start = ui.cursor().right();
            {
                let mut changed = false;
                ui.toggle_value(&mut self.syntax, tr!("语法着色"));
                ui.toggle_value(&mut self.wrap, tr!("换行"));
                changed |= ui
                    .toggle_value(&mut self.full_file, tr!("完整文件"))
                    .changed();
                if ui
                    .add_enabled(
                        !self.full_file,
                        egui::Button::new(Icon::Plus.glyph().to_string()).small(),
                    )
                    .on_hover_text(tr!("增加上下文行"))
                    .clicked()
                {
                    self.diff_settings.context_lines =
                        (self.diff_settings.context_lines + 3).min(500);
                    changed = true;
                }
                ui.weak(tr!(
                    "{context_lines} 行",
                    context_lines = self.diff_settings.context_lines
                ));
                if ui
                    .add_enabled(
                        !self.full_file,
                        egui::Button::new(Icon::Minus.glyph().to_string()).small(),
                    )
                    .on_hover_text(tr!("减少上下文行"))
                    .clicked()
                {
                    self.diff_settings.context_lines =
                        self.diff_settings.context_lines.saturating_sub(3);
                    changed = true;
                }
                changed |= ui
                    .toggle_value(&mut self.diff_settings.ignore_whitespace, tr!("忽略空白"))
                    .changed();
                if changed {
                    self.actions.push(Action::ReloadDiffs);
                }
            }
            let used = start - ui.cursor().right() + ui.spacing().item_spacing.x;
            let previous: Option<f32> = ui.data_mut(|data| {
                let previous = data.get_temp(width_id);
                data.insert_temp(width_id, used);
                previous
            });
            if previous != Some(used) {
                ui.ctx().request_repaint();
            }
        });
    }

    // ---- 本地更改 ----

    fn changes_view(&mut self, ui: &mut Ui, data: &TabData) {
        self.discard_selected_on_delete(ui, data);
        egui::Panel::left("changes-files")
            .resizable(true)
            .default_size(360.0)
            .min_size(260.0)
            .show(ui, |ui| self.change_lists(ui, data));
        egui::Panel::bottom("commit-box")
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(egui::Margin::symmetric(10, 8)),
            )
            .show(ui, |ui| self.commit_box(ui, data));
        egui::CentralPanel::default().show(ui, |ui| self.change_diff_view(ui));
    }

    fn change_lists(&mut self, ui: &mut Ui, data: &TabData) {
        ui.add(
            egui::TextEdit::singleline(&mut self.change_filter)
                .hint_text(Icon::Search.with(tr!("过滤文件")))
                .desired_width(f32::INFINITY),
        );
        let filter = self.change_filter.to_lowercase();
        let keep = |change: &&FileChange| {
            filter.is_empty() || change.path.to_lowercase().contains(&filter)
        };
        let unstaged_items = listed_changes(
            &data.tree.conflicted,
            Area::Conflicted,
            &data.tree.unstaged,
            Area::Unstaged,
            &keep,
        );
        let staged_items =
            listed_changes(&data.tree.staged, Area::Staged, &[], Area::Staged, &keep);

        let available = ui.available_rect_before_wrap();
        let ratio = changes_split_ratio(ui);
        let (top_rect, handle_rect, bottom_rect) = changes_split_rects(available, ratio);
        // 先占住整块区域，后面的子界面和分割条画在它上面，才能吃到点击。
        let _ = ui.allocate_rect(available, Sense::hover());

        let mut top = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("changes-unstaged")
                .max_rect(top_rect)
                .layout(Layout::top_down(Align::Min)),
        );
        top.shrink_clip_rect(top_rect);
        self.change_section(
            &mut top,
            tr!(
                "未暂存  {count}",
                count = data.tree.unstaged.len() + data.tree.conflicted.len()
            ),
            true,
            tr!("暂存"),
            self.selected_change
                .as_ref()
                .filter(|selected| selected.area == Area::Unstaged)
                .map(|selected| Action::Stage(vec![selected.path.clone()])),
            tr!("全部暂存"),
            !data.tree.unstaged.is_empty(),
            Some(Action::StageAll),
            &unstaged_items,
            "unstaged",
            tr!("没有未暂存的更改"),
        );

        let mut bottom = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("changes-staged")
                .max_rect(bottom_rect)
                .layout(Layout::top_down(Align::Min)),
        );
        bottom.shrink_clip_rect(bottom_rect);
        self.change_section(
            &mut bottom,
            tr!("已暂存  {count}", count = data.tree.staged.len()),
            false,
            tr!("取消暂存"),
            self.selected_change
                .as_ref()
                .filter(|selected| selected.area == Area::Staged)
                .map(|selected| Action::Unstage(vec![selected.path.clone()])),
            tr!("全部取消暂存"),
            !data.tree.staged.is_empty(),
            Some(Action::UnstageAll),
            &staged_items,
            "staged",
            tr!("没有已暂存的更改"),
        );

        paint_changes_split(ui, available, handle_rect);
    }

    /// 选中未暂存文件时，Delete 键放弃它的改动。Mac 上标着 delete 的键是退格。
    fn discard_selected_on_delete(&mut self, ui: &mut Ui, data: &TabData) {
        if self.has_modal() || ui.ctx().text_edit_focused() {
            return;
        }
        let Some(selected) = &self.selected_change else {
            return;
        };
        if selected.area != Area::Unstaged {
            return;
        }
        let pressed = ui.input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, Key::Delete)
                || input.consume_key(egui::Modifiers::NONE, Key::Backspace)
        });
        if !pressed {
            return;
        }
        let Some(change) = data
            .tree
            .unstaged
            .iter()
            .find(|change| change.path == selected.path)
            .cloned()
        else {
            return;
        };
        self.dialog = Some(Dialog::Discard {
            changes: vec![change],
        });
    }

    fn change_section(
        &mut self,
        ui: &mut Ui,
        title: String,
        view_menu: bool,
        primary_label: &str,
        mut primary: Option<Action>,
        all_label: &str,
        all_enabled: bool,
        mut all: Option<Action>,
        items: &[ListedChange<'_>],
        group: &str,
        empty: &str,
    ) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).strong());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(all_enabled, egui::Button::new(all_label).small())
                    .clicked()
                    && let Some(action) = all.take()
                {
                    self.actions.push(action);
                }
                if ui
                    .add_enabled(primary.is_some(), egui::Button::new(primary_label).small())
                    .clicked()
                    && let Some(action) = primary.take()
                {
                    self.actions.push(action);
                }
                if view_menu {
                    self.changes_view_button(ui);
                }
            });
        });
        self.show_change_files(ui, items, group, empty);
    }

    fn changes_view_button(&mut self, ui: &mut Ui) {
        let response = icons::icon_button(ui, Icon::ListTree, tr!("更改的显示方式"));
        let current = changes_layout(ui);
        egui::Popup::menu(&response).show(|ui| {
            for (layout, label) in [
                (ChangesLayout::Tree, tr!("以树状显示")),
                (ChangesLayout::List, tr!("以列表显示")),
                (ChangesLayout::Combined, tr!("以合并列表显示")),
            ] {
                if ui.selectable_label(current == layout, label).clicked() {
                    set_changes_layout(ui, layout);
                    ui.close();
                }
            }
        });
    }

    fn show_change_files(
        &mut self,
        ui: &mut Ui,
        items: &[ListedChange<'_>],
        group: &str,
        empty: &str,
    ) {
        let layout = changes_layout(ui);
        egui::ScrollArea::vertical()
            .id_salt(group)
            .auto_shrink(false)
            .show(ui, |ui| {
                if items.is_empty() {
                    ui.weak(empty);
                    return;
                }
                match layout {
                    ChangesLayout::List => {
                        for item in items {
                            self.change_row(ui, item.change, item.area);
                        }
                    }
                    ChangesLayout::Combined => {
                        let mut sorted = items.to_vec();
                        sorted.sort_by(|a, b| a.change.path.cmp(&b.change.path));
                        for item in &sorted {
                            self.combined_row(ui, item.change, item.area);
                        }
                    }
                    ChangesLayout::Tree => self.change_tree(ui, items, group),
                }
            });
    }

    fn change_tree(&mut self, ui: &mut Ui, items: &[ListedChange<'_>], group: &str) {
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut sorted = items.to_vec();
        sorted.sort_by(|a, b| a.change.path.cmp(&b.change.path));
        let names: Vec<&str> = sorted
            .iter()
            .map(|item| item.change.path.as_str())
            .collect();
        let nodes = sidebar::tree(&names);
        let forced_open = !self.change_filter.is_empty();
        let mut hidden_below: Option<usize> = None;
        for node in nodes {
            let depth = match &node {
                sidebar::Node::Folder { depth, .. } | sidebar::Node::Leaf { depth, .. } => *depth,
            };
            if let Some(limit) = hidden_below {
                if depth > limit {
                    continue;
                }
                hidden_below = None;
            }
            match node {
                sidebar::Node::Folder { path, name, depth } => {
                    let key = format!("{group}/{path}");
                    let open = forced_open || !self.collapsed_changes.contains(&key);
                    let prefix = format!("{path}/");
                    let count = sorted
                        .iter()
                        .filter(|item| item.change.path.starts_with(&prefix))
                        .count();
                    let response = change_folder_row(ui, name, depth, open)
                        .on_hover_text(tr!("{count} 个文件", count = count));
                    if response.clicked() && !response.double_clicked() && !forced_open {
                        if self.collapsed_changes.contains(&key) {
                            self.collapsed_changes.remove(&key);
                        } else {
                            self.collapsed_changes.insert(key);
                        }
                    }
                    let actionable = sorted.iter().any(|item| {
                        item.area != Area::Conflicted && item.change.path.starts_with(&prefix)
                    });
                    if actionable {
                        response.context_menu(|ui| self.change_folder_menu(ui, &path, &sorted));
                    }
                    if !open {
                        hidden_below = Some(depth);
                    }
                }
                sidebar::Node::Leaf { index, name, depth } => {
                    let item = sorted[index];
                    self.change_tree_file(ui, item.change, item.area, name, depth);
                }
            }
        }
    }

    fn change_folder_menu(&mut self, ui: &mut Ui, folder: &str, items: &[ListedChange<'_>]) {
        let prefix = format!("{folder}/");
        let under: Vec<ListedChange<'_>> = items
            .iter()
            .copied()
            .filter(|item| item.change.path.starts_with(&prefix))
            .collect();
        if under.iter().any(|item| item.area == Area::Staged) {
            if ui.button(tr!("取消暂存此文件夹")).clicked() {
                let paths = under.iter().map(|item| item.change.path.clone()).collect();
                self.actions.push(Action::Unstage(paths));
                ui.close();
            }
            return;
        }
        let unstaged: Vec<&FileChange> = under
            .iter()
            .filter(|item| item.area == Area::Unstaged)
            .map(|item| item.change)
            .collect();
        if unstaged.is_empty() {
            return;
        }
        if ui.button(tr!("暂存此文件夹")).clicked() {
            self.actions.push(Action::Stage(
                unstaged.iter().map(|change| change.path.clone()).collect(),
            ));
            ui.close();
        }
        if ui.button(tr!("放弃此文件夹的更改…")).clicked() {
            self.dialog = Some(Dialog::Discard {
                changes: unstaged.into_iter().cloned().collect(),
            });
            ui.close();
        }
    }

    fn change_row(&mut self, ui: &mut Ui, change: &FileChange, area: Area) {
        let row = file_row(
            ui,
            change,
            self.change_selected(area, &change.path),
            area == Area::Unstaged,
        );
        self.bind_change(row, change, area);
    }

    fn combined_row(&mut self, ui: &mut Ui, change: &FileChange, area: Area) {
        let row = combined_file_row(
            ui,
            change,
            self.change_selected(area, &change.path),
            area == Area::Unstaged,
        );
        self.bind_change(row, change, area);
    }

    fn change_tree_file(
        &mut self,
        ui: &mut Ui,
        change: &FileChange,
        area: Area,
        name: &str,
        depth: usize,
    ) {
        let row = tree_file_row(
            ui,
            change.kind,
            name,
            depth,
            self.change_selected(area, &change.path),
            area == Area::Unstaged,
        );
        self.bind_change(row, change, area);
    }

    fn change_selected(&self, area: Area, path: &str) -> bool {
        self.selected_change
            .as_ref()
            .is_some_and(|selected| selected.area == area && selected.path == path)
    }

    fn bind_change(&mut self, row: ChangeRow, change: &FileChange, area: Area) {
        let ChangeRow {
            response,
            over_discard,
        } = row;
        let entry = SelectedChange {
            area,
            path: change.path.clone(),
            kind: change.kind,
            old_path: change.old_path.clone(),
        };
        if response.clicked() {
            self.actions.push(Action::SelectChange(entry));
        }
        // 点在删除按钮上时不要双击暂存，否则确认放弃之前改动已经进了暂存区。
        if response.double_clicked() && !over_discard {
            match area {
                Area::Unstaged => self.actions.push(Action::Stage(vec![change.path.clone()])),
                Area::Staged => self
                    .actions
                    .push(Action::Unstage(vec![change.path.clone()])),
                Area::Conflicted => {}
            }
        }
        if over_discard && response.clicked() {
            self.dialog = Some(Dialog::Discard {
                changes: vec![change.clone()],
            });
        }
        let tip = if over_discard {
            discard_tip(change.kind).to_string()
        } else {
            format!(
                "{}{}",
                widgets::change_name(change.kind),
                change
                    .old_path
                    .as_ref()
                    .map(|old| tr!("，原路径 {old}", old = old))
                    .unwrap_or_default()
            )
        };
        let response = response.on_hover_text(tip);
        response.context_menu(|ui| {
            match area {
                Area::Unstaged => {
                    if ui.button(tr!("暂存")).clicked() {
                        self.actions.push(Action::Stage(vec![change.path.clone()]));
                        ui.close();
                    }
                    if ui.button(tr!("放弃更改…")).clicked() {
                        self.dialog = Some(Dialog::Discard {
                            changes: vec![change.clone()],
                        });
                        ui.close();
                    }
                }
                Area::Staged => {
                    if ui.button(tr!("取消暂存")).clicked() {
                        self.actions
                            .push(Action::Unstage(vec![change.path.clone()]));
                        ui.close();
                    }
                }
                Area::Conflicted => self.conflict_menu(ui, &change.path),
            }
            ui.separator();
            self.file_menu(ui, &change.path, false);
        });
    }

    fn conflict_menu(&mut self, ui: &mut Ui, path: &str) {
        if ui.button(tr!("使用我方版本 (ours)")).clicked() {
            self.resolve(path, Some("--ours"));
            ui.close();
        }
        if ui.button(tr!("使用对方版本 (theirs)")).clicked() {
            self.resolve(path, Some("--theirs"));
            ui.close();
        }
        if ui.button(tr!("标记为已解决")).clicked() {
            self.resolve(path, None);
            ui.close();
        }
        if ui.button(tr!("用合并工具打开")).clicked() {
            self.push_job(
                GitJob::new(tr!("合并工具 {path}", path = path), &self.root).step([
                    "mergetool",
                    "--no-prompt",
                    "--",
                    path,
                ]),
            );
            ui.close();
        }
    }

    fn resolve(&mut self, path: &str, side: Option<&str>) {
        let mut job = GitJob::new(tr!("解决冲突 {path}", path = path), &self.root);
        if let Some(side) = side {
            job = job.step(["checkout", side, "--", path]);
        }
        self.push_job(job.step(["add", "--", path]));
    }

    fn change_diff_view(&mut self, ui: &mut Ui) {
        let Some(selected) = self.selected_change.clone() else {
            ui.centered_and_justified(|ui| ui.weak(tr!("工作区没有更改")));
            return;
        };
        if selected.area == Area::Conflicted {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&selected.path).strong().monospace());
                ui.colored_label(widgets::change_color(ChangeKind::Conflicted), tr!("冲突"));
            });
            ui.horizontal(|ui| {
                if ui.button(tr!("使用我方版本")).clicked() {
                    self.resolve(&selected.path, Some("--ours"));
                }
                if ui.button(tr!("使用对方版本")).clicked() {
                    self.resolve(&selected.path, Some("--theirs"));
                }
                if ui.button(tr!("标记为已解决")).clicked() {
                    self.resolve(&selected.path, None);
                }
                if ui.button(tr!("合并工具")).clicked() {
                    self.push_job(
                        GitJob::new(tr!("合并工具 {path}", path = selected.path), &self.root)
                            .step(["mergetool", "--no-prompt", "--", selected.path.as_str()]),
                    );
                }
            });
            ui.separator();
            match &self.conflict_preview {
                Some(text) => {
                    let lines: Vec<&str> = text.lines().collect();
                    egui::ScrollArea::both()
                        .id_salt("conflict")
                        .auto_shrink(false)
                        .show_rows(ui, 18.0, lines.len(), |ui, range| {
                            for line in &lines[range] {
                                let color = if line.starts_with("<<<<<<<")
                                    || line.starts_with("=======")
                                    || line.starts_with(">>>>>>>")
                                {
                                    widgets::change_color(ChangeKind::Conflicted)
                                } else {
                                    ui.visuals().text_color()
                                };
                                ui.label(
                                    RichText::new(line.replace('\t', "    "))
                                        .monospace()
                                        .color(color),
                                );
                            }
                        });
                }
                None => {
                    ui.weak(tr!("无法以文本方式读取该文件"));
                }
            }
            return;
        }

        let Some(diff) = self.change_diff.clone() else {
            return;
        };
        let path = selected.path.clone();
        let area = selected.area;
        let mut file_action: Option<Action> = None;
        let mut discard = false;
        self.diff_header(
            ui,
            &path,
            Some(&mut |ui: &mut Ui| match area {
                Area::Unstaged => {
                    if ui.small_button(tr!("暂存文件")).clicked() {
                        file_action = Some(Action::Stage(vec![path.clone()]));
                    }
                    if ui.small_button(tr!("放弃文件…")).clicked() {
                        discard = true;
                    }
                }
                Area::Staged => {
                    if ui.small_button(tr!("取消暂存文件")).clicked() {
                        file_action = Some(Action::Unstage(vec![path.clone()]));
                    }
                }
                Area::Conflicted => {}
            }),
        );
        if let Some(action) = file_action {
            self.actions.push(action);
        }
        if discard {
            self.dialog = Some(Dialog::Discard {
                changes: vec![FileChange {
                    path: selected.path.clone(),
                    old_path: selected.old_path.clone(),
                    kind: selected.kind,
                }],
            });
        }
        match diff {
            Ok(diff) => {
                let options = DiffViewOptions {
                    mode: match selected.area {
                        Area::Staged => DiffMode::Staged,
                        _ => DiffMode::Unstaged,
                    },
                    hunk_ops: !matches!(
                        selected.kind,
                        ChangeKind::Untracked
                            | ChangeKind::Deleted
                            | ChangeKind::Renamed
                            | ChangeKind::TypeChange
                    ),
                    wrap: self.wrap,
                    syntax: self.syntax,
                };
                if options.hunk_ops && options.mode != DiffMode::ReadOnly {
                    ui.weak(tr!(
                        "点击变更行可以选择，Shift 点击选择范围；然后使用区块标题上的按钮。"
                    ));
                }
                if let Some(request) =
                    diff_view::show(ui, &diff, &options, &mut self.diff_selection)
                {
                    self.actions.push(Action::Diff(request));
                }
            }
            Err(err) => {
                ui.colored_label(ui.visuals().error_fg_color, err);
            }
        }
    }

    fn commit_box(&mut self, ui: &mut Ui, data: &TabData) {
        ui.add_space(4.0);
        if data.identity.is_none() {
            ui.horizontal(|ui| {
                ui.colored_label(ui.visuals().warn_fg_color, tr!("未设置提交身份"));
                if ui.small_button(tr!("设置…")).clicked() {
                    self.dialog = Some(Dialog::Identity {
                        name: String::new(),
                        email: String::new(),
                    });
                }
            });
        }
        let icon_size = super::icons::INLINE + 8.0;
        let spacing = ui.spacing().item_spacing.x;
        let editor_width = (ui.available_width() - icon_size - spacing).max(80.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(editor_width);
                let editor = ui.add(
                    egui::TextEdit::multiline(&mut self.message)
                        .hint_text(tr!("提交说明，第一行是摘要"))
                        .desired_rows(4)
                        .desired_width(editor_width),
                );
                if std::mem::take(&mut self.focus_commit) {
                    editor.request_focus();
                }
                ui.horizontal(|ui| {
                    let can_amend = data.head.is_some();
                    let before = self.amend;
                    ui.add_enabled(
                        can_amend,
                        egui::Checkbox::new(&mut self.amend, tr!("修改上次提交")),
                    );
                    if self.amend
                        && !before
                        && self.message.trim().is_empty()
                        && let Some(head) = data.head
                        && let Ok(repo) = Repo::discover(&self.root)
                        && let Ok(details) = repo.commit_details(head)
                    {
                        self.message = details.message.trim().to_string();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let staged = data.tree.staged.len();
                        let conflicted = !data.tree.conflicted.is_empty();
                        let label = if self.amend {
                            tr!("修改提交")
                        } else {
                            tr!("提交")
                        };
                        // 只提交已暂存的文件；没有暂存任何文件时只有“修改上次提交”可用。
                        let subject_empty = split_message(&self.message).0.is_empty();
                        let ready = !subject_empty
                            && !conflicted
                            && (staged > 0 || self.amend)
                            && !self.is_busy();
                        let reason = if self.is_busy() {
                            tr!("已有任务在运行，请稍后再试")
                        } else if conflicted {
                            tr!("请先解决冲突")
                        } else if subject_empty {
                            tr!("请填写提交摘要")
                        } else if staged == 0 && !self.amend {
                            tr!("先暂存要提交的文件")
                        } else {
                            ""
                        };
                        let hover = if reason.is_empty() {
                            ui.ctx().format_shortcut(&egui::KeyboardShortcut::new(
                                egui::Modifiers::COMMAND,
                                Key::Enter,
                            ))
                        } else {
                            reason.to_string()
                        };
                        if ui
                            .add_enabled(
                                ready,
                                egui::Button::new(RichText::new(label).strong())
                                    .min_size(Vec2::new(110.0, 26.0)),
                            )
                            .on_hover_text(hover)
                            .clicked()
                        {
                            self.actions.push(Action::Commit);
                        }
                    });
                });
            });
            let history = super::icons::icon_button(ui, Icon::History, tr!("最近的提交说明"));
            let just_opened = history.clicked();
            egui::Popup::from_toggle_button_response(&history)
                .layout(Layout::top_down_justified(Align::Min))
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .show(|ui| self.message_menu(ui, just_opened));
        });
        if data.tree.staged.is_empty() && !self.amend {
            ui.weak(tr!("先暂存要提交的文件"));
        }
        ui.add_space(4.0);
    }

    fn message_menu(&mut self, ui: &mut Ui, just_opened: bool) {
        if self.message_index.is_none() {
            self.message_index = Some(load_commit_messages(&self.root));
        }
        // 弹层的宽高固定，不随筛选结果的多少变化。
        ui.set_width(MESSAGE_MENU_WIDTH);
        if just_opened {
            self.message_query.clear();
        }
        let search = ui.add(
            egui::TextEdit::singleline(&mut self.message_query)
                .hint_text(tr!("搜索说明、作者或 SHA"))
                .desired_width(f32::INFINITY),
        );
        if just_opened {
            search.request_focus();
        }
        if ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            ui.close();
            return;
        }
        let query = self.message_query.trim().to_lowercase();
        let mut chosen: Option<String> = None;
        let mut shown = 0usize;
        let mut more = false;
        egui::ScrollArea::vertical()
            .id_salt("message-menu")
            .auto_shrink([false, false])
            .max_height(MESSAGE_MENU_HEIGHT)
            .show(ui, |ui| {
                let index = self.message_index.clone().unwrap_or_default();
                let known: HashSet<String> =
                    index.iter().map(|commit| commit.summary.clone()).collect();
                for message in &self.recent_messages {
                    let first = message.lines().next().unwrap_or(message).trim();
                    if first.is_empty() || known.contains(first) {
                        continue;
                    }
                    if !query.is_empty() && !message.to_lowercase().contains(&query) {
                        continue;
                    }
                    if shown >= MESSAGE_PICK_LIMIT {
                        more = true;
                        break;
                    }
                    shown += 1;
                    if ui.button(first).clicked() {
                        chosen = Some(message.clone());
                    }
                }
                if !more {
                    for commit in &index {
                        let haystack =
                            format!("{} {} {}", commit.summary, commit.author, commit.id);
                        if !query.is_empty() && !haystack.to_lowercase().contains(&query) {
                            continue;
                        }
                        if shown >= MESSAGE_PICK_LIMIT {
                            more = true;
                            break;
                        }
                        shown += 1;
                        let label = if commit.author.is_empty() {
                            commit.summary.clone()
                        } else {
                            format!("{} · {}", commit.summary, commit.author)
                        };
                        if ui.button(label).clicked() {
                            chosen =
                                Some(commit_message_body(&self.root, &commit.id, &commit.summary));
                        }
                    }
                }
                if shown == 0 {
                    ui.weak(if query.is_empty() {
                        tr!("没有最近的提交说明")
                    } else {
                        tr!("没有匹配的提交")
                    });
                }
                if more {
                    ui.weak(tr!("还有更多结果，请继续输入关键字"));
                }
            });
        if let Some(message) = chosen {
            self.message = message;
            ui.close();
        }
    }

    pub fn request_commit(&mut self) {
        if self.view == View::LocalChanges {
            self.actions.push(Action::Commit);
        }
    }

    pub fn request_reload(&mut self) {
        self.actions.push(Action::Reload);
    }

    // ---- 浮动窗口 ----

    pub fn windows(&mut self, ctx: &egui::Context, recent_messages: &[String]) {
        self.recent_messages = recent_messages.to_vec();
        if let Some(mut dialog) = self.dialog.take() {
            let outcome = match &self.data {
                Some(data) => dialog.show(
                    ctx,
                    &DialogContext {
                        root: &self.root,
                        refs: &data.refs,
                        local_changes: data.tree.unstaged.len()
                            + data.tree.staged.len()
                            + data.tree.conflicted.len(),
                    },
                ),
                None => Outcome::Cancel,
            };
            match outcome {
                Outcome::Keep => self.dialog = Some(dialog),
                Outcome::Cancel => {}
                Outcome::Job(job) => self.push_job(job),
                Outcome::Discard(changes) => {
                    let result =
                        Repo::discover(&self.root).and_then(|repo| repo.discard_paths(&changes));
                    self.report(result);
                    self.actions.push(Action::Reload);
                }
            }
        }
        self.palette_window(ctx);
        self.blame_window(ctx);
        self.file_history_window(ctx);
        if self.show_log {
            let mut open = true;
            egui::Window::new(tr!("活动日志 — {name}", name = self.name))
                .open(&mut open)
                .default_size([640.0, 360.0])
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .stick_to_bottom(true)
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            for line in &self.log {
                                ui.label(RichText::new(line).monospace().size(11.5));
                            }
                            if let Some(job) = &self.job
                                && job.is_running()
                            {
                                for line in &job.log {
                                    ui.label(
                                        RichText::new(line.trim_start_matches('\r'))
                                            .monospace()
                                            .size(11.5),
                                    );
                                }
                            }
                        });
                });
            self.show_log = open;
        }
    }

    fn palette_window(&mut self, ctx: &egui::Context) {
        let Some(mut palette) = self.palette.take() else {
            return;
        };
        let Some(data) = self.data.take() else {
            return;
        };
        let entries = palette_entries(&palette, &data);
        let mut close = false;
        let mut chosen: Option<usize> = None;
        let modal = egui::Modal::new(egui::Id::unique("palette")).show(ctx, |ui| {
            ui.set_width(520.0);
            let hint = match palette.mode {
                PaletteMode::Checkout => tr!("输入分支名快速检出"),
                PaletteMode::Commands => tr!("输入命令"),
            };
            let response = ui.add(
                egui::TextEdit::singleline(&mut palette.query)
                    .hint_text(hint)
                    .desired_width(f32::INFINITY),
            );
            response.request_focus();
            if response.changed() {
                palette.selected = 0;
            }
            let count = entries.len();
            ui.input_mut(|input| {
                if input.consume_key(egui::Modifiers::NONE, Key::ArrowDown) && count > 0 {
                    palette.selected = (palette.selected + 1).min(count - 1);
                }
                if input.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                    palette.selected = palette.selected.saturating_sub(1);
                }
                if input.consume_key(egui::Modifiers::NONE, Key::Enter) && count > 0 {
                    chosen = Some(palette.selected);
                }
                if input.consume_key(egui::Modifiers::NONE, Key::Escape) {
                    close = true;
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .show(ui, |ui| {
                    for (index, (label, _)) in entries.iter().enumerate() {
                        let response = ui.selectable_label(index == palette.selected, label);
                        if index == palette.selected {
                            response.scroll_to_me(None);
                        }
                        if response.clicked() {
                            chosen = Some(index);
                        }
                    }
                    if entries.is_empty() {
                        ui.weak(tr!("没有匹配项"));
                    }
                });
        });
        if modal.should_close() {
            close = true;
        }
        self.data = Some(data);
        if let Some(index) = chosen {
            let command = entries[index].1.clone();
            self.run_palette(command);
            return;
        }
        if !close {
            self.palette = Some(palette);
        }
    }

    fn run_palette(&mut self, command: PaletteCommand) {
        match command {
            PaletteCommand::CheckoutLocal(name) => self.checkout_branch(&name),
            PaletteCommand::CheckoutRemote(name) => {
                if let Some(data) = self.data.take() {
                    self.checkout_remote(&data, &name);
                    self.data = Some(data);
                }
            }
            PaletteCommand::Fetch => self.open_fetch(),
            PaletteCommand::Pull => self.open_pull(),
            PaletteCommand::Push => self.open_push(),
            PaletteCommand::Stash => self.open_stash(),
            PaletteCommand::PopStash => self.pop_stash(),
            PaletteCommand::CreateBranch => self.open_create_branch(),
            PaletteCommand::CreateTag => self.open_create_tag(),
            PaletteCommand::AddRemote => self.add_remote(),
            PaletteCommand::Terminal => self.open_terminal(),
            PaletteCommand::Reveal => self.reveal_in_file_manager(),
            PaletteCommand::Refresh => self.request_reload(),
            PaletteCommand::ShowChanges => self.view = View::LocalChanges,
            PaletteCommand::ShowHistory => self.view = View::History,
            PaletteCommand::ShowHead => self.show_head(),
            PaletteCommand::QuickCheckout => self.open_quick_checkout(),
            PaletteCommand::GarbageCollect => {
                self.push_job(GitJob::new(tr!("整理仓库"), &self.root).step(["gc", "--prune=now"]))
            }
            PaletteCommand::BisectStart => {
                if let Some(Ok(details)) = &self.details {
                    let bad = details.id.to_string();
                    self.push_job(
                        GitJob::new(tr!("开始二分查找"), &self.root)
                            .step(["bisect", "start"])
                            .step(["bisect", "bad", bad.as_str()]),
                    );
                }
            }
        }
    }

    fn blame_window(&mut self, ctx: &egui::Context) {
        let Some(blame) = &self.blame else { return };
        let mut open = true;
        let mut reveal = None;
        egui::Window::new(tr!("追溯 — {path}", path = blame.path))
            .open(&mut open)
            .default_size([820.0, 520.0])
            .show(ctx, |ui| match &blame.lines {
                Err(err) => {
                    ui.colored_label(ui.visuals().error_fg_color, err);
                }
                Ok(lines) => {
                    egui::ScrollArea::both().auto_shrink(false).show_rows(
                        ui,
                        18.0,
                        lines.len(),
                        |ui, range| {
                            for index in range {
                                let line = &lines[index];
                                let first = index == 0 || lines[index - 1].commit != line.commit;
                                ui.horizontal(|ui| {
                                    let info = if first {
                                        format!(
                                            "{:<8} {:<14} {}",
                                            line.commit
                                                .map(|id| git_forker::short_id(&id))
                                                .unwrap_or_default(),
                                            truncate(&line.author, 14),
                                            widgets::format_time(line.time).get(..10).unwrap_or("")
                                        )
                                    } else {
                                        String::new()
                                    };
                                    let response = ui.add_sized(
                                        [260.0, 16.0],
                                        egui::Label::new(
                                            RichText::new(info)
                                                .monospace()
                                                .size(11.5)
                                                .color(ui.visuals().weak_text_color()),
                                        )
                                        .sense(Sense::click()),
                                    );
                                    if response.clicked()
                                        && let Some(id) = line.commit
                                    {
                                        reveal = Some(id);
                                    }
                                    ui.label(
                                        RichText::new(format!("{:>5} ", line.line_no))
                                            .monospace()
                                            .color(ui.visuals().weak_text_color()),
                                    );
                                    ui.label(
                                        RichText::new(line.text.replace('\t', "    ")).monospace(),
                                    );
                                });
                            }
                        },
                    );
                }
            });
        if let Some(id) = reveal {
            self.actions.push(Action::RevealCommit(id));
        }
        if !open {
            self.blame = None;
        }
    }

    fn file_history_window(&mut self, ctx: &egui::Context) {
        let Some(history) = &self.file_history else {
            return;
        };
        let mut open = true;
        let mut reveal = None;
        egui::Window::new(tr!("文件历史 — {path}", path = history.path))
            .open(&mut open)
            .default_size([640.0, 420.0])
            .show(ctx, |ui| match &history.entries {
                Err(err) => {
                    ui.colored_label(ui.visuals().error_fg_color, err);
                }
                Ok(entries) => {
                    if entries.is_empty() {
                        ui.weak(tr!("没有找到修改该文件的提交"));
                    }
                    egui::ScrollArea::vertical().auto_shrink(false).show_rows(
                        ui,
                        20.0,
                        entries.len(),
                        |ui, range| {
                            for entry in &entries[range] {
                                let selected = self.selected_commit == Some(entry.id);
                                let response = ui.selectable_label(
                                    selected,
                                    format!(
                                        "{}  {}  —  {}  {}",
                                        git_forker::short_id(&entry.id),
                                        entry.summary,
                                        entry.author,
                                        widgets::format_time(entry.time)
                                    ),
                                );
                                if response.clicked() {
                                    reveal = Some(entry.id);
                                }
                            }
                        },
                    );
                }
            });
        if let Some(id) = reveal {
            self.details_file = Some(history.path.clone());
            self.actions.push(Action::RevealCommit(id));
        }
        if !open {
            self.file_history = None;
        }
    }
}

#[derive(Clone)]
enum PaletteCommand {
    CheckoutLocal(String),
    CheckoutRemote(String),
    Fetch,
    Pull,
    Push,
    Stash,
    PopStash,
    CreateBranch,
    CreateTag,
    AddRemote,
    Terminal,
    Reveal,
    Refresh,
    ShowChanges,
    ShowHistory,
    ShowHead,
    QuickCheckout,
    GarbageCollect,
    BisectStart,
}

fn palette_entries(palette: &Palette, data: &TabData) -> Vec<(String, PaletteCommand)> {
    let query = palette.query.to_lowercase();
    let entries: Vec<(String, PaletteCommand)> = match palette.mode {
        PaletteMode::Checkout => data
            .refs
            .local
            .iter()
            .filter(|branch| !branch.is_head)
            .map(|branch| {
                (
                    Icon::Branch.with(&branch.name),
                    PaletteCommand::CheckoutLocal(branch.name.clone()),
                )
            })
            .chain(data.refs.remotes.iter().flat_map(|remote| {
                remote.branches.iter().map(|branch| {
                    (
                        Icon::Remote.with(&branch.name),
                        PaletteCommand::CheckoutRemote(branch.name.clone()),
                    )
                })
            }))
            .collect(),
        PaletteMode::Commands => vec![
            (tr!("抓取…").into(), PaletteCommand::Fetch),
            (tr!("拉取…").into(), PaletteCommand::Pull),
            (tr!("推送…").into(), PaletteCommand::Push),
            (tr!("贮藏更改…").into(), PaletteCommand::Stash),
            (tr!("弹出贮藏…").into(), PaletteCommand::PopStash),
            (tr!("创建分支…").into(), PaletteCommand::CreateBranch),
            (tr!("创建标签…").into(), PaletteCommand::CreateTag),
            (tr!("快速检出…").into(), PaletteCommand::QuickCheckout),
            (tr!("添加远程仓库…").into(), PaletteCommand::AddRemote),
            (tr!("打开终端").into(), PaletteCommand::Terminal),
            (tr!("在文件管理器中显示").into(), PaletteCommand::Reveal),
            (tr!("刷新").into(), PaletteCommand::Refresh),
            (tr!("显示本地更改").into(), PaletteCommand::ShowChanges),
            (tr!("显示全部提交").into(), PaletteCommand::ShowHistory),
            (tr!("定位到 HEAD").into(), PaletteCommand::ShowHead),
            (
                tr!("从所选提交开始二分查找（标记为坏）").into(),
                PaletteCommand::BisectStart,
            ),
            (
                tr!("整理仓库 (git gc)").into(),
                PaletteCommand::GarbageCollect,
            ),
        ],
    };
    entries
        .into_iter()
        .filter(|(label, _)| fuzzy_match(&label.to_lowercase(), &query))
        .collect()
}

/// 查询中的字符按顺序出现在文本里即算匹配。
fn fuzzy_match(text: &str, query: &str) -> bool {
    let mut chars = text.chars();
    query
        .chars()
        .filter(|c| !c.is_whitespace())
        .all(|wanted| chars.any(|c| c == wanted))
}

fn changes_layout(ui: &Ui) -> ChangesLayout {
    ui.data_mut(|data| {
        data.get_persisted(egui::Id::unique("changes-layout"))
            .unwrap_or(ChangesLayout::List)
    })
}

fn set_changes_layout(ui: &Ui, layout: ChangesLayout) {
    ui.data_mut(|data| data.insert_persisted(egui::Id::unique("changes-layout"), layout));
}

fn changes_split_ratio(ui: &Ui) -> f32 {
    ui.data_mut(|data| {
        data.get_persisted(egui::Id::unique("changes-split"))
            .unwrap_or(0.55)
    })
}

/// 把可用高度分成未暂存、分割条、已暂存三段。比例超出范围时仍保证两侧都留有最小高度。
fn changes_split_rects(available: Rect, ratio: f32) -> (Rect, Rect, Rect) {
    let handle_h = CHANGES_SPLIT_HANDLE.min(available.height());
    let usable = (available.height() - handle_h).max(0.0);
    let top_h = if usable <= CHANGES_SECTION_MIN * 2.0 {
        usable * 0.5
    } else {
        (usable * ratio).clamp(CHANGES_SECTION_MIN, usable - CHANGES_SECTION_MIN)
    };
    let top = Rect::from_min_size(available.min, Vec2::new(available.width(), top_h));
    let handle = Rect::from_min_size(
        Pos2::new(available.left(), top.bottom()),
        Vec2::new(available.width(), handle_h),
    );
    let bottom = Rect::from_min_max(Pos2::new(available.left(), handle.bottom()), available.max);
    (top, handle, bottom)
}

fn paint_changes_split(ui: &Ui, available: Rect, handle: Rect) {
    let response = ui
        .interact(
            handle,
            ui.scope_id().with("changes-split-handle"),
            egui::Sense::DRAG,
        )
        .on_hover_and_drag_cursor(egui::CursorIcon::ResizeVertical);
    let hot = response.hovered() || response.dragged();
    if hot {
        ui.painter()
            .rect_filled(handle, 0.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let color = if hot {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().widgets.noninteractive.bg_stroke.color
    };
    ui.painter().hline(
        handle.x_range(),
        handle.center().y,
        egui::Stroke::new(if hot { 2.0 } else { 1.0 }, color),
    );
    if response.dragged() {
        let usable = (available.height() - handle.height()).max(1.0);
        let min_ratio = (CHANGES_SECTION_MIN / usable).min(0.5);
        let ratio = (changes_split_ratio(ui) + response.drag_delta().y / usable)
            .clamp(min_ratio, 1.0 - min_ratio);
        ui.data_mut(|data| data.insert_persisted(egui::Id::unique("changes-split"), ratio));
    }
}

fn listed_changes<'a>(
    first: &'a [FileChange],
    first_area: Area,
    second: &'a [FileChange],
    second_area: Area,
    keep: &impl Fn(&&FileChange) -> bool,
) -> Vec<ListedChange<'a>> {
    first
        .iter()
        .filter(keep)
        .map(|change| ListedChange {
            change,
            area: first_area,
        })
        .chain(second.iter().filter(keep).map(|change| ListedChange {
            change,
            area: second_area,
        }))
        .collect()
}

fn file_row(ui: &mut Ui, change: &FileChange, selected: bool, discardable: bool) -> ChangeRow {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 20.0), Sense::click());
    let visuals = ui.visuals();
    if selected {
        ui.painter()
            .rect_filled(rect, 2.0, visuals.selection.bg_fill);
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, 2.0, visuals.widgets.hovered.weak_bg_fill);
    }
    let show_discard = discardable && response.hovered();
    let text_rect = row_text_rect(rect, show_discard);
    let badge = Rect::from_min_size(rect.min + Vec2::new(2.0, 0.0), Vec2::new(18.0, 20.0));
    widgets::paint_change_badge(ui, badge, change.kind);
    let (dir, file) = match change.path.rsplit_once('/') {
        Some((dir, file)) => (Some(dir), file),
        None => (None, change.path.as_str()),
    };
    let text_color = if selected {
        visuals.selection.stroke.color
    } else {
        visuals.text_color()
    };
    let painter = ui.painter().with_clip_rect(text_rect);
    let galley = painter.layout_no_wrap(file.to_string(), FontId::proportional(13.0), text_color);
    let name_pos = Pos2::new(badge.right() + 6.0, rect.center().y - galley.size().y / 2.0);
    let name_right = name_pos.x + galley.size().x;
    painter.galley(name_pos, galley, text_color);
    if let Some(dir) = dir {
        painter.text(
            Pos2::new(name_right + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            dir,
            FontId::proportional(12.0),
            visuals.weak_text_color(),
        );
    }
    ChangeRow {
        over_discard: paint_discard_button(ui, rect, &response, show_discard, selected),
        response,
    }
}

/// 合并列表：整条相对路径画在一行里。
fn combined_file_row(
    ui: &mut Ui,
    change: &FileChange,
    selected: bool,
    discardable: bool,
) -> ChangeRow {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 20.0), Sense::click());
    paint_change_row_background(ui, rect, &response, selected);
    let show_discard = discardable && response.hovered();
    let badge = Rect::from_min_size(rect.min + Vec2::new(2.0, 0.0), Vec2::new(18.0, 20.0));
    widgets::paint_change_badge(ui, badge, change.kind);
    paint_change_label(
        ui,
        row_text_rect(rect, show_discard),
        badge.right() + 6.0,
        &change.path,
        change_row_text_color(ui, selected),
    );
    ChangeRow {
        over_discard: paint_discard_button(ui, rect, &response, show_discard, selected),
        response,
    }
}

fn change_folder_row(ui: &mut Ui, name: &str, depth: usize, open: bool) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, CHANGES_TREE_ROW), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 2.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let weak = ui.visuals().weak_text_color();
    let mut x = rect.left() + 4.0 + depth as f32 * CHANGES_TREE_INDENT;
    let y = rect.center().y;
    let chevron = if open {
        Icon::ChevronDown
    } else {
        Icon::ChevronRight
    };
    icons::paint(
        ui.painter(),
        Pos2::new(x + icons::SMALL / 2.0, y),
        icons::SMALL,
        chevron,
        weak,
    );
    x += 14.0;
    icons::paint_folder(ui.painter(), Pos2::new(x + 7.0, y), open);
    paint_change_label(ui, rect, x + 18.0, name, ui.visuals().text_color());
    response
}

fn tree_file_row(
    ui: &mut Ui,
    kind: ChangeKind,
    name: &str,
    depth: usize,
    selected: bool,
    discardable: bool,
) -> ChangeRow {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, CHANGES_TREE_ROW), Sense::click());
    paint_change_row_background(ui, rect, &response, selected);
    let show_discard = discardable && response.hovered();
    // 文件徽标和同级目录图标对齐，前面留出箭头的位置。
    let x = rect.left() + 4.0 + depth as f32 * CHANGES_TREE_INDENT + 14.0;
    let badge = Rect::from_min_size(Pos2::new(x, rect.center().y - 10.0), Vec2::new(18.0, 20.0));
    widgets::paint_change_badge(ui, badge, kind);
    paint_change_label(
        ui,
        row_text_rect(rect, show_discard),
        badge.right() + 6.0,
        name,
        change_row_text_color(ui, selected),
    );
    ChangeRow {
        over_discard: paint_discard_button(ui, rect, &response, show_discard, selected),
        response,
    }
}

fn row_text_rect(rect: Rect, show_discard: bool) -> Rect {
    if show_discard {
        rect.with_max_x(rect.right() - DISCARD_SLOT)
    } else {
        rect
    }
}

/// 未暂存行右侧的还原按钮。悬停时出现，放弃改动；未跟踪文件则删除。
fn paint_discard_button(
    ui: &Ui,
    rect: Rect,
    response: &egui::Response,
    show: bool,
    selected: bool,
) -> bool {
    if !show {
        return false;
    }
    let width = DISCARD_SLOT.min(rect.width());
    let slot = Rect::from_min_size(
        Pos2::new(rect.right() - width, rect.top()),
        Vec2::new(width, rect.height()),
    );
    let over = response.hover_pos().is_some_and(|pos| slot.contains(pos));
    let color = if selected {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().text_color()
    };
    icons::paint(ui.painter(), slot.center(), 13.0, Icon::Revert, color);
    over
}

fn discard_tip(kind: ChangeKind) -> &'static str {
    if kind == ChangeKind::Untracked {
        tr!("删除此文件")
    } else {
        tr!("放弃此文件的更改")
    }
}

fn paint_change_row_background(ui: &Ui, rect: Rect, response: &egui::Response, selected: bool) {
    let visuals = ui.visuals();
    if selected {
        ui.painter()
            .rect_filled(rect, 2.0, visuals.selection.bg_fill);
    } else if response.hovered() {
        ui.painter()
            .rect_filled(rect, 2.0, visuals.widgets.hovered.weak_bg_fill);
    }
}

fn change_row_text_color(ui: &Ui, selected: bool) -> Color32 {
    if selected {
        ui.visuals().selection.stroke.color
    } else {
        ui.visuals().text_color()
    }
}

fn paint_change_label(ui: &Ui, rect: Rect, x: f32, text: &str, color: Color32) {
    let painter = ui.painter().with_clip_rect(rect);
    let galley = painter.layout_no_wrap(text.to_string(), FontId::proportional(13.0), color);
    painter.galley(
        Pos2::new(x, rect.center().y - galley.size().y / 2.0),
        galley,
        color,
    );
}

/// 贮藏栈顶的提交 ID；没有贮藏时为 `None`。
fn stash_top(root: &Path) -> Option<String> {
    git_output(root, &["rev-parse", "-q", "--verify", "refs/stash"])
        .ok()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
}

/// 贮藏后重新应用发生冲突时，git 不会丢掉这个贮藏。
/// 应用成功时贮藏会消失，栈顶回到任务开始前的样子。
fn autostash_conflicted(root: &Path, before: &Option<String>) -> bool {
    let after = stash_top(root);
    if after.is_none() || after == *before {
        return false;
    }
    git_output(root, &["stash", "list", "-1", "--format=%gs"]).is_ok_and(|subject| {
        let subject = subject.trim();
        subject == "autostash" || subject.contains(PULL_STASH_MESSAGE)
    })
}

#[derive(Clone)]
struct CommitMessageRef {
    id: String,
    author: String,
    summary: String,
}

fn load_commit_messages(root: &Path) -> Vec<CommitMessageRef> {
    let output = git_output(
        root,
        &[
            "log",
            "--all",
            "-n",
            MESSAGE_INDEX_LIMIT,
            "--format=%h%x1f%an%x1f%s",
        ],
    );
    output
        .map(|text| parse_commit_messages(&text))
        .unwrap_or_default()
}

fn parse_commit_messages(output: &str) -> Vec<CommitMessageRef> {
    output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\u{1f}');
            let id = parts.next()?.trim();
            let author = parts.next()?.trim();
            let summary = parts.next()?.trim();
            if id.is_empty() || summary.is_empty() {
                return None;
            }
            Some(CommitMessageRef {
                id: id.to_string(),
                author: author.to_string(),
                summary: summary.to_string(),
            })
        })
        .collect()
}

fn commit_message_body(root: &Path, id: &str, summary: &str) -> String {
    git_output(root, &["log", "-1", "--format=%B", id])
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| summary.to_string())
}

fn split_message(message: &str) -> (String, String) {
    let message = message.trim();
    match message.split_once('\n') {
        Some((subject, rest)) => (subject.trim().to_string(), rest.trim().to_string()),
        None => (message.to_string(), String::new()),
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        text.chars().take(max - 1).collect::<String>() + "…"
    }
}

fn file_history(root: &Path, path: &str) -> Result<Vec<FileHistoryEntry>, String> {
    let output = git_output(
        root,
        &[
            "log",
            "--follow",
            "-n",
            FILE_HISTORY_LIMIT,
            "--format=%H%x1f%an%x1f%at%x1f%s",
            "--",
            path,
        ],
    )?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split('\x1f');
            let id = Oid::from_str(parts.next()?).ok()?;
            let author = parts.next()?.to_string();
            let time = parts.next()?.parse().ok()?;
            let summary = parts.next().unwrap_or_default().to_string();
            Some(FileHistoryEntry {
                id,
                author,
                time,
                summary,
            })
        })
        .collect())
}

fn append_gitignore(root: &Path, path: &str) -> std::io::Result<()> {
    use std::io::Write;
    let file = root.join(".gitignore");
    let existing = std::fs::read_to_string(&file).unwrap_or_default();
    let entry = format!("/{}", path.trim_start_matches('/'));
    if existing.lines().any(|line| line.trim() == entry) {
        return Ok(());
    }
    let mut handle = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)?;
    if !existing.is_empty() && !existing.ends_with('\n') {
        writeln!(handle)?;
    }
    writeln!(handle, "{entry}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_keeps_both_change_sections_usable() {
        let available = Rect::from_min_size(Pos2::ZERO, Vec2::new(300.0, 400.0));
        let (top, handle, bottom) = changes_split_rects(available, 0.55);
        let usable = 400.0 - CHANGES_SPLIT_HANDLE;
        assert!((top.height() - usable * 0.55).abs() < 0.1);
        assert_eq!(handle.top(), top.bottom());
        assert_eq!(bottom.top(), handle.bottom());
        assert!((bottom.bottom() - available.bottom()).abs() < 0.1);
        assert!(top.height() >= CHANGES_SECTION_MIN);
        assert!(bottom.height() >= CHANGES_SECTION_MIN);

        let (top, _, bottom) = changes_split_rects(available, 0.99);
        assert!((top.height() - (usable - CHANGES_SECTION_MIN)).abs() < 0.1);
        assert!((bottom.height() - CHANGES_SECTION_MIN).abs() < 0.1);
    }
}
