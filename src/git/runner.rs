use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::RwLock;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

static GIT_OVERRIDE: RwLock<Option<String>> = RwLock::new(None);

/// 使用的 git 可执行文件。优先级：设置里指定的路径、环境变量 `GIT_FORK_GIT`、PATH 里的 `git`。
pub fn git_program() -> String {
    if let Some(path) = GIT_OVERRIDE.read().ok().and_then(|path| path.clone()) {
        return path;
    }
    std::env::var("GIT_FORK_GIT").unwrap_or_else(|_| "git".to_string())
}

/// 指定 git 可执行文件。空字符串或 `None` 表示恢复默认查找方式。
pub fn set_git_program(path: Option<&str>) {
    let path = path.map(str::trim).filter(|path| !path.is_empty());
    if let Ok(mut current) = GIT_OVERRIDE.write() {
        *current = path.map(str::to_string);
    }
}

fn git_command(cwd: &Path) -> Command {
    let mut command = Command::new(git_program());
    command
        .current_dir(cwd)
        .args(["-c", "color.ui=false", "-c", "core.quotepath=false"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_EDITOR", "true")
        .env("GIT_MERGE_AUTOEDIT", "no");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// 同步运行一条 git 命令并返回标准输出；失败时返回标准错误。
pub fn git_output(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = git_command(cwd)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| crate::tr!("无法启动 git: {err}", err = err))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        Err(if stderr.is_empty() {
            crate::tr!("git 退出状态 {status}", status = output.status)
        } else {
            stderr
        })
    }
}

/// 拉取被本地更改挡住时写入贮藏的说明。重新应用发生冲突时贮藏会留下，界面靠它认出来。
pub const PULL_STASH_MESSAGE: &str = "git-forker: 拉取前自动贮藏";

/// 后台执行的一组 git 命令。前一条失败时不再执行后面的命令。
///
/// `stash_if_blocked` 为真时，某一步因本地更改无法继续，会先贮藏再重试这一步，
/// 然后用 `stash pop --index` 按原来的暂存状态放回去。索引本身冲突时，再退回不带
/// `--index` 的弹出。这是拉取挡住本地更改时的做法，不使用 `git pull --autostash`：
/// 后者总会贮藏，并且重新应用时不恢复暂存区，没冲突的文件会被暂存或取消暂存。
#[derive(Debug, Clone)]
pub struct GitJob {
    pub title: String,
    pub cwd: PathBuf,
    pub steps: Vec<Vec<String>>,
    pub stash_if_blocked: bool,
}

impl GitJob {
    pub fn new(title: impl Into<String>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            title: title.into(),
            cwd: cwd.into(),
            steps: Vec::new(),
            stash_if_blocked: false,
        }
    }

    /// 这一步被本地更改挡住时，贮藏后重试，再按暂存区原来的样子放回去。
    pub fn stash_if_blocked(mut self) -> Self {
        self.stash_if_blocked = true;
        self
    }

    pub fn step<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.steps.push(args.into_iter().map(Into::into).collect());
        self
    }
}

#[derive(Debug, Clone)]
pub enum JobEvent {
    Output(String),
    Finished { ok: bool },
}

/// 正在运行或已结束的任务。
pub struct JobHandle {
    pub title: String,
    pub log: Vec<String>,
    pub finished: Option<bool>,
    receiver: Receiver<JobEvent>,
}

impl JobHandle {
    /// `notify` 在每条新输出到达时调用，用于唤醒界面。
    pub fn spawn(job: GitJob, notify: impl Fn() + Send + Clone + 'static) -> Self {
        let (sender, receiver) = mpsc::channel();
        let title = job.title.clone();
        thread::spawn(move || {
            let ok = run_steps(&job, &sender, &notify);
            let _ = sender.send(JobEvent::Finished { ok });
            notify();
        });
        Self {
            title,
            log: Vec::new(),
            finished: None,
            receiver,
        }
    }

    /// 收取新事件。任务刚结束时返回 `Some(成功与否)`。
    pub fn poll(&mut self) -> Option<bool> {
        let mut completed = None;
        while let Ok(event) = self.receiver.try_recv() {
            match event {
                JobEvent::Output(line) => {
                    if let Some(last) = self.log.last_mut()
                        && last.starts_with('\r')
                        && line.starts_with('\r')
                    {
                        *last = line;
                    } else {
                        self.log.push(line);
                    }
                }
                JobEvent::Finished { ok } => {
                    self.finished = Some(ok);
                    completed = Some(ok);
                }
            }
        }
        completed
    }

    pub fn is_running(&self) -> bool {
        self.finished.is_none()
    }

    pub fn last_line(&self) -> &str {
        self.log
            .iter()
            .rev()
            .map(|line| line.trim_start_matches('\r').trim())
            .find(|line| !line.is_empty())
            .unwrap_or("")
    }
}

struct StepOutcome {
    ok: bool,
    /// 标准输出和标准错误，用来判断失败是不是被本地更改挡住。
    text: String,
}

fn run_steps(
    job: &GitJob,
    sender: &Sender<JobEvent>,
    notify: &(impl Fn() + Send + Clone + 'static),
) -> bool {
    for args in &job.steps {
        let outcome = run_step(job, args, sender, notify);
        if outcome.ok {
            continue;
        }
        if job.stash_if_blocked
            && local_changes_block(&outcome.text)
            && recover(job, args, sender, notify)
        {
            continue;
        }
        return false;
    }
    true
}

/// 本地更改挡住了这一步：贮藏、重试，再按暂存区原来的样子放回去。
/// 重试本身失败时，仍尝试把贮藏放回去，避免工作区被清成干净的。
fn recover(
    job: &GitJob,
    args: &[String],
    sender: &Sender<JobEvent>,
    notify: &(impl Fn() + Send + Clone + 'static),
) -> bool {
    let _ = sender.send(JobEvent::Output(
        crate::tr!("本地更改挡住了更新，先贮藏再重新应用。").into(),
    ));
    notify();
    let stashed = run_step(
        job,
        &[
            "stash".into(),
            "push".into(),
            "--include-untracked".into(),
            "-m".into(),
            PULL_STASH_MESSAGE.into(),
        ],
        sender,
        notify,
    );
    if !stashed.ok {
        return false;
    }
    let again = run_step(job, args, sender, notify);
    reapply_stash(job, sender, notify);
    again.ok
}

/// `stash pop --index` 会分别恢复暂存区和未暂存的修改。
/// 索引对不上时，git 拒绝并提示去掉 `--index`，这时再弹出一次。
fn reapply_stash(
    job: &GitJob,
    sender: &Sender<JobEvent>,
    notify: &(impl Fn() + Send + Clone + 'static),
) {
    let popped = run_step(
        job,
        &["stash".into(), "pop".into(), "--index".into()],
        sender,
        notify,
    );
    if !popped.ok && popped.text.contains("conflicts in index") {
        let _ = run_step(job, &["stash".into(), "pop".into()], sender, notify);
    }
}

fn local_changes_block(text: &str) -> bool {
    text.contains("would be overwritten")
        || text.contains("Please commit or stash them")
        || text.contains("Please commit your changes or stash them")
}

fn run_step(
    job: &GitJob,
    args: &[String],
    sender: &Sender<JobEvent>,
    notify: &(impl Fn() + Send + Clone + 'static),
) -> StepOutcome {
    let _ = sender.send(JobEvent::Output(format!("$ git {}", args.join(" "))));
    notify();
    let child = git_command(&job.cwd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(err) => {
            let _ = sender.send(JobEvent::Output(crate::tr!(
                "无法启动 git: {err}",
                err = err
            )));
            notify();
            return StepOutcome {
                ok: false,
                text: String::new(),
            };
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let readers: Vec<_> = [
        stdout.map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
        stderr.map(|pipe| Box::new(pipe) as Box<dyn Read + Send>),
    ]
    .into_iter()
    .flatten()
    .map(|pipe| {
        let sender = sender.clone();
        let notify = notify.clone();
        thread::spawn(move || forward_lines(pipe, &sender, &notify))
    })
    .collect();
    let mut text = String::new();
    for reader in readers {
        if let Ok(part) = reader.join() {
            text.push_str(&part);
        }
    }
    match child.wait() {
        Ok(status) if status.success() => StepOutcome { ok: true, text },
        Ok(status) => {
            let _ = sender.send(JobEvent::Output(crate::tr!(
                "git 退出状态 {status}",
                status = status
            )));
            notify();
            StepOutcome { ok: false, text }
        }
        Err(err) => {
            let _ = sender.send(JobEvent::Output(crate::tr!(
                "等待 git 结束失败: {err}",
                err = err
            )));
            notify();
            StepOutcome { ok: false, text }
        }
    }
}

/// git 用 `\r` 刷新进度；进度行以 `\r` 开头，界面用它覆盖上一条进度。
fn forward_lines(
    mut pipe: Box<dyn Read + Send>,
    sender: &Sender<JobEvent>,
    notify: &impl Fn(),
) -> String {
    let mut buffer = [0u8; 4096];
    let mut pending = Vec::new();
    let mut collected = String::new();
    loop {
        let read = match pipe.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        for &byte in &buffer[..read] {
            if byte == b'\n' || byte == b'\r' {
                if !pending.is_empty() {
                    let mut line = String::from_utf8_lossy(&pending).into_owned();
                    if byte == b'\r' {
                        line.insert(0, '\r');
                    }
                    collected.push_str(line.trim_start_matches('\r'));
                    collected.push('\n');
                    let _ = sender.send(JobEvent::Output(line));
                    notify();
                    pending.clear();
                }
            } else {
                pending.push(byte);
            }
        }
    }
    if !pending.is_empty() {
        let line = String::from_utf8_lossy(&pending).into_owned();
        collected.push_str(&line);
        collected.push('\n');
        let _ = sender.send(JobEvent::Output(line));
        notify();
    }
    collected
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait(handle: &mut JobHandle) -> bool {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(ok) = handle.poll() {
                return ok;
            }
            assert!(Instant::now() < deadline, "任务超时");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn job_stops_at_first_failing_step() {
        let dir = tempfile::tempdir().unwrap();
        let job = GitJob::new("测试", dir.path())
            .step(["init", "-q"])
            .step(["rev-parse", "--verify", "no-such-ref"])
            .step(["tag", "never-created"]);
        let mut handle = JobHandle::spawn(job, || {});
        assert!(!wait(&mut handle));
        assert!(
            handle
                .log
                .iter()
                .any(|line| line.starts_with("$ git rev-parse"))
        );
        assert!(!handle.log.iter().any(|line| line.starts_with("$ git tag")));
    }

    #[test]
    fn carriage_return_lines_are_marked_as_progress() {
        let (sender, receiver) = mpsc::channel();
        let input: Box<dyn Read + Send> = Box::new(&b"a 10%\ra 50%\rdone\n"[..]);
        forward_lines(input, &sender, &|| {});
        drop(sender);
        let lines: Vec<String> = receiver
            .iter()
            .filter_map(|event| match event {
                JobEvent::Output(line) => Some(line),
                JobEvent::Finished { .. } => None,
            })
            .collect();
        assert_eq!(lines, vec!["\ra 10%", "\ra 50%", "done"]);
    }

    fn git(dir: &std::path::Path, args: &[&str]) {
        let status = git_command(dir).args(args).status().unwrap();
        assert!(status.success(), "git {args:?} 失败: {status}");
    }

    fn repo_pair() -> (tempfile::TempDir, std::path::PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let remote = root.path().join("remote");
        let local = root.path().join("local");
        std::fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "-q", "-b", "main"]);
        git(&remote, &["config", "user.email", "a@b"]);
        git(&remote, &["config", "user.name", "a"]);
        std::fs::write(remote.join("a"), "1\n2\n3\n4\n5\n").unwrap();
        std::fs::write(remote.join("b"), "1\n2\n3\n4\n5\n").unwrap();
        git(&remote, &["add", "."]);
        git(&remote, &["commit", "-qm", "init"]);
        git(root.path(), &["clone", "-q", "remote", "local"]);
        git(&local, &["config", "user.email", "a@b"]);
        git(&local, &["config", "user.name", "a"]);
        (root, local)
    }

    fn status(dir: &std::path::Path) -> String {
        git_output(dir, &["status", "--short"]).unwrap()
    }

    /// 上游没碰到本地改过的文件时，直接拉取，暂存和未暂存都保持原样。
    #[test]
    fn pull_leaves_the_index_alone_when_local_changes_do_not_block() {
        let (root, local) = repo_pair();
        std::fs::write(local.join("a"), "staged\n2\n3\n4\n5\n").unwrap();
        git(&local, &["add", "a"]);
        std::fs::write(local.join("b"), "unstaged\n2\n3\n4\n5\n").unwrap();
        std::fs::write(root.path().join("remote/c"), "new\n").unwrap();
        git(&root.path().join("remote"), &["add", "c"]);
        git(&root.path().join("remote"), &["commit", "-qm", "add c"]);

        let job = GitJob::new("拉取", &local).stash_if_blocked().step([
            "pull",
            "--no-rebase",
            "origin",
            "main",
        ]);
        let mut handle = JobHandle::spawn(job, || {});
        assert!(wait(&mut handle), "{}", handle.last_line());
        assert_eq!(status(&local), "M  a\n M b\n");
        assert!(git_output(&local, &["stash", "list"]).unwrap().is_empty());
    }

    /// 本地更改挡住更新时先贮藏，重新应用后已暂存的文件仍暂存，未暂存的仍未暂存。
    #[test]
    fn blocked_pull_reapplies_without_staging_unstaged_files() {
        let (root, local) = repo_pair();
        std::fs::write(local.join("a"), "local\n2\n3\n4\n5\n").unwrap();
        std::fs::write(local.join("b"), "staged\n2\n3\n4\n5\n").unwrap();
        git(&local, &["add", "b"]);
        std::fs::write(root.path().join("remote/a"), "1\n2\n3\n4\nupstream\n").unwrap();
        git(&root.path().join("remote"), &["commit", "-qam", "touch a"]);

        let job = GitJob::new("拉取", &local).stash_if_blocked().step([
            "pull",
            "--no-rebase",
            "origin",
            "main",
        ]);
        let mut handle = JobHandle::spawn(job, || {});
        assert!(wait(&mut handle), "{:?}", handle.log);
        assert_eq!(status(&local), " M a\nM  b\n");
        assert!(git_output(&local, &["stash", "list"]).unwrap().is_empty());
    }
}
