use std::collections::HashMap;
use std::path::{Path, PathBuf};

use git2::{
    ApplyLocation, BlameOptions, BranchType, Delta, Diff, DiffFindOptions, DiffOptions, ErrorCode,
    IndexAddOption, ObjectType, Patch, Repository, RepositoryState, Sort, Status, StatusOptions,
    build::CheckoutBuilder,
};

use super::graph;
use super::model::*;

/// 单个文件差异最多显示的行数；超出部分只计数。
const MAX_DIFF_LINES: usize = 50_000;
/// 文件树页最多列出的文件数。
const MAX_TREE_FILES: usize = 100_000;
/// 文件树页最多显示的文件大小。
const MAX_BLOB_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Git(#[from] git2::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{}", crate::tr!("没有可提交的更改"))]
    NothingToCommit,
    #[error("{}", crate::tr!("提交说明不能为空"))]
    EmptyMessage,
    #[error("{}", crate::tr!("请指定要暂存的路径"))]
    NoPaths,
    #[error("{}", crate::tr!("仓库没有工作目录"))]
    NoWorkdir,
    #[error("{}", crate::tr!("路径不在仓库内: {path}", path = .0))]
    OutsideWorkdir(String),
    #[error("{}", crate::tr!("当前仓库还没有提交"))]
    NoHead,
    #[error("{}", crate::tr!("找不到分支 {branch}", branch = .0))]
    MissingBranch(String),
    #[error("{}", crate::tr!("分支 {branch} 已存在", branch = .0))]
    BranchExists(String),
    #[error("{}", crate::tr!("未配置提交身份。请设置 user.name 和 user.email"))]
    MissingIdentity,
    #[error("{}", crate::tr!("所选内容没有可应用的更改"))]
    EmptyPatch,
}

/// 状态查询结果，供命令行使用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    pub code: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusReport {
    pub head: HeadState,
    pub entries: Vec<StatusEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSummary {
    pub id: String,
    pub summary: String,
    pub author: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchInfo {
    pub name: String,
    pub current: bool,
}

/// 打开后的 Git 仓库。读取在进程内完成；会运行钩子或访问网络的写操作交给系统 git。
pub struct Repo {
    inner: Repository,
}

impl Repo {
    /// 打开 `path` 所在的仓库。`path` 可以是工作目录里面的子路径。
    pub fn discover(path: impl AsRef<Path>) -> Result<Self, Error> {
        Ok(Self {
            inner: Repository::discover(path)?,
        })
    }

    pub fn init(path: impl AsRef<Path>) -> Result<Self, Error> {
        Ok(Self {
            inner: Repository::init(path)?,
        })
    }

    /// 工作目录；裸仓库返回 `.git` 目录。
    pub fn root(&self) -> PathBuf {
        self.inner
            .workdir()
            .unwrap_or_else(|| self.inner.path())
            .to_path_buf()
    }

    pub fn identity(&self) -> Option<(String, String)> {
        let signature = self.inner.signature().ok()?;
        Some((
            lossy(signature.name_bytes()),
            lossy(signature.email_bytes()),
        ))
    }

    pub fn head_state(&self) -> Result<HeadState, Error> {
        match self.inner.head() {
            Ok(head) if head.is_branch() => Ok(HeadState::Branch(
                head.shorthand().unwrap_or("HEAD").to_string(),
            )),
            Ok(head) => {
                let short = head
                    .peel_to_commit()
                    .ok()
                    .map(|commit| short_id(&commit.id()))
                    .unwrap_or_else(|| "HEAD".to_string());
                Ok(HeadState::Detached(short))
            }
            Err(err) if err.code() == ErrorCode::UnbornBranch => Ok(HeadState::Unborn),
            Err(err) => Err(err.into()),
        }
    }

    pub fn head_id(&self) -> Option<Oid> {
        self.inner
            .head()
            .ok()?
            .peel_to_commit()
            .ok()
            .map(|c| c.id())
    }

    pub fn operation(&self) -> RepoOperation {
        match self.inner.state() {
            RepositoryState::Clean => RepoOperation::None,
            RepositoryState::Merge => RepoOperation::Merge,
            RepositoryState::Revert | RepositoryState::RevertSequence => RepoOperation::Revert,
            RepositoryState::CherryPick | RepositoryState::CherryPickSequence => {
                RepoOperation::CherryPick
            }
            RepositoryState::Bisect => RepoOperation::Bisect,
            RepositoryState::Rebase
            | RepositoryState::RebaseInteractive
            | RepositoryState::RebaseMerge => RepoOperation::Rebase,
            RepositoryState::ApplyMailbox | RepositoryState::ApplyMailboxOrRebase => {
                RepoOperation::ApplyMailbox
            }
        }
    }

    // ---- 工作区 ----

    pub fn working_tree(&self) -> Result<WorkingTree, Error> {
        let mut options = StatusOptions::new();
        options
            .include_untracked(true)
            .recurse_untracked_dirs(true)
            .renames_head_to_index(true)
            .renames_index_to_workdir(true);
        let statuses = self.inner.statuses(Some(&mut options))?;
        let mut tree = WorkingTree {
            head: self.head_state()?,
            operation: self.operation(),
            unstaged: Vec::new(),
            staged: Vec::new(),
            conflicted: Vec::new(),
        };
        for entry in statuses.iter() {
            let status = entry.status();
            let path = lossy(entry.path_bytes());
            if status.is_conflicted() {
                tree.conflicted.push(FileChange {
                    path,
                    old_path: None,
                    kind: ChangeKind::Conflicted,
                });
                continue;
            }
            // 重命名时 `entry.path()` 是旧路径，文件的身份应当取差异里的新路径。
            let change = |delta: Option<git2::DiffDelta<'_>>, kind: ChangeKind| {
                let new_path = delta
                    .as_ref()
                    .and_then(|delta| delta.new_file().path().map(path_string))
                    .unwrap_or_else(|| path.clone());
                let old_path = delta
                    .and_then(|delta| delta.old_file().path().map(path_string))
                    .filter(|old| *old != new_path && kind == ChangeKind::Renamed);
                FileChange {
                    path: new_path,
                    old_path,
                    kind,
                }
            };
            if let Some(kind) = index_kind(status) {
                tree.staged.push(change(entry.head_to_index(), kind));
            }
            if let Some(kind) = worktree_kind(status) {
                tree.unstaged.push(change(entry.index_to_workdir(), kind));
            }
        }
        Ok(tree)
    }

    /// 暂存区或工作区中一个文件的差异。
    pub fn worktree_diff(
        &self,
        path: &str,
        staged: bool,
        settings: DiffSettings,
    ) -> Result<FileDiff, Error> {
        let mut options = diff_options(settings);
        options.pathspec(path).disable_pathspec_match(true);
        let diff = if staged {
            let head_tree = match self.inner.head() {
                Ok(head) => Some(head.peel_to_tree()?),
                Err(_) => None,
            };
            self.inner
                .diff_tree_to_index(head_tree.as_ref(), None, Some(&mut options))?
        } else {
            options
                .include_untracked(true)
                .recurse_untracked_dirs(true)
                .show_untracked_content(true);
            self.inner.diff_index_to_workdir(None, Some(&mut options))?
        };
        file_diff(&diff, path)
    }

    pub fn stage_paths(&self, paths: &[String]) -> Result<(), Error> {
        let workdir = self.inner.workdir().ok_or(Error::NoWorkdir)?;
        let mut index = self.inner.index()?;
        for path in paths {
            if workdir.join(path).symlink_metadata().is_ok() {
                index.add_path(Path::new(path))?;
            } else {
                index.remove_path(Path::new(path))?;
            }
        }
        index.write()?;
        Ok(())
    }

    pub fn stage_all(&self) -> Result<(), Error> {
        let mut index = self.inner.index()?;
        index.add_all(["*"], IndexAddOption::DEFAULT, None)?;
        index.update_all(["*"], None)?;
        index.write()?;
        Ok(())
    }

    pub fn unstage_paths(&self, paths: &[String]) -> Result<(), Error> {
        match self.inner.head() {
            Ok(head) => {
                let commit = head.peel_to_commit()?;
                self.inner.reset_default(Some(commit.as_object()), paths)?;
            }
            Err(err) if err.code() == ErrorCode::UnbornBranch => {
                let mut index = self.inner.index()?;
                for path in paths {
                    index.remove_path(Path::new(path))?;
                }
                index.write()?;
            }
            Err(err) => return Err(err.into()),
        }
        Ok(())
    }

    /// 把工作区文件恢复为暂存区内容。未跟踪文件直接删除。
    /// 未暂存的重命名会恢复旧路径，并删掉新路径。
    pub fn discard_paths(&self, changes: &[FileChange]) -> Result<(), Error> {
        let workdir = self.inner.workdir().ok_or(Error::NoWorkdir)?;
        let mut restore = Vec::new();
        let mut remove = Vec::new();
        for change in changes {
            match change.kind {
                ChangeKind::Untracked => remove.push(change.path.clone()),
                ChangeKind::Renamed => match change.old_path.as_deref() {
                    Some(old) if old != change.path => {
                        restore.push(old.to_string());
                        remove.push(change.path.clone());
                    }
                    _ => restore.push(change.path.clone()),
                },
                _ => restore.push(change.path.clone()),
            }
        }
        if !restore.is_empty() {
            let mut checkout = CheckoutBuilder::new();
            checkout.force();
            for path in &restore {
                checkout.path(path.as_str());
            }
            self.inner.checkout_index(None, Some(&mut checkout))?;
        }
        for path in &remove {
            if restore.iter().any(|restored| restored == path) {
                continue;
            }
            remove_worktree_path(workdir, path)?;
        }
        Ok(())
    }

    /// 把统一格式补丁应用到暂存区或工作区。
    pub fn apply_patch(&self, patch: &str, to_index: bool) -> Result<(), Error> {
        let diff = Diff::from_buffer(patch.as_bytes())?;
        let location = if to_index {
            ApplyLocation::Index
        } else {
            ApplyLocation::WorkDir
        };
        self.inner.apply(&diff, location, None)?;
        Ok(())
    }

    // ---- 引用 ----

    pub fn refs(&self) -> Result<RefsSnapshot, Error> {
        let mut local = Vec::new();
        for item in self.inner.branches(Some(BranchType::Local))? {
            let (branch, _) = item?;
            let Some(name) = branch.name()?.map(str::to_string) else {
                continue;
            };
            let Some(target) = branch.get().resolve()?.target() else {
                continue;
            };
            let upstream = branch.upstream().ok();
            let (upstream_name, ahead, behind) = match &upstream {
                Some(up) => {
                    let up_name = up.name().ok().flatten().map(str::to_string);
                    let counts = up
                        .get()
                        .target()
                        .and_then(|up_target| self.inner.graph_ahead_behind(target, up_target).ok())
                        .unwrap_or((0, 0));
                    (up_name, counts.0, counts.1)
                }
                None => (None, 0, 0),
            };
            local.push(LocalBranch {
                is_head: branch.is_head(),
                name,
                target,
                upstream: upstream_name,
                ahead,
                behind,
            });
        }
        local.sort_by(|a, b| a.name.cmp(&b.name));

        let mut remotes: Vec<RemoteInfo> = Vec::new();
        for name in self
            .inner
            .remotes()?
            .iter()
            .filter_map(|name| name.ok().flatten())
        {
            let url = self
                .inner
                .find_remote(name)
                .ok()
                .map(|remote| lossy(remote.url_bytes()))
                .unwrap_or_default();
            remotes.push(RemoteInfo {
                name: name.to_string(),
                url,
                branches: Vec::new(),
            });
        }
        remotes.sort_by_key(|remote| std::cmp::Reverse(remote.name.len()));
        for item in self.inner.branches(Some(BranchType::Remote))? {
            let (branch, _) = item?;
            if branch.get().symbolic_target_bytes().is_some() {
                continue;
            }
            let Some(name) = branch.name()?.map(str::to_string) else {
                continue;
            };
            let Some(target) = branch.get().target() else {
                continue;
            };
            if let Some(remote) = remotes
                .iter_mut()
                .find(|remote| name.starts_with(&format!("{}/", remote.name)))
            {
                let short = name[remote.name.len() + 1..].to_string();
                remote.branches.push(RemoteBranch {
                    name,
                    branch: short,
                    target,
                });
            }
        }
        remotes.sort_by(|a, b| a.name.cmp(&b.name));
        for remote in &mut remotes {
            remote.branches.sort_by(|a, b| a.branch.cmp(&b.branch));
        }

        let mut tags = Vec::new();
        for name in self
            .inner
            .tag_names(None)?
            .iter()
            .filter_map(|name| name.ok().flatten())
        {
            let reference = self.inner.find_reference(&format!("refs/tags/{name}"));
            let (target, annotated) = match reference {
                Ok(reference) => {
                    let annotated = reference
                        .target()
                        .and_then(|id| self.inner.find_tag(id).ok())
                        .is_some();
                    (reference.peel_to_commit().ok().map(|c| c.id()), annotated)
                }
                Err(_) => (None, false),
            };
            tags.push(TagInfo {
                name: name.to_string(),
                target,
                annotated,
            });
        }
        tags.sort_by(|a, b| b.name.cmp(&a.name));

        let mut stashes = Vec::new();
        if let Ok(mut stash_repo) = Repository::open(self.inner.path()) {
            let _ = stash_repo.stash_foreach(|index, message, id| {
                stashes.push(StashInfo {
                    index,
                    message: message.to_string(),
                    id: *id,
                });
                true
            });
        }

        let submodules = self
            .inner
            .submodules()
            .map(|list| {
                list.iter()
                    .map(|module| SubmoduleInfo {
                        name: lossy(module.name_bytes()),
                        path: path_string(module.path()),
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(RefsSnapshot {
            local,
            remotes,
            tags,
            stashes,
            submodules,
        })
    }

    // ---- 历史 ----

    pub fn history(&self, options: HistoryOptions, refs: &RefsSnapshot) -> Result<History, Error> {
        let mut walk = self.inner.revwalk()?;
        walk.set_sorting(Sort::TOPOLOGICAL | Sort::TIME)?;
        if options.first_parent {
            walk.simplify_first_parent()?;
        }
        let head = self.head_id();
        if let Some(head) = head {
            walk.push(head)?;
        }
        if !options.current_branch_only {
            walk.push_glob("refs/heads")?;
            if options.show_remotes {
                walk.push_glob("refs/remotes")?;
            }
            if options.show_tags {
                walk.push_glob("refs/tags")?;
            }
        }

        let mut commits = Vec::new();
        let mut truncated = false;
        for id in walk {
            let id = id?;
            if commits.len() >= options.limit {
                truncated = true;
                break;
            }
            let commit = self.inner.find_commit(id)?;
            let author = commit.author();
            let parents = if options.first_parent {
                commit.parent_ids().take(1).collect()
            } else {
                commit.parent_ids().collect()
            };
            commits.push(CommitRow {
                id,
                short: short_id(&id),
                summary: commit.summary_bytes().map(lossy).unwrap_or_default(),
                author: lossy(author.name_bytes()),
                email: lossy(author.email_bytes()),
                time: author.when().seconds(),
                parents,
            });
        }

        let mut labels: HashMap<Oid, Vec<RefLabel>> = HashMap::new();
        if let Some(head) = head
            && matches!(self.head_state()?, HeadState::Detached(_))
        {
            labels.entry(head).or_default().push(RefLabel {
                name: "HEAD".into(),
                kind: RefKind::Head,
            });
        }
        for branch in &refs.local {
            labels.entry(branch.target).or_default().push(RefLabel {
                name: branch.name.clone(),
                kind: if branch.is_head {
                    RefKind::Head
                } else {
                    RefKind::LocalBranch
                },
            });
        }
        if options.show_remotes {
            for remote in &refs.remotes {
                for branch in &remote.branches {
                    labels.entry(branch.target).or_default().push(RefLabel {
                        name: branch.name.clone(),
                        kind: RefKind::RemoteBranch,
                    });
                }
            }
        }
        if options.show_tags {
            for tag in &refs.tags {
                if let Some(target) = tag.target {
                    labels.entry(target).or_default().push(RefLabel {
                        name: tag.name.clone(),
                        kind: RefKind::Tag,
                    });
                }
            }
        }

        let graph = graph::layout(&commits);
        Ok(History {
            commits,
            graph,
            labels,
            truncated,
        })
    }

    pub fn commit_details(&self, id: Oid) -> Result<CommitDetails, Error> {
        let commit = self.inner.find_commit(id)?;
        let diff = self.commit_diff(&commit, DiffSettings::default())?;
        let files = diff
            .deltas()
            .map(|delta| {
                let path = delta
                    .new_file()
                    .path()
                    .or(delta.old_file().path())
                    .map(path_string)
                    .unwrap_or_default();
                let old_path = (delta.status() == Delta::Renamed)
                    .then(|| delta.old_file().path().map(path_string))
                    .flatten();
                FileChange {
                    path,
                    old_path,
                    kind: delta_kind(delta.status()),
                }
            })
            .collect();
        Ok(CommitDetails {
            id,
            summary: commit.summary_bytes().map(lossy).unwrap_or_default(),
            message: lossy(commit.message_bytes()),
            author: person(&commit.author()),
            committer: person(&commit.committer()),
            parents: commit.parent_ids().collect(),
            files,
        })
    }

    pub fn commit_file_diff(
        &self,
        id: Oid,
        path: &str,
        settings: DiffSettings,
    ) -> Result<FileDiff, Error> {
        let commit = self.inner.find_commit(id)?;
        let diff = self.commit_diff(&commit, settings)?;
        file_diff(&diff, path)
    }

    fn commit_diff(
        &self,
        commit: &git2::Commit<'_>,
        settings: DiffSettings,
    ) -> Result<Diff<'_>, Error> {
        let tree = commit.tree()?;
        let parent_tree = match commit.parent(0) {
            Ok(parent) => Some(parent.tree()?),
            Err(_) => None,
        };
        let mut options = diff_options(settings);
        let mut diff =
            self.inner
                .diff_tree_to_tree(parent_tree.as_ref(), Some(&tree), Some(&mut options))?;
        let mut find = DiffFindOptions::new();
        find.renames(true);
        diff.find_similar(Some(&mut find))?;
        Ok(diff)
    }

    pub fn tree_files(&self, id: Oid) -> Result<(Vec<String>, bool), Error> {
        let tree = self.inner.find_commit(id)?.tree()?;
        let mut files = Vec::new();
        let mut truncated = false;
        tree.walk(git2::TreeWalkMode::PreOrder, |root, entry| {
            if files.len() >= MAX_TREE_FILES {
                truncated = true;
                return git2::TreeWalkResult::Abort;
            }
            if entry.kind() == Some(ObjectType::Blob) {
                files.push(format!("{root}{}", lossy(entry.name_bytes())));
            }
            git2::TreeWalkResult::Ok
        })
        .or_else(|err| if truncated { Ok(()) } else { Err(err) })?;
        Ok((files, truncated))
    }

    pub fn blob_at(&self, id: Oid, path: &str) -> Result<BlobContent, Error> {
        let tree = self.inner.find_commit(id)?.tree()?;
        let entry = tree.get_path(Path::new(path))?;
        let blob = entry.to_object(&self.inner)?.peel_to_blob()?;
        let size = blob.size();
        if blob.is_binary() {
            Ok(BlobContent::Binary { size })
        } else if size > MAX_BLOB_BYTES {
            Ok(BlobContent::TooLarge { size })
        } else {
            Ok(BlobContent::Text(lossy(blob.content())))
        }
    }

    pub fn blame(&self, path: &str) -> Result<Vec<BlameLine>, Error> {
        let head = self.inner.head()?.peel_to_commit()?;
        let mut options = BlameOptions::new();
        options.newest_commit(head.id());
        let blame = self.inner.blame_file(Path::new(path), Some(&mut options))?;
        let BlobContent::Text(text) = self.blob_at(head.id(), path)? else {
            return Ok(Vec::new());
        };
        let mut lines = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let hunk = blame.get_line(index + 1);
            let (commit, author, time) = match &hunk {
                Some(hunk) => {
                    let signature = hunk.final_signature();
                    (
                        Some(hunk.final_commit_id()),
                        signature
                            .as_ref()
                            .map(|s| lossy(s.name_bytes()))
                            .unwrap_or_default(),
                        signature.map(|s| s.when().seconds()).unwrap_or(0),
                    )
                }
                None => (None, String::new(), 0),
            };
            lines.push(BlameLine {
                line_no: index + 1,
                text: line.to_string(),
                commit,
                author,
                time,
            });
        }
        Ok(lines)
    }

    // ---- 命令行使用的简单操作 ----

    pub fn status(&self) -> Result<StatusReport, Error> {
        let mut options = StatusOptions::new();
        options
            .include_untracked(true)
            .renames_head_to_index(true)
            .renames_index_to_workdir(true);
        let statuses = self.inner.statuses(Some(&mut options))?;
        let entries = statuses
            .iter()
            .map(|entry| StatusEntry {
                code: status_code(entry.status()),
                // 重命名时 `entry.path()` 是旧路径；取文件在工作区或暂存区里的当前路径。
                path: entry
                    .index_to_workdir()
                    .or_else(|| entry.head_to_index())
                    .and_then(|delta| delta.new_file().path().map(path_string))
                    .unwrap_or_else(|| lossy(entry.path_bytes())),
            })
            .collect();
        Ok(StatusReport {
            head: self.head_state()?,
            entries,
        })
    }

    pub fn log(&self, limit: usize) -> Result<Vec<CommitSummary>, Error> {
        if limit == 0 || matches!(self.head_state()?, HeadState::Unborn) {
            return Ok(Vec::new());
        }
        let mut walk = self.inner.revwalk()?;
        walk.push_head()?;
        let mut commits = Vec::new();
        for oid in walk.take(limit) {
            let commit = self.inner.find_commit(oid?)?;
            commits.push(CommitSummary {
                id: short_id(&commit.id()),
                summary: commit.summary_bytes().map(lossy).unwrap_or_default(),
                author: lossy(commit.author().name_bytes()),
            });
        }
        Ok(commits)
    }

    pub fn branches(&self) -> Result<Vec<BranchInfo>, Error> {
        Ok(self
            .refs()?
            .local
            .into_iter()
            .map(|branch| BranchInfo {
                name: branch.name,
                current: branch.is_head,
            })
            .collect())
    }

    pub fn add(&self, paths: &[PathBuf]) -> Result<(), Error> {
        if paths.is_empty() {
            return Err(Error::NoPaths);
        }
        let workdir = self.inner.workdir().ok_or(Error::NoWorkdir)?;
        let specs: Vec<String> = paths
            .iter()
            .map(|path| pathspec(workdir, path))
            .collect::<Result<_, _>>()?;
        let mut index = self.inner.index()?;
        index.add_all(specs, IndexAddOption::DEFAULT, None)?;
        index.write()?;
        Ok(())
    }

    pub fn commit(&self, message: &str) -> Result<String, Error> {
        let message = message.trim();
        if message.is_empty() {
            return Err(Error::EmptyMessage);
        }
        let signature = self.inner.signature().map_err(|err| {
            if err.code() == ErrorCode::NotFound {
                Error::MissingIdentity
            } else {
                Error::Git(err)
            }
        })?;
        let mut index = self.inner.index()?;
        let tree_id = index.write_tree()?;
        let tree = self.inner.find_tree(tree_id)?;
        let parent = match self.inner.head() {
            Ok(head) => Some(head.peel_to_commit()?),
            Err(err) if err.code() == ErrorCode::UnbornBranch => None,
            Err(err) => return Err(err.into()),
        };
        match &parent {
            Some(parent) if parent.tree_id() == tree_id => return Err(Error::NothingToCommit),
            None if tree.iter().next().is_none() => return Err(Error::NothingToCommit),
            _ => {}
        }
        let parents: Vec<&git2::Commit<'_>> = parent.iter().collect();
        let id = self.inner.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parents,
        )?;
        Ok(short_id(&id))
    }

    pub fn create_branch(&self, name: &str) -> Result<(), Error> {
        let head = match self.inner.head() {
            Ok(head) => head.peel_to_commit()?,
            Err(err) if err.code() == ErrorCode::UnbornBranch => return Err(Error::NoHead),
            Err(err) => return Err(err.into()),
        };
        match self.inner.branch(name, &head, false) {
            Ok(_) => Ok(()),
            Err(err) if err.code() == ErrorCode::Exists => {
                Err(Error::BranchExists(name.to_string()))
            }
            Err(err) => Err(err.into()),
        }
    }

    pub fn switch(&self, name: &str) -> Result<(), Error> {
        let refname = format!("refs/heads/{name}");
        let object = match self.inner.revparse_single(&refname) {
            Ok(object) => object,
            Err(err) if err.code() == ErrorCode::NotFound => {
                return Err(Error::MissingBranch(name.to_string()));
            }
            Err(err) => return Err(err.into()),
        };
        self.inner.checkout_tree(&object, None)?;
        self.inner.set_head(&refname)?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn raw(&self) -> &Repository {
        &self.inner
    }
}

pub fn short_id(oid: &Oid) -> String {
    oid.to_string().chars().take(8).collect()
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn person(signature: &git2::Signature<'_>) -> Person {
    Person {
        name: lossy(signature.name_bytes()),
        email: lossy(signature.email_bytes()),
        time: signature.when().seconds(),
    }
}

fn diff_options(settings: DiffSettings) -> DiffOptions {
    let mut options = DiffOptions::new();
    options
        .context_lines(settings.context_lines)
        .ignore_whitespace(settings.ignore_whitespace);
    options
}

fn remove_worktree_path(workdir: &Path, path: &str) -> Result<(), Error> {
    let full = workdir.join(path);
    if full.is_dir() {
        std::fs::remove_dir_all(full)?;
    } else if full.symlink_metadata().is_ok() {
        std::fs::remove_file(full)?;
    }
    Ok(())
}

fn delta_kind(delta: Delta) -> ChangeKind {
    match delta {
        Delta::Added | Delta::Copied => ChangeKind::Added,
        Delta::Deleted => ChangeKind::Deleted,
        Delta::Renamed => ChangeKind::Renamed,
        Delta::Typechange => ChangeKind::TypeChange,
        Delta::Untracked => ChangeKind::Untracked,
        Delta::Conflicted => ChangeKind::Conflicted,
        _ => ChangeKind::Modified,
    }
}

fn index_kind(status: Status) -> Option<ChangeKind> {
    if status.is_index_new() {
        Some(ChangeKind::Added)
    } else if status.is_index_modified() {
        Some(ChangeKind::Modified)
    } else if status.is_index_deleted() {
        Some(ChangeKind::Deleted)
    } else if status.is_index_renamed() {
        Some(ChangeKind::Renamed)
    } else if status.is_index_typechange() {
        Some(ChangeKind::TypeChange)
    } else {
        None
    }
}

fn worktree_kind(status: Status) -> Option<ChangeKind> {
    if status.is_wt_new() {
        Some(ChangeKind::Untracked)
    } else if status.is_wt_modified() {
        Some(ChangeKind::Modified)
    } else if status.is_wt_deleted() {
        Some(ChangeKind::Deleted)
    } else if status.is_wt_renamed() {
        Some(ChangeKind::Renamed)
    } else if status.is_wt_typechange() {
        Some(ChangeKind::TypeChange)
    } else {
        None
    }
}

fn file_diff(diff: &Diff<'_>, path: &str) -> Result<FileDiff, Error> {
    let index = diff.deltas().position(|delta| {
        delta.new_file().path().map(path_string).as_deref() == Some(path)
            || delta.old_file().path().map(path_string).as_deref() == Some(path)
    });
    let Some(index) = index else {
        return Ok(FileDiff {
            path: path.to_string(),
            ..Default::default()
        });
    };
    let delta = diff.get_delta(index).expect("索引来自同一个差异");
    let old_path = (delta.status() == Delta::Renamed)
        .then(|| delta.old_file().path().map(path_string))
        .flatten();
    let mut result = FileDiff {
        path: path.to_string(),
        old_path,
        binary: delta.flags().is_binary(),
        hunks: Vec::new(),
        omitted_lines: 0,
    };
    let Some(patch) = Patch::from_diff(diff, index)? else {
        result.binary = true;
        return Ok(result);
    };
    let mut shown = 0usize;
    for hunk_index in 0..patch.num_hunks() {
        let (hunk, line_count) = patch.hunk(hunk_index)?;
        let mut lines = Vec::new();
        for line_index in 0..line_count {
            let line = patch.line_in_hunk(hunk_index, line_index)?;
            let kind = match line.origin() {
                '+' => LineKind::Added,
                '-' => LineKind::Removed,
                ' ' => LineKind::Context,
                _ => continue,
            };
            if shown >= MAX_DIFF_LINES {
                result.omitted_lines += 1;
                continue;
            }
            shown += 1;
            lines.push(DiffLine {
                kind,
                old_no: line.old_lineno(),
                new_no: line.new_lineno(),
                raw: lossy(line.content()),
            });
        }
        if lines.is_empty() {
            continue;
        }
        result.hunks.push(Hunk {
            header: lossy(hunk.header()).trim_end().to_string(),
            old_start: hunk.old_start(),
            old_lines: hunk.old_lines(),
            new_start: hunk.new_start(),
            new_lines: hunk.new_lines(),
            lines,
        });
    }
    Ok(result)
}

fn status_code(status: Status) -> String {
    if status.is_conflicted() {
        return "UU".to_string();
    }
    if status.is_wt_new() {
        return "??".to_string();
    }
    let index = index_kind(status).map(ChangeKind::letter).unwrap_or(' ');
    let worktree = worktree_kind(status).map(ChangeKind::letter).unwrap_or(' ');
    format!("{index}{worktree}")
}

fn pathspec(workdir: &Path, path: &Path) -> Result<String, Error> {
    let relative = if path.is_absolute() {
        strip_workdir(workdir, path)?
    } else {
        path.to_path_buf()
    };
    if relative.as_os_str().is_empty() {
        return Ok(".".to_string());
    }
    Ok(path_string(&relative))
}

fn strip_workdir(workdir: &Path, path: &Path) -> Result<PathBuf, Error> {
    if let Ok(relative) = path.strip_prefix(workdir) {
        return Ok(relative.to_path_buf());
    }
    let outside = || Error::OutsideWorkdir(path.display().to_string());
    let workdir = workdir.canonicalize().map_err(|_| outside())?;
    let canonical = path.canonicalize().map_err(|_| outside())?;
    canonical
        .strip_prefix(workdir)
        .map(Path::to_path_buf)
        .map_err(|_| outside())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::patch::{PatchDirection, hunk_patch};
    use std::collections::BTreeSet;

    fn repo_with_identity() -> (tempfile::TempDir, Repo) {
        let dir = tempfile::tempdir().expect("临时目录");
        let repo = Repo::init(dir.path()).expect("初始化");
        let mut config =
            git2::Config::open(&repo.raw().path().join("config")).expect("打开仓库配置");
        config.set_str("user.name", "Test").expect("user.name");
        config
            .set_str("user.email", "test@example.com")
            .expect("user.email");
        (dir, repo)
    }

    fn commit_file(dir: &Path, repo: &Repo, name: &str, text: &str, message: &str) {
        std::fs::write(dir.join(name), text).unwrap();
        repo.add(&[PathBuf::from(name)]).unwrap();
        repo.commit(message).unwrap();
    }

    #[test]
    fn commit_then_status_is_clean_and_log_has_one_entry() {
        let (dir, repo) = repo_with_identity();
        commit_file(dir.path(), &repo, "a.txt", "hello\n", "第一条");
        let status = repo.status().unwrap();
        assert!(matches!(status.head, HeadState::Branch(_)));
        assert!(status.entries.is_empty());
        let log = repo.log(20).unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].summary, "第一条");
    }

    #[test]
    fn working_tree_separates_staged_and_unstaged() {
        let (dir, repo) = repo_with_identity();
        commit_file(dir.path(), &repo, "a.txt", "one\n", "init");
        std::fs::write(dir.path().join("a.txt"), "two\n").unwrap();
        std::fs::write(dir.path().join("b.txt"), "new\n").unwrap();
        repo.stage_paths(&["a.txt".into()]).unwrap();
        std::fs::write(dir.path().join("a.txt"), "three\n").unwrap();

        let tree = repo.working_tree().unwrap();
        assert_eq!(tree.staged.len(), 1);
        assert_eq!(tree.staged[0].kind, ChangeKind::Modified);
        let kinds: Vec<_> = tree
            .unstaged
            .iter()
            .map(|c| (c.path.as_str(), c.kind))
            .collect();
        assert!(kinds.contains(&("a.txt", ChangeKind::Modified)));
        assert!(kinds.contains(&("b.txt", ChangeKind::Untracked)));

        repo.unstage_paths(&["a.txt".into()]).unwrap();
        assert!(repo.working_tree().unwrap().staged.is_empty());
    }

    #[test]
    fn staged_rename_is_listed_under_its_new_path() {
        let (dir, repo) = repo_with_identity();
        commit_file(dir.path(), &repo, "old.txt", "same text\n", "init");
        std::fs::rename(dir.path().join("old.txt"), dir.path().join("new.txt")).unwrap();
        repo.stage_paths(&["old.txt".into(), "new.txt".into()])
            .unwrap();

        let tree = repo.working_tree().unwrap();
        let staged: Vec<_> = tree
            .staged
            .iter()
            .map(|c| (c.path.as_str(), c.old_path.as_deref(), c.kind))
            .collect();
        assert_eq!(staged, [("new.txt", Some("old.txt"), ChangeKind::Renamed)]);
    }

    #[test]
    fn staging_selected_line_through_patch() {
        let (dir, repo) = repo_with_identity();
        commit_file(dir.path(), &repo, "a.txt", "1\n2\n3\n", "init");
        std::fs::write(dir.path().join("a.txt"), "1\nX\n3\nY\n").unwrap();
        let diff = repo
            .worktree_diff("a.txt", false, DiffSettings::default())
            .unwrap();
        let hunk = &diff.hunks[0];
        let added_y = hunk
            .lines
            .iter()
            .position(|line| line.text() == "Y")
            .unwrap();
        let patch = hunk_patch(
            "a.txt",
            hunk,
            Some(&BTreeSet::from([added_y])),
            PatchDirection::Forward,
        )
        .unwrap();
        repo.apply_patch(&patch, true).unwrap();

        let staged = repo
            .worktree_diff("a.txt", true, DiffSettings::default())
            .unwrap();
        let added: Vec<_> = staged.hunks[0]
            .lines
            .iter()
            .filter(|line| line.kind == LineKind::Added)
            .map(DiffLine::text)
            .collect();
        assert_eq!(added, vec!["Y"]);
    }

    #[test]
    fn history_labels_branch_and_details_list_files() {
        let (dir, repo) = repo_with_identity();
        commit_file(dir.path(), &repo, "a.txt", "1\n", "first");
        commit_file(dir.path(), &repo, "b.txt", "2\n", "second");
        let refs = repo.refs().unwrap();
        let history = repo.history(HistoryOptions::default(), &refs).unwrap();
        assert_eq!(history.commits.len(), 2);
        assert_eq!(history.commits[0].summary, "second");
        let head_labels = &history.labels[&history.commits[0].id];
        assert_eq!(head_labels[0].kind, RefKind::Head);

        let details = repo.commit_details(history.commits[0].id).unwrap();
        assert_eq!(details.files.len(), 1);
        assert_eq!(details.files[0].path, "b.txt");
        assert_eq!(details.files[0].kind, ChangeKind::Added);
    }

    #[test]
    fn discard_restores_tracked_and_removes_untracked() {
        let (dir, repo) = repo_with_identity();
        commit_file(dir.path(), &repo, "a.txt", "1\n", "init");
        std::fs::write(dir.path().join("a.txt"), "changed\n").unwrap();
        std::fs::write(dir.path().join("new.txt"), "x\n").unwrap();
        let tree = repo.working_tree().unwrap();
        repo.discard_paths(&tree.unstaged).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "1\n"
        );
        assert!(!dir.path().join("new.txt").exists());
    }

    #[test]
    fn discard_undoes_workdir_rename_and_keeps_staged_edits() {
        let (dir, repo) = repo_with_identity();
        commit_file(dir.path(), &repo, "old.txt", "1\n", "init");
        commit_file(dir.path(), &repo, "a.txt", "one\n", "second");
        std::fs::rename(dir.path().join("old.txt"), dir.path().join("new.txt")).unwrap();
        std::fs::write(dir.path().join("a.txt"), "staged\n").unwrap();
        repo.stage_paths(&["a.txt".into()]).unwrap();
        std::fs::write(dir.path().join("a.txt"), "unstaged\n").unwrap();

        let tree = repo.working_tree().unwrap();
        let renamed = tree
            .unstaged
            .iter()
            .find(|change| change.kind == ChangeKind::Renamed)
            .expect("相同内容的改名应记成一条重命名");
        assert_eq!(renamed.path, "new.txt");
        assert_eq!(renamed.old_path.as_deref(), Some("old.txt"));
        let modified = tree
            .unstaged
            .iter()
            .find(|change| change.path == "a.txt")
            .unwrap();
        repo.discard_paths(&[renamed.clone(), modified.clone()])
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(dir.path().join("old.txt")).unwrap(),
            "1\n"
        );
        assert!(!dir.path().join("new.txt").exists());
        // 只丢掉未暂存的修改，已暂存的内容留在工作区。
        assert_eq!(
            std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
            "staged\n"
        );
    }

    #[test]
    fn empty_and_blank_commits_are_rejected() {
        let (dir, repo) = repo_with_identity();
        assert!(matches!(repo.commit("  "), Err(Error::EmptyMessage)));
        commit_file(dir.path(), &repo, "a.txt", "1\n", "init");
        assert!(matches!(repo.commit("again"), Err(Error::NothingToCommit)));
    }
}
