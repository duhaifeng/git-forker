use std::collections::HashMap;

pub use git2::Oid;

/// 当前 HEAD 的位置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeadState {
    Branch(String),
    Detached(String),
    Unborn,
}

/// 仓库正处于的多步操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoOperation {
    None,
    Merge,
    Rebase,
    CherryPick,
    Revert,
    Bisect,
    ApplyMailbox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    TypeChange,
    Untracked,
    Conflicted,
}

impl ChangeKind {
    /// 与 `git status --short` 一致的单字母标记。
    pub fn letter(self) -> char {
        match self {
            ChangeKind::Added => 'A',
            ChangeKind::Modified => 'M',
            ChangeKind::Deleted => 'D',
            ChangeKind::Renamed => 'R',
            ChangeKind::TypeChange => 'T',
            ChangeKind::Untracked => '?',
            ChangeKind::Conflicted => 'U',
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub kind: ChangeKind,
}

/// 工作区、暂存区和冲突文件。
#[derive(Debug, Clone)]
pub struct WorkingTree {
    pub head: HeadState,
    pub operation: RepoOperation,
    pub unstaged: Vec<FileChange>,
    pub staged: Vec<FileChange>,
    pub conflicted: Vec<FileChange>,
}

#[derive(Debug, Clone)]
pub struct LocalBranch {
    pub name: String,
    pub target: Oid,
    pub is_head: bool,
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
}

#[derive(Debug, Clone)]
pub struct RemoteBranch {
    /// 例如 `origin/main`。
    pub name: String,
    /// 去掉远程名后的分支名，例如 `main`。
    pub branch: String,
    pub target: Oid,
}

#[derive(Debug, Clone)]
pub struct RemoteInfo {
    pub name: String,
    pub url: String,
    pub branches: Vec<RemoteBranch>,
}

#[derive(Debug, Clone)]
pub struct TagInfo {
    pub name: String,
    pub target: Option<Oid>,
    pub annotated: bool,
}

#[derive(Debug, Clone)]
pub struct StashInfo {
    pub index: usize,
    pub message: String,
    pub id: Oid,
}

#[derive(Debug, Clone)]
pub struct SubmoduleInfo {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct RefsSnapshot {
    pub local: Vec<LocalBranch>,
    pub remotes: Vec<RemoteInfo>,
    pub tags: Vec<TagInfo>,
    pub stashes: Vec<StashInfo>,
    pub submodules: Vec<SubmoduleInfo>,
}

impl RefsSnapshot {
    pub fn remote_names(&self) -> Vec<String> {
        self.remotes
            .iter()
            .map(|remote| remote.name.clone())
            .collect()
    }

    pub fn current_branch(&self) -> Option<&LocalBranch> {
        self.local.iter().find(|branch| branch.is_head)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Head,
    LocalBranch,
    RemoteBranch,
    Tag,
    Stash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefLabel {
    pub name: String,
    pub kind: RefKind,
}

#[derive(Debug, Clone)]
pub struct CommitRow {
    pub id: Oid,
    pub short: String,
    pub summary: String,
    pub author: String,
    pub email: String,
    pub time: i64,
    pub parents: Vec<Oid>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryOptions {
    pub limit: usize,
    pub first_parent: bool,
    pub current_branch_only: bool,
    pub show_remotes: bool,
    pub show_tags: bool,
}

impl Default for HistoryOptions {
    fn default() -> Self {
        Self {
            limit: 5000,
            first_parent: false,
            current_branch_only: false,
            show_remotes: true,
            show_tags: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// 从行顶部的某列连到本行节点。
    TopToNode,
    /// 从本行节点连到行底部的某列。
    NodeToBottom,
    /// 不经过节点，从顶部某列直通到底部同一列。
    Through,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphEdge {
    pub from: usize,
    pub to: usize,
    pub kind: EdgeKind,
    pub color: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphRow {
    pub node: usize,
    pub color: usize,
    pub width: usize,
    pub edges: Vec<GraphEdge>,
}

#[derive(Debug, Clone)]
pub struct History {
    pub commits: Vec<CommitRow>,
    pub graph: Vec<GraphRow>,
    pub labels: HashMap<Oid, Vec<RefLabel>>,
    pub truncated: bool,
}

#[derive(Debug, Clone)]
pub struct Person {
    pub name: String,
    pub email: String,
    pub time: i64,
}

#[derive(Debug, Clone)]
pub struct CommitDetails {
    pub id: Oid,
    pub summary: String,
    pub message: String,
    pub author: Person,
    pub committer: Person,
    pub parents: Vec<Oid>,
    pub files: Vec<FileChange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    pub old_no: Option<u32>,
    pub new_no: Option<u32>,
    /// 行内容，保留原始换行符；缺少换行表示文件末尾没有换行。
    pub raw: String,
}

impl DiffLine {
    pub fn text(&self) -> &str {
        self.raw.strip_suffix('\n').unwrap_or(&self.raw)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub header: String,
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileDiff {
    pub path: String,
    pub old_path: Option<String>,
    pub binary: bool,
    pub hunks: Vec<Hunk>,
    /// 超出显示上限后省略的行数。
    pub omitted_lines: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffSettings {
    pub context_lines: u32,
    pub ignore_whitespace: bool,
}

impl Default for DiffSettings {
    fn default() -> Self {
        Self {
            context_lines: 3,
            ignore_whitespace: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobContent {
    Text(String),
    Binary { size: usize },
    TooLarge { size: usize },
}

#[derive(Debug, Clone)]
pub struct BlameLine {
    pub line_no: usize,
    pub text: String,
    pub commit: Option<Oid>,
    pub author: String,
    pub time: i64,
}
