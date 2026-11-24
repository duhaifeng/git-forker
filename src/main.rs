use git_forker::tr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use git_forker::{HeadState, Repo};

mod gui;

#[derive(Parser)]
#[command(
    name = "git-forker",
    version,
    about = "跨平台的本地 Git 客户端。不带子命令时打开图形界面；子命令仍走终端。"
)]
struct Cli {
    /// 仓库路径。默认是当前目录。
    #[arg(long, global = true, default_value = ".")]
    path: PathBuf,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// 在指定路径创建仓库
    Init,
    /// 显示工作区和暂存区状态
    Status,
    /// 显示提交历史
    Log {
        /// 最多显示多少条提交
        #[arg(long, default_value_t = 20)]
        max: usize,
    },
    /// 把路径加入暂存区。相对路径相对于仓库工作目录
    Add {
        /// 文件或目录
        paths: Vec<PathBuf>,
    },
    /// 用暂存区内容创建提交
    Commit {
        /// 提交说明
        #[arg(short, long)]
        message: String,
    },
    /// 列出本地分支，或创建分支
    Branch {
        #[command(subcommand)]
        action: Option<BranchCommand>,
    },
    /// 切换到已有本地分支
    Switch {
        /// 分支名
        name: String,
    },
}

#[derive(Subcommand)]
enum BranchCommand {
    /// 从当前提交创建本地分支
    Create {
        /// 新分支名
        name: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = if let Some(command) = cli.command {
        run(&cli.path, command).map_err(|err| err.to_string())
    } else {
        gui::run(&cli.path).map_err(|err| err.to_string())
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{}", tr!("错误: {err}", err = err));
            ExitCode::from(1)
        }
    }
}

fn run(path: &std::path::Path, command: Command) -> Result<(), git_forker::Error> {
    match command {
        Command::Init => {
            Repo::init(path)?;
            println!("{}", tr!("已创建仓库 {path}", path = path.display()));
        }
        Command::Status => {
            let report = Repo::discover(path)?.status()?;
            println!("{}", head_line(&report.head));
            if report.entries.is_empty() {
                println!("{}", tr!("工作区是干净的"));
            } else {
                for entry in report.entries {
                    println!("{} {}", entry.code, entry.path);
                }
            }
        }
        Command::Log { max } => {
            let commits = Repo::discover(path)?.log(max)?;
            if commits.is_empty() {
                println!("{}", tr!("尚无提交"));
            } else {
                for commit in commits {
                    println!("{} {} {}", commit.id, commit.summary, commit.author);
                }
            }
        }
        Command::Add { paths } => {
            Repo::discover(path)?.add(&paths)?;
            println!("{}", tr!("已暂存 {count} 个路径", count = paths.len()));
        }
        Command::Commit { message } => {
            let id = Repo::discover(path)?.commit(&message)?;
            println!("{}", tr!("已提交 {id}", id = id));
        }
        Command::Branch { action } => {
            let repo = Repo::discover(path)?;
            match action {
                None => {
                    let branches = repo.branches()?;
                    if branches.is_empty() {
                        println!("{}", tr!("尚无分支"));
                    } else {
                        for branch in branches {
                            let marker = if branch.current { '*' } else { ' ' };
                            println!("{marker} {}", branch.name);
                        }
                    }
                }
                Some(BranchCommand::Create { name }) => {
                    repo.create_branch(&name)?;
                    println!("{}", tr!("已创建分支 {name}", name = name));
                }
            }
        }
        Command::Switch { name } => {
            Repo::discover(path)?.switch(&name)?;
            println!("{}", tr!("已切换到 {name}", name = name));
        }
    }
    Ok(())
}

fn head_line(head: &HeadState) -> String {
    match head {
        HeadState::Branch(name) => tr!("位于分支 {name}", name = name),
        HeadState::Detached(id) => tr!("HEAD 分离于 {id}", id = id),
        HeadState::Unborn => tr!("尚无提交").to_string(),
    }
}
