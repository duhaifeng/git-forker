use git_forker::tr;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, RichText, Ui};
use git_forker::{FileChange, GitJob, RefsSnapshot};

/// 切换分支或提交时如何处理工作区里的更改。对应原程序的三个单选项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalChanges {
    Keep,
    StashAndReapply,
    Discard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMode {
    Default,
    NoFastForward,
    FastForwardOnly,
    Squash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

pub enum Dialog {
    Fetch {
        remote: String,
        all: bool,
        prune: bool,
    },
    Pull {
        remote: String,
        branch: String,
        rebase: bool,
        autostash: bool,
    },
    Push {
        local: String,
        remote: String,
        remote_branch: String,
        track: bool,
        force: bool,
        tags: bool,
    },
    CreateBranch {
        start: String,
        start_label: String,
        name: String,
        checkout: bool,
        changes: LocalChanges,
    },
    Checkout {
        target: String,
        label: String,
        detach: bool,
        changes: LocalChanges,
    },
    TrackRemote {
        remote_branch: String,
        local_name: String,
        changes: LocalChanges,
    },
    CreateTag {
        target: String,
        target_label: String,
        name: String,
        message: String,
        push: bool,
        remote: String,
    },
    CreateStash {
        message: String,
        include_untracked: bool,
        /// 保留已暂存的文件，贮藏后仍可提交。
        keep_index: bool,
    },
    ApplyStash {
        index: usize,
        message: String,
        delete_after: bool,
    },
    DropStash {
        index: usize,
        message: String,
    },
    Merge {
        branch: String,
        current: String,
        mode: MergeMode,
    },
    Rebase {
        onto: String,
        current: String,
        autostash: bool,
        update_refs: bool,
    },
    CherryPick {
        sha: String,
        summary: String,
        append_origin: bool,
        commit: bool,
        parents: usize,
        parent: usize,
    },
    Revert {
        sha: String,
        summary: String,
        commit: bool,
        parents: usize,
        parent: usize,
    },
    Reset {
        target: String,
        label: String,
        current: String,
        mode: ResetMode,
    },
    DeleteBranch {
        name: String,
        upstream: Option<String>,
        delete_remote: bool,
        force: bool,
    },
    DeleteRemoteBranch {
        name: String,
    },
    RenameBranch {
        old: String,
        new: String,
        upstream: Option<String>,
        rename_remote: bool,
    },
    DeleteTag {
        name: String,
        delete_remote: bool,
        remote: String,
    },
    EditRemote {
        original: Option<String>,
        name: String,
        url: String,
    },
    DeleteRemote {
        name: String,
    },
    Reword {
        subject: String,
        description: String,
    },
    Discard {
        changes: Vec<FileChange>,
    },
    Identity {
        name: String,
        email: String,
    },
}

pub enum Outcome {
    Keep,
    Cancel,
    Job(GitJob),
    Discard(Vec<FileChange>),
}

pub struct DialogContext<'a> {
    pub root: &'a Path,
    pub refs: &'a RefsSnapshot,
    /// 工作区里还没提交的文件数（含已暂存和冲突）。
    pub local_changes: usize,
}

impl Dialog {
    fn title(&self) -> &'static str {
        match self {
            Dialog::Fetch { .. } => tr!("抓取"),
            Dialog::Pull { .. } => tr!("拉取"),
            Dialog::Push { .. } => tr!("推送"),
            Dialog::CreateBranch { .. } => tr!("创建分支"),
            Dialog::Checkout { .. } => tr!("检出"),
            Dialog::TrackRemote { .. } => tr!("检出远程分支"),
            Dialog::CreateTag { .. } => tr!("创建标签"),
            Dialog::CreateStash { .. } => tr!("贮藏更改"),
            Dialog::ApplyStash { .. } => tr!("应用贮藏"),
            Dialog::DropStash { .. } => tr!("删除贮藏"),
            Dialog::Merge { .. } => tr!("合并"),
            Dialog::Rebase { .. } => tr!("变基"),
            Dialog::CherryPick { .. } => tr!("拣选提交"),
            Dialog::Revert { .. } => tr!("还原提交"),
            Dialog::Reset { .. } => tr!("重置"),
            Dialog::DeleteBranch { .. } => tr!("删除分支"),
            Dialog::DeleteRemoteBranch { .. } => tr!("删除远程分支"),
            Dialog::RenameBranch { .. } => tr!("重命名分支"),
            Dialog::DeleteTag { .. } => tr!("删除标签"),
            Dialog::EditRemote { original: None, .. } => tr!("添加远程仓库"),
            Dialog::EditRemote { .. } => tr!("编辑远程仓库"),
            Dialog::DeleteRemote { .. } => tr!("删除远程仓库"),
            Dialog::Reword { .. } => tr!("修改提交说明"),
            Dialog::Discard { .. } => tr!("放弃更改"),
            Dialog::Identity { .. } => tr!("设置提交身份"),
        }
    }

    fn confirm_label(&self) -> &'static str {
        match self {
            Dialog::Fetch { .. } => tr!("抓取"),
            Dialog::Pull { .. } => tr!("拉取"),
            Dialog::Push { .. } => tr!("推送"),
            Dialog::CreateBranch { .. } => tr!("创建"),
            Dialog::Checkout { .. } | Dialog::TrackRemote { .. } => tr!("检出"),
            Dialog::CreateTag { .. } => tr!("创建"),
            Dialog::CreateStash { .. } => tr!("贮藏"),
            Dialog::ApplyStash { .. } => tr!("应用"),
            Dialog::Merge { .. } => tr!("合并"),
            Dialog::Rebase { .. } => tr!("变基"),
            Dialog::CherryPick { .. } => tr!("拣选"),
            Dialog::Revert { .. } => tr!("还原"),
            Dialog::Reset { .. } => tr!("重置"),
            Dialog::RenameBranch { .. } => tr!("重命名"),
            Dialog::EditRemote { .. } | Dialog::Identity { .. } => tr!("保存"),
            Dialog::Reword { .. } => tr!("修改"),
            Dialog::DropStash { .. }
            | Dialog::DeleteBranch { .. }
            | Dialog::DeleteRemoteBranch { .. }
            | Dialog::DeleteTag { .. }
            | Dialog::DeleteRemote { .. } => tr!("删除"),
            Dialog::Discard { .. } => tr!("放弃"),
        }
    }

    fn destructive(&self) -> bool {
        matches!(
            self,
            Dialog::DropStash { .. }
                | Dialog::DeleteBranch { .. }
                | Dialog::DeleteRemoteBranch { .. }
                | Dialog::DeleteTag { .. }
                | Dialog::DeleteRemote { .. }
                | Dialog::Discard { .. }
                | Dialog::Reset {
                    mode: ResetMode::Hard,
                    ..
                }
        )
    }

    pub fn show(&mut self, ctx: &egui::Context, env: &DialogContext<'_>) -> Outcome {
        let mut outcome = Outcome::Keep;
        let modal = egui::Modal::new(egui::Id::unique("git-forker-dialog")).show(ctx, |ui| {
            ui.set_min_width(440.0);
            ui.heading(self.title());
            ui.add_space(8.0);
            let valid = self.body(ui, env);
            ui.add_space(12.0);
            ui.separator();
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut text = RichText::new(self.confirm_label());
                    if self.destructive() {
                        text = text.color(Color32::from_rgb(0xff, 0x7b, 0x7b));
                    }
                    let confirm = ui.add_enabled(valid.is_ok(), egui::Button::new(text));
                    let enter = valid.is_ok()
                        && ui.input(|input| {
                            input.key_pressed(egui::Key::Enter) && input.modifiers.command
                        });
                    if confirm.clicked() || enter {
                        outcome = self.finish(env);
                    }
                    if ui.button(tr!("取消")).clicked() {
                        outcome = Outcome::Cancel;
                    }
                    if let Err(reason) = &valid {
                        ui.colored_label(ui.visuals().warn_fg_color, *reason);
                    }
                });
            });
        });
        if matches!(outcome, Outcome::Keep) && modal.should_close() {
            outcome = Outcome::Cancel;
        }
        outcome
    }

    /// 绘制对话框内容，返回当前输入是否可以提交。
    fn body(&mut self, ui: &mut Ui, env: &DialogContext<'_>) -> Result<(), &'static str> {
        let remotes = env.refs.remote_names();
        match self {
            Dialog::Fetch { remote, all, prune } => {
                ui.add_enabled_ui(!*all, |ui| {
                    remote_combo(ui, "fetch-remote", remote, &remotes)
                });
                ui.checkbox(all, tr!("抓取所有远程仓库"));
                ui.checkbox(prune, tr!("删除远程已不存在的分支 (--prune)"));
                require(*all || !remote.is_empty(), tr!("没有远程仓库"))
            }
            Dialog::Pull {
                remote,
                branch,
                rebase,
                autostash,
            } => {
                remote_combo(ui, "pull-remote", remote, &remotes);
                let branches: Vec<String> = env
                    .refs
                    .remotes
                    .iter()
                    .filter(|info| info.name == *remote)
                    .flat_map(|info| info.branches.iter().map(|b| b.branch.clone()))
                    .collect();
                labeled(ui, tr!("远程分支"), |ui| {
                    combo(ui, "pull-branch", branch, &branches)
                });
                ui.checkbox(rebase, tr!("用变基代替合并 (--rebase)"));
                ui.checkbox(autostash, tr!("挡住更新时贮藏并重新应用"));
                if env.local_changes > 0 {
                    let text = if *autostash {
                        tr!(
                            "本地有 {count} 个未提交的更改。会先直接拉取，暂存状态保持不变。只有这些更改挡住更新时才先贮藏，再按原来的暂存状态重新应用。重新应用发生冲突时，改动会留在贮藏里。",
                            count = env.local_changes
                        )
                    } else {
                        tr!(
                            "本地有 {count} 个未提交的更改。与远端的修改重叠时拉取会失败，请先提交或贮藏。",
                            count = env.local_changes
                        )
                    };
                    ui.add_space(4.0);
                    ui.colored_label(ui.visuals().warn_fg_color, text);
                }
                require(
                    !remote.is_empty() && !branch.is_empty(),
                    tr!("请选择远程分支"),
                )
            }
            Dialog::Push {
                local,
                remote,
                remote_branch,
                track,
                force,
                tags,
            } => {
                let locals: Vec<String> = env.refs.local.iter().map(|b| b.name.clone()).collect();
                labeled(ui, tr!("本地分支"), |ui| {
                    combo(ui, "push-local", local, &locals)
                });
                remote_combo(ui, "push-remote", remote, &remotes);
                labeled(ui, tr!("远程分支"), |ui| {
                    ui.text_edit_singleline(remote_branch);
                });
                ui.checkbox(track, tr!("设置上游分支 (-u)"));
                ui.checkbox(tags, tr!("同时推送所有标签"));
                ui.checkbox(force, tr!("强制推送 (--force-with-lease)"));
                if *force {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        tr!("强制推送会覆盖远程分支上别人推送的提交。"),
                    );
                }
                require(
                    !local.is_empty() && !remote.is_empty() && !remote_branch.trim().is_empty(),
                    tr!("请填写分支和远程仓库"),
                )
            }
            Dialog::CreateBranch {
                start_label,
                name,
                checkout,
                changes,
                ..
            } => {
                ui.label(tr!("起点: {start_label}", start_label = start_label));
                labeled(ui, tr!("分支名"), |ui| {
                    let response = ui.text_edit_singleline(name);
                    autofocus(ui, response);
                });
                ui.checkbox(checkout, tr!("创建后检出"));
                if *checkout {
                    local_changes_ui(ui, changes);
                }
                valid_ref_name(name)
            }
            Dialog::Checkout {
                label,
                changes,
                detach,
                ..
            } => {
                ui.label(tr!("检出 {label}", label = label));
                if *detach {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        tr!("将进入分离 HEAD 状态。新提交不属于任何分支。"),
                    );
                }
                local_changes_ui(ui, changes);
                Ok(())
            }
            Dialog::TrackRemote {
                remote_branch,
                local_name,
                changes,
            } => {
                ui.label(tr!(
                    "远程分支: {remote_branch}",
                    remote_branch = remote_branch
                ));
                labeled(ui, tr!("本地分支名"), |ui| {
                    ui.text_edit_singleline(local_name);
                });
                local_changes_ui(ui, changes);
                valid_ref_name(local_name)
            }
            Dialog::CreateTag {
                target_label,
                name,
                message,
                push,
                remote,
                ..
            } => {
                ui.label(tr!("位置: {target_label}", target_label = target_label));
                labeled(ui, tr!("标签名"), |ui| {
                    let response = ui.text_edit_singleline(name);
                    autofocus(ui, response);
                });
                ui.label(tr!("说明（留空则创建轻量标签）"));
                ui.add(
                    egui::TextEdit::multiline(message)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );
                ui.checkbox(push, tr!("推送到远程仓库"));
                if *push {
                    remote_combo(ui, "tag-remote", remote, &remotes);
                }
                valid_ref_name(name)?;
                require(!*push || !remote.is_empty(), tr!("没有远程仓库"))
            }
            Dialog::CreateStash {
                message,
                include_untracked,
                keep_index,
            } => {
                labeled(ui, tr!("说明"), |ui| {
                    let response = ui.text_edit_singleline(message);
                    autofocus(ui, response);
                });
                ui.checkbox(include_untracked, tr!("包含未跟踪的新文件"));
                ui.checkbox(keep_index, tr!("保留已暂存的文件"));
                Ok(())
            }
            Dialog::ApplyStash {
                message,
                delete_after,
                ..
            } => {
                ui.label(message.as_str());
                ui.checkbox(delete_after, tr!("应用后删除该贮藏"));
                Ok(())
            }
            Dialog::DropStash { message, .. } => {
                ui.label(tr!(
                    "确定删除贮藏“{message}”？此操作无法撤销。",
                    message = message
                ));
                Ok(())
            }
            Dialog::Merge {
                branch,
                current,
                mode,
            } => {
                ui.label(tr!(
                    "把 {branch} 合并到 {current}",
                    branch = branch,
                    current = current
                ));
                labeled(ui, tr!("方式"), |ui| {
                    egui::ComboBox::from_id_salt("merge-mode")
                        .selected_text(merge_mode_name(*mode))
                        .show_ui(ui, |ui| {
                            for option in [
                                MergeMode::Default,
                                MergeMode::NoFastForward,
                                MergeMode::FastForwardOnly,
                                MergeMode::Squash,
                            ] {
                                ui.selectable_value(mode, option, merge_mode_name(option));
                            }
                        });
                });
                Ok(())
            }
            Dialog::Rebase {
                onto,
                current,
                autostash,
                update_refs,
            } => {
                ui.label(tr!(
                    "把 {current} 变基到 {onto}",
                    current = current,
                    onto = onto
                ));
                ui.checkbox(autostash, tr!("贮藏并重新应用本地更改 (--autostash)"));
                ui.checkbox(
                    update_refs,
                    tr!("同时更新指向被改写提交的分支 (--update-refs)"),
                );
                Ok(())
            }
            Dialog::CherryPick {
                sha,
                summary,
                append_origin,
                commit,
                parents,
                parent,
            } => {
                ui.label(format!("{} {summary}", short(sha)));
                ui.checkbox(commit, tr!("立即提交"));
                ui.checkbox(append_origin, tr!("在说明里附加原提交 SHA (-x)"));
                parent_combo(ui, "pick-parent", *parents, parent);
                Ok(())
            }
            Dialog::Revert {
                sha,
                summary,
                commit,
                parents,
                parent,
            } => {
                ui.label(format!("{} {summary}", short(sha)));
                ui.checkbox(commit, tr!("立即提交"));
                parent_combo(ui, "revert-parent", *parents, parent);
                Ok(())
            }
            Dialog::Reset {
                label,
                current,
                mode,
                ..
            } => {
                ui.label(tr!(
                    "把 {current} 重置到 {label}",
                    current = current,
                    label = label
                ));
                for (option, name, help) in [
                    (ResetMode::Soft, "Soft", tr!("保留工作区和暂存区")),
                    (ResetMode::Mixed, "Mixed", tr!("保留工作区，清空暂存区")),
                    (ResetMode::Hard, "Hard", tr!("丢弃所有本地更改")),
                ] {
                    ui.radio_value(mode, option, format!("{name} — {help}"));
                }
                if *mode == ResetMode::Hard {
                    ui.colored_label(ui.visuals().warn_fg_color, tr!("未提交的更改会丢失。"));
                }
                Ok(())
            }
            Dialog::DeleteBranch {
                name,
                upstream,
                delete_remote,
                force,
            } => {
                ui.label(tr!("确定删除本地分支 {name}？", name = name));
                ui.checkbox(force, tr!("即使未合并也删除 (-D)"));
                if let Some(upstream) = upstream {
                    ui.checkbox(
                        delete_remote,
                        tr!("同时删除远程分支 {upstream}", upstream = upstream),
                    );
                }
                Ok(())
            }
            Dialog::DeleteRemoteBranch { name } => {
                ui.label(tr!("确定从远程仓库删除 {name}？", name = name));
                Ok(())
            }
            Dialog::RenameBranch {
                old,
                new,
                upstream,
                rename_remote,
            } => {
                ui.label(tr!("原名称: {old}", old = old));
                labeled(ui, tr!("新名称"), |ui| {
                    let response = ui.text_edit_singleline(new);
                    autofocus(ui, response);
                });
                if let Some(upstream) = upstream {
                    ui.checkbox(
                        rename_remote,
                        tr!("同时重命名远程分支 {upstream}", upstream = upstream),
                    );
                }
                valid_ref_name(new)?;
                require(new != old, tr!("名称没有变化"))
            }
            Dialog::DeleteTag {
                name,
                delete_remote,
                remote,
            } => {
                ui.label(tr!("确定删除标签 {name}？", name = name));
                if !remotes.is_empty() {
                    ui.checkbox(delete_remote, tr!("同时从远程仓库删除"));
                    if *delete_remote {
                        remote_combo(ui, "tag-delete-remote", remote, &remotes);
                    }
                }
                Ok(())
            }
            Dialog::EditRemote { name, url, .. } => {
                labeled(ui, tr!("名称"), |ui| {
                    ui.text_edit_singleline(name);
                });
                labeled(ui, tr!("地址"), |ui| {
                    ui.add(egui::TextEdit::singleline(url).desired_width(320.0));
                });
                ui.weak(tr!(
                    "支持 HTTPS 和 SSH 地址，例如 git@github.com:owner/repo.git"
                ));
                valid_ref_name(name)?;
                require(!url.trim().is_empty(), tr!("请填写地址"))
            }
            Dialog::DeleteRemote { name } => {
                ui.label(tr!(
                    "确定删除远程仓库 {name}？对应的远程跟踪分支也会删除。",
                    name = name
                ));
                Ok(())
            }
            Dialog::Reword {
                subject,
                description,
            } => {
                ui.label(tr!("只修改最新提交的说明，暂存区的更改不会加入。"));
                ui.add(
                    egui::TextEdit::singleline(subject)
                        .hint_text(tr!("摘要"))
                        .desired_width(f32::INFINITY),
                );
                ui.add(
                    egui::TextEdit::multiline(description)
                        .hint_text(tr!("描述"))
                        .desired_rows(5)
                        .desired_width(f32::INFINITY),
                );
                require(!subject.trim().is_empty(), tr!("摘要不能为空"))
            }
            Dialog::Discard { changes } => {
                ui.label(tr!(
                    "确定放弃 {count} 个文件的更改？未跟踪的文件会被删除，此操作无法撤销。",
                    count = changes.len()
                ));
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        for change in changes.iter() {
                            ui.monospace(&change.path);
                        }
                    });
                Ok(())
            }
            Dialog::Identity { name, email } => {
                ui.label(tr!(
                    "提交需要作者姓名和邮箱。将写入全局 Git 配置 (git config --global)。"
                ));
                labeled(ui, tr!("姓名"), |ui| {
                    ui.text_edit_singleline(name);
                });
                labeled(ui, tr!("邮箱"), |ui| {
                    ui.text_edit_singleline(email);
                });
                require(
                    !name.trim().is_empty() && email.contains('@'),
                    tr!("请填写姓名和有效邮箱"),
                )
            }
        }
    }

    pub fn finish(&mut self, env: &DialogContext<'_>) -> Outcome {
        let root = env.root;
        let job = |title: String| GitJob::new(title, root);
        let job = match self {
            Dialog::Fetch { remote, all, prune } => {
                let mut args = vec!["fetch".to_string(), "--progress".into()];
                if *prune {
                    args.push("--prune".into());
                }
                if *all {
                    args.push("--all".into());
                } else {
                    args.push(remote.clone());
                }
                job(if *all {
                    tr!("抓取所有远程仓库").into()
                } else {
                    tr!("抓取 {remote}", remote = remote)
                })
                .step(args)
            }
            Dialog::Pull {
                remote,
                branch,
                rebase,
                autostash,
            } => {
                let mut args = vec!["pull".to_string(), "--progress".into()];
                args.push(if *rebase { "--rebase" } else { "--no-rebase" }.into());
                args.extend([remote.clone(), branch.clone()]);
                let job = job(tr!(
                    "从 {remote}/{branch} 拉取",
                    remote = remote,
                    branch = branch
                ))
                .step(args);
                if *autostash {
                    job.stash_if_blocked()
                } else {
                    job
                }
            }
            Dialog::Push {
                local,
                remote,
                remote_branch,
                track,
                force,
                tags,
            } => {
                let mut args = vec!["push".to_string(), "--progress".into()];
                if *track {
                    args.push("--set-upstream".into());
                }
                if *force {
                    args.push("--force-with-lease".into());
                }
                args.push(remote.clone());
                args.push(format!(
                    "refs/heads/{local}:refs/heads/{}",
                    remote_branch.trim()
                ));
                let mut job = job(tr!(
                    "推送 {local} 到 {remote}",
                    local = local,
                    remote = remote
                ))
                .step(args);
                if *tags {
                    job = job.step(["push", "--progress", "--tags", remote.as_str()]);
                }
                job
            }
            Dialog::CreateBranch {
                start,
                name,
                checkout,
                changes,
                ..
            } => {
                let name = name.trim().to_string();
                if *checkout {
                    switch_job(
                        job(tr!("创建并检出分支 {name}", name = name)),
                        *changes,
                        vec!["switch".into(), "-c".into(), name, start.clone()],
                    )
                } else {
                    job(tr!("创建分支 {name}", name = name)).step([
                        "branch",
                        name.as_str(),
                        start.as_str(),
                    ])
                }
            }
            Dialog::Checkout {
                target,
                label,
                detach,
                changes,
            } => {
                let mut args = vec!["switch".to_string()];
                if *detach {
                    args.push("--detach".into());
                }
                args.push(target.clone());
                switch_job(job(tr!("检出 {label}", label = label)), *changes, args)
            }
            Dialog::TrackRemote {
                remote_branch,
                local_name,
                changes,
            } => switch_job(
                job(tr!("检出 {remote_branch}", remote_branch = remote_branch)),
                *changes,
                vec![
                    "switch".into(),
                    "-c".into(),
                    local_name.trim().to_string(),
                    "--track".into(),
                    remote_branch.clone(),
                ],
            ),
            Dialog::CreateTag {
                target,
                name,
                message,
                push,
                remote,
                ..
            } => {
                let name = name.trim().to_string();
                let mut args = vec!["tag".to_string()];
                if !message.trim().is_empty() {
                    args.extend(["-a".into(), "-m".into(), message.trim().to_string()]);
                }
                args.extend([name.clone(), target.clone()]);
                let mut job = job(tr!("创建标签 {name}", name = name)).step(args);
                if *push {
                    job = job.step(vec![
                        "push".into(),
                        "--progress".into(),
                        remote.clone(),
                        format!("refs/tags/{name}"),
                    ]);
                }
                job
            }
            Dialog::CreateStash {
                message,
                include_untracked,
                keep_index,
            } => {
                let mut args = vec!["stash".to_string(), "push".into()];
                if *include_untracked {
                    args.push("--include-untracked".into());
                }
                // 否则已暂存的文件也会被收进贮藏，提交按钮会因为暂存区变空而无法点击。
                if *keep_index {
                    args.push("--keep-index".into());
                }
                if !message.trim().is_empty() {
                    args.extend(["-m".into(), message.trim().to_string()]);
                }
                job(tr!("贮藏更改").into()).step(args)
            }
            Dialog::ApplyStash {
                index,
                delete_after,
                ..
            } => {
                let verb = if *delete_after { "pop" } else { "apply" };
                job(tr!(
                    "应用贮藏 {stash}",
                    stash = format!("stash@{{{index}}}")
                ))
                .step(vec![
                    "stash".into(),
                    verb.into(),
                    format!("stash@{{{index}}}"),
                ])
            }
            Dialog::DropStash { index, .. } => job(tr!(
                "删除贮藏 {stash}",
                stash = format!("stash@{{{index}}}")
            ))
            .step(vec![
                "stash".into(),
                "drop".into(),
                format!("stash@{{{index}}}"),
            ]),
            Dialog::Merge { branch, mode, .. } => {
                let mut args = vec!["merge".to_string()];
                match mode {
                    MergeMode::Default => {}
                    MergeMode::NoFastForward => args.push("--no-ff".into()),
                    MergeMode::FastForwardOnly => args.push("--ff-only".into()),
                    MergeMode::Squash => args.push("--squash".into()),
                }
                args.push(branch.clone());
                job(tr!("合并 {branch}", branch = branch)).step(args)
            }
            Dialog::Rebase {
                onto,
                autostash,
                update_refs,
                ..
            } => {
                let mut args = vec!["rebase".to_string()];
                if *autostash {
                    args.push("--autostash".into());
                }
                if *update_refs {
                    args.push("--update-refs".into());
                }
                args.push(onto.clone());
                job(tr!("变基到 {onto}", onto = onto)).step(args)
            }
            Dialog::CherryPick {
                sha,
                append_origin,
                commit,
                parents,
                parent,
                ..
            } => {
                let mut args = vec!["cherry-pick".to_string()];
                if *append_origin {
                    args.push("-x".into());
                }
                if !*commit {
                    args.push("--no-commit".into());
                }
                if *parents > 1 {
                    args.extend(["-m".into(), (*parent + 1).to_string()]);
                }
                args.push(sha.clone());
                job(tr!("拣选 {short}", short = short(sha))).step(args)
            }
            Dialog::Revert {
                sha,
                commit,
                parents,
                parent,
                ..
            } => {
                let mut args = vec!["revert".to_string(), "--no-edit".into()];
                if !*commit {
                    args.push("--no-commit".into());
                }
                if *parents > 1 {
                    args.extend(["-m".into(), (*parent + 1).to_string()]);
                }
                args.push(sha.clone());
                job(tr!("还原 {short}", short = short(sha))).step(args)
            }
            Dialog::Reset {
                target,
                label,
                mode,
                ..
            } => {
                let flag = match mode {
                    ResetMode::Soft => "--soft",
                    ResetMode::Mixed => "--mixed",
                    ResetMode::Hard => "--hard",
                };
                job(tr!("重置到 {label}", label = label)).step(["reset", flag, target.as_str()])
            }
            Dialog::DeleteBranch {
                name,
                upstream,
                delete_remote,
                force,
            } => {
                let flag = if *force { "-D" } else { "-d" };
                let mut job =
                    job(tr!("删除分支 {name}", name = name)).step(["branch", flag, name.as_str()]);
                if *delete_remote
                    && let Some(upstream) = upstream
                    && let Some((remote, branch)) = split_remote(env.refs, upstream)
                {
                    job = job.step(["push", remote.as_str(), "--delete", branch.as_str()]);
                }
                job
            }
            Dialog::DeleteRemoteBranch { name } => {
                let Some((remote, branch)) = split_remote(env.refs, name) else {
                    return Outcome::Cancel;
                };
                job(tr!("删除远程分支 {name}", name = name)).step([
                    "push",
                    remote.as_str(),
                    "--delete",
                    branch.as_str(),
                ])
            }
            Dialog::RenameBranch {
                old,
                new,
                upstream,
                rename_remote,
            } => {
                let new = new.trim().to_string();
                let mut job = job(tr!("重命名分支 {old}", old = old)).step([
                    "branch",
                    "-m",
                    old.as_str(),
                    new.as_str(),
                ]);
                if *rename_remote
                    && let Some(upstream) = upstream
                    && let Some((remote, branch)) = split_remote(env.refs, upstream)
                {
                    job = job
                        .step(vec![
                            "push".into(),
                            "--set-upstream".into(),
                            remote.clone(),
                            format!("refs/heads/{new}:refs/heads/{new}"),
                        ])
                        .step(["push", remote.as_str(), "--delete", branch.as_str()]);
                }
                job
            }
            Dialog::DeleteTag {
                name,
                delete_remote,
                remote,
            } => {
                let mut job =
                    job(tr!("删除标签 {name}", name = name)).step(["tag", "-d", name.as_str()]);
                if *delete_remote && !remote.is_empty() {
                    job = job.step(vec![
                        "push".into(),
                        remote.clone(),
                        "--delete".into(),
                        format!("refs/tags/{name}"),
                    ]);
                }
                job
            }
            Dialog::EditRemote {
                original,
                name,
                url,
            } => {
                let name = name.trim().to_string();
                let url = url.trim().to_string();
                match original {
                    None => job(tr!("添加远程仓库 {name}", name = name)).step([
                        "remote",
                        "add",
                        name.as_str(),
                        url.as_str(),
                    ]),
                    Some(original) => {
                        let mut job = job(tr!("编辑远程仓库 {original}", original = original));
                        if *original != name {
                            job = job.step(["remote", "rename", original.as_str(), name.as_str()]);
                        }
                        job.step(["remote", "set-url", name.as_str(), url.as_str()])
                    }
                }
            }
            Dialog::DeleteRemote { name } => job(tr!("删除远程仓库 {name}", name = name)).step([
                "remote",
                "remove",
                name.as_str(),
            ]),
            Dialog::Reword {
                subject,
                description,
            } => {
                let mut args = vec![
                    "commit".to_string(),
                    "--amend".into(),
                    "--only".into(),
                    "-m".into(),
                    subject.trim().to_string(),
                ];
                if !description.trim().is_empty() {
                    args.extend(["-m".into(), description.trim().to_string()]);
                }
                job(tr!("修改提交说明").into()).step(args)
            }
            Dialog::Discard { changes } => return Outcome::Discard(std::mem::take(changes)),
            Dialog::Identity { name, email } => job(tr!("设置提交身份").into())
                .step(["config", "--global", "user.name", name.trim()])
                .step(["config", "--global", "user.email", email.trim()]),
        };
        Outcome::Job(job)
    }
}

fn switch_job(job: GitJob, changes: LocalChanges, mut args: Vec<String>) -> GitJob {
    match changes {
        LocalChanges::Keep => job.step(args),
        LocalChanges::Discard => {
            args.push("--discard-changes".into());
            job.step(args)
        }
        LocalChanges::StashAndReapply => job
            .step([
                "stash",
                "push",
                "--include-untracked",
                "-m",
                tr!("git-forker: 切换前自动贮藏"),
            ])
            .step(args)
            .step(["stash", "pop"]),
    }
}

/// 把 `origin/feature/x` 拆成远程名和分支名。远程名本身可以含斜杠。
fn split_remote(refs: &RefsSnapshot, name: &str) -> Option<(String, String)> {
    refs.remotes
        .iter()
        .filter(|remote| name.starts_with(&format!("{}/", remote.name)))
        .max_by_key(|remote| remote.name.len())
        .map(|remote| {
            (
                remote.name.clone(),
                name[remote.name.len() + 1..].to_string(),
            )
        })
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(8)]
}

fn require(ok: bool, reason: &'static str) -> Result<(), &'static str> {
    if ok { Ok(()) } else { Err(reason) }
}

fn valid_ref_name(name: &str) -> Result<(), &'static str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(tr!("名称不能为空"));
    }
    let bad = name.starts_with('-')
        || name.starts_with('.')
        || name.ends_with('/')
        || name.ends_with(".lock")
        || name.contains("..")
        || name.contains("@{")
        || name
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\".contains(c));
    require(!bad, tr!("名称包含 Git 不允许的字符"))
}

fn labeled(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.add_sized([80.0, 20.0], egui::Label::new(label));
        add(ui);
    });
}

fn combo(ui: &mut Ui, id: &str, value: &mut String, options: &[String]) {
    egui::ComboBox::from_id_salt(id)
        .width(260.0)
        .selected_text(value.as_str())
        .show_ui(ui, |ui| {
            for option in options {
                ui.selectable_value(value, option.clone(), option);
            }
        });
}

fn remote_combo(ui: &mut Ui, id: &str, value: &mut String, remotes: &[String]) {
    labeled(ui, tr!("远程仓库"), |ui| combo(ui, id, value, remotes));
}

fn parent_combo(ui: &mut Ui, id: &str, parents: usize, parent: &mut usize) {
    if parents < 2 {
        return;
    }
    labeled(ui, tr!("主线父提交"), |ui| {
        egui::ComboBox::from_id_salt(id)
            .selected_text(tr!("第 {index} 个父提交", index = *parent + 1))
            .show_ui(ui, |ui| {
                for index in 0..parents {
                    ui.selectable_value(
                        parent,
                        index,
                        tr!("第 {index} 个父提交", index = index + 1),
                    );
                }
            });
    });
}

fn local_changes_ui(ui: &mut Ui, changes: &mut LocalChanges) {
    ui.label(tr!("本地更改"));
    ui.radio_value(
        changes,
        LocalChanges::Keep,
        tr!("保留（与目标冲突时 Git 会拒绝）"),
    );
    ui.radio_value(
        changes,
        LocalChanges::StashAndReapply,
        tr!("贮藏后重新应用"),
    );
    ui.radio_value(changes, LocalChanges::Discard, tr!("丢弃"));
}

fn merge_mode_name(mode: MergeMode) -> &'static str {
    match mode {
        MergeMode::Default => tr!("默认（可快进时快进）"),
        MergeMode::NoFastForward => tr!("总是创建合并提交 (--no-ff)"),
        MergeMode::FastForwardOnly => tr!("只允许快进 (--ff-only)"),
        MergeMode::Squash => tr!("压缩为一次更改 (--squash)"),
    }
}

/// 克隆对话框属于整个应用，而不是某个仓库标签页。
pub struct CloneDialog {
    pub url: String,
    pub parent: String,
    pub name: String,
}

pub enum CloneOutcome {
    Keep,
    Cancel,
    Start(GitJob, PathBuf),
}

impl CloneDialog {
    pub fn new() -> Self {
        Self {
            url: String::new(),
            parent: dirs::home_dir()
                .map(|home| home.display().to_string())
                .unwrap_or_default(),
            name: String::new(),
        }
    }

    pub fn show(&mut self, ctx: &egui::Context) -> CloneOutcome {
        let mut outcome = CloneOutcome::Keep;
        let modal = egui::Modal::new(egui::Id::unique("clone-dialog")).show(ctx, |ui| {
            ui.set_min_width(480.0);
            ui.heading(tr!("克隆仓库"));
            ui.add_space(8.0);
            labeled(ui, tr!("仓库地址"), |ui| {
                let response =
                    ui.add(egui::TextEdit::singleline(&mut self.url).desired_width(340.0));
                if response.changed() {
                    self.name = guess_name(&self.url);
                }
            });
            labeled(ui, tr!("上级目录"), |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.parent).desired_width(270.0));
                if ui.button(tr!("选择…")).clicked()
                    && let Some(folder) = rfd::FileDialog::new().pick_folder()
                {
                    self.parent = folder.display().to_string();
                }
            });
            labeled(ui, tr!("目录名"), |ui| {
                ui.text_edit_singleline(&mut self.name);
            });
            let target = PathBuf::from(&self.parent).join(self.name.trim());
            let valid = if self.url.trim().is_empty() || self.name.trim().is_empty() {
                Err(tr!("请填写地址和目录名"))
            } else if target.exists() {
                Err(tr!("目标目录已存在"))
            } else {
                Ok(())
            };
            ui.add_space(12.0);
            ui.separator();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(valid.is_ok(), egui::Button::new(tr!("克隆")))
                    .clicked()
                {
                    let job = GitJob::new(tr!("克隆 {url}", url = self.url.trim()), &self.parent)
                        .step(["clone", "--progress", self.url.trim(), self.name.trim()]);
                    outcome = CloneOutcome::Start(job, target.clone());
                }
                if ui.button(tr!("取消")).clicked() {
                    outcome = CloneOutcome::Cancel;
                }
                if let Err(reason) = valid {
                    ui.colored_label(ui.visuals().warn_fg_color, reason);
                }
            });
        });
        if matches!(outcome, CloneOutcome::Keep) && modal.should_close() {
            outcome = CloneOutcome::Cancel;
        }
        outcome
    }
}

fn guess_name(url: &str) -> String {
    url.trim()
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or("")
        .trim_end_matches(".git")
        .to_string()
}

/// 对话框打开时把焦点放到第一个输入框，之后不再抢焦点。
fn autofocus(ui: &Ui, response: egui::Response) {
    if ui.memory(|memory| memory.focused().is_none()) {
        response.request_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use git_forker::{RemoteBranch, RemoteInfo};

    fn refs() -> RefsSnapshot {
        RefsSnapshot {
            local: Vec::new(),
            remotes: vec![RemoteInfo {
                name: "origin".into(),
                url: String::new(),
                branches: vec![RemoteBranch {
                    name: "origin/feature/x".into(),
                    branch: "feature/x".into(),
                    target: git_forker::Oid::ZERO_SHA1,
                }],
            }],
            tags: Vec::new(),
            stashes: Vec::new(),
            submodules: Vec::new(),
        }
    }

    fn job_of(mut dialog: Dialog) -> GitJob {
        let refs = refs();
        let env = DialogContext {
            root: Path::new("/repo"),
            refs: &refs,
            local_changes: 0,
        };
        match dialog.finish(&env) {
            Outcome::Job(job) => job,
            _ => panic!("应当生成任务"),
        }
    }

    #[test]
    fn push_uses_explicit_refspec_and_upstream() {
        let job = job_of(Dialog::Push {
            local: "main".into(),
            remote: "origin".into(),
            remote_branch: "release".into(),
            track: true,
            force: true,
            tags: false,
        });
        assert_eq!(
            job.steps,
            vec![vec![
                "push",
                "--progress",
                "--set-upstream",
                "--force-with-lease",
                "origin",
                "refs/heads/main:refs/heads/release"
            ]]
        );
    }

    #[test]
    fn pull_does_not_pass_autostash_and_restores_the_index_itself() {
        let job = job_of(Dialog::Pull {
            remote: "origin".into(),
            branch: "main".into(),
            rebase: false,
            autostash: true,
        });
        assert!(job.stash_if_blocked);
        assert_eq!(
            job.steps,
            vec![vec!["pull", "--progress", "--no-rebase", "origin", "main"]]
        );
    }

    #[test]
    fn stash_and_reapply_wraps_the_switch() {
        let job = job_of(Dialog::Checkout {
            target: "dev".into(),
            label: "dev".into(),
            detach: false,
            changes: LocalChanges::StashAndReapply,
        });
        let commands: Vec<_> = job.steps.iter().map(|step| step[..2].join(" ")).collect();
        assert_eq!(commands, vec!["stash push", "switch dev", "stash pop"]);
    }

    #[test]
    fn remote_branch_names_split_on_known_remote() {
        let job = job_of(Dialog::DeleteRemoteBranch {
            name: "origin/feature/x".into(),
        });
        assert_eq!(
            job.steps,
            vec![vec!["push", "origin", "--delete", "feature/x"]]
        );
    }

    #[test]
    fn ref_names_follow_git_rules() {
        assert!(valid_ref_name("feature/新功能").is_ok());
        for bad in ["", "a..b", "-x", "a b", "x.lock", "a~1", "end/"] {
            assert!(valid_ref_name(bad).is_err(), "{bad}");
        }
    }
}
