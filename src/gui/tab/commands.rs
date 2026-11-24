use git_forker::{GitJob, HeadState, Repo, tr};

use super::{DetailsTab, RepoTab, SidebarMode, View};
use crate::gui::dialogs::{DialogContext, Outcome};
use crate::gui::menu::{Command, RepositoryState};

impl RepoTab {
    pub fn menu_state(&self) -> Option<RepositoryState> {
        let data = self.data.as_ref()?;
        Some(RepositoryState {
            view: self.view,
            details_tab: self.details_tab,
            show_details: self.show_details,
            show_log: self.show_log,
            options: self.history_options,
            has_stash: !data.refs.stashes.is_empty(),
            has_changes: self.change_count() > 0,
            has_remotes: !data.refs.remotes.is_empty(),
            on_branch: matches!(data.tree.head, HeadState::Branch(_)),
        })
    }

    /// 执行作用于当前仓库的菜单命令。返回 false 表示命令不归仓库处理。
    pub fn run(&mut self, command: Command) -> bool {
        match command {
            Command::QuickLaunch => self.open_command_palette(),
            Command::Find => {
                self.view = View::History;
                self.sidebar_mode = SidebarMode::Search;
                self.focus_search = true;
            }
            Command::ShowChanges => self.view = View::LocalChanges,
            Command::ShowHistory => self.view = View::History,
            Command::Details(tab) => {
                self.view = View::History;
                self.show_details = true;
                self.details_tab = tab;
                if tab == DetailsTab::Tree
                    && let Ok(repo) = Repo::discover(&self.root)
                {
                    self.load_tree(&repo);
                }
            }
            Command::ToggleDetails => self.show_details = !self.show_details,
            Command::ShowHead => self.show_head(),
            Command::ToggleTags => self.toggle_option(|options| {
                options.show_tags = !options.show_tags;
            }),
            Command::ToggleRemoteBranches => self.toggle_option(|options| {
                options.show_remotes = !options.show_remotes;
            }),
            Command::ToggleFirstParent => self.toggle_option(|options| {
                options.first_parent = !options.first_parent;
            }),
            Command::ToggleCurrentBranch => self.toggle_option(|options| {
                options.current_branch_only = !options.current_branch_only;
            }),
            Command::ToggleActivity => self.show_log = !self.show_log,
            Command::Refresh => self.request_reload(),
            Command::Commit => {
                self.view = View::LocalChanges;
                self.focus_commit = true;
            }
            Command::Fetch => self.open_fetch(),
            Command::QuickFetch => self.quick(Self::open_fetch),
            Command::Pull => self.open_pull(),
            Command::QuickPull => self.quick(Self::open_pull),
            Command::Push => self.open_push(),
            Command::QuickPush => self.quick(Self::open_push),
            Command::Stash => self.open_stash(),
            Command::QuickStash => self.quick(Self::open_stash),
            Command::PopStash => self.pop_stash(),
            Command::CheckoutBranch => self.open_quick_checkout(),
            Command::NewBranch => self.open_create_branch(),
            Command::NewTag => self.open_create_tag(),
            Command::AddRemote => self.add_remote(),
            Command::ApplyPatch => self.apply_patch(),
            Command::RevealRepository => self.reveal_in_file_manager(),
            Command::OpenTerminal => self.open_terminal(),
            Command::RepositorySettings => self.open_identity(),
            _ => return false,
        }
        true
    }

    fn toggle_option(&mut self, change: impl FnOnce(&mut git_forker::HistoryOptions)) {
        change(&mut self.history_options);
        self.request_reload();
    }

    /// 用对话框的默认值直接执行，不弹出对话框。
    fn quick(&mut self, open: fn(&mut Self)) {
        open(self);
        let (Some(mut dialog), Some(data)) = (self.dialog.take(), self.data.as_ref()) else {
            return;
        };
        let outcome = dialog.finish(&DialogContext {
            root: &self.root,
            refs: &data.refs,
            local_changes: data.tree.unstaged.len()
                + data.tree.staged.len()
                + data.tree.conflicted.len(),
        });
        match outcome {
            Outcome::Job(job) => self.push_job(job),
            // 默认值不能直接执行时（例如还没有远程分支），退回到对话框让用户补全。
            _ => self.dialog = Some(dialog),
        }
    }

    fn apply_patch(&mut self) {
        let Some(file) = rfd::FileDialog::new()
            .set_title(tr!("选择补丁文件"))
            .add_filter("patch", &["patch", "diff"])
            .set_directory(&self.root)
            .pick_file()
        else {
            return;
        };
        let title = tr!("应用补丁 {name}", name = file.display());
        let path = file.to_string_lossy().into_owned();
        self.push_job(GitJob::new(title, &self.root).step(["apply", "--3way", path.as_str()]));
    }

    fn open_identity(&mut self) {
        let (name, email) = self
            .data
            .as_ref()
            .and_then(|data| data.identity.clone())
            .unwrap_or_default();
        self.dialog = Some(crate::gui::dialogs::Dialog::Identity { name, email });
    }
}
