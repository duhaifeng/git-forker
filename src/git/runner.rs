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

/// 后台执行的一组 git 命令。前一条失败时不再执行后面的命令。
#[derive(Debug, Clone)]
pub struct GitJob {
    pub title: String,
    pub cwd: PathBuf,
    pub steps: Vec<Vec<String>>,
}

impl GitJob {
    pub fn new(title: impl Into<String>, cwd: impl Into<PathBuf>) -> Self {
        Self {
            title: title.into(),
            cwd: cwd.into(),
            steps: Vec::new(),
        }
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

fn run_steps(
    job: &GitJob,
    sender: &Sender<JobEvent>,
    notify: &(impl Fn() + Send + Clone + 'static),
) -> bool {
    for args in &job.steps {
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
                return false;
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
        for reader in readers {
            let _ = reader.join();
        }
        match child.wait() {
            Ok(status) if status.success() => {}
            Ok(status) => {
                let _ = sender.send(JobEvent::Output(crate::tr!(
                    "git 退出状态 {status}",
                    status = status
                )));
                return false;
            }
            Err(err) => {
                let _ = sender.send(JobEvent::Output(crate::tr!(
                    "等待 git 结束失败: {err}",
                    err = err
                )));
                return false;
            }
        }
    }
    true
}

/// git 用 `\r` 刷新进度；进度行以 `\r` 开头，界面用它覆盖上一条进度。
fn forward_lines(mut pipe: Box<dyn Read + Send>, sender: &Sender<JobEvent>, notify: &impl Fn()) {
    let mut buffer = [0u8; 4096];
    let mut pending = Vec::new();
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
        let _ = sender.send(JobEvent::Output(
            String::from_utf8_lossy(&pending).into_owned(),
        ));
        notify();
    }
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
}
