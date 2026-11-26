# git-forker

[English](README.md) | [简体中文](README.zh-CN.md)

跨平台的本地 Git 客户端。在一个窗口里打开仓库、查看提交图、暂存更改，并完成日常的 Git 操作。需要访问网络或执行钩子的命令，交给本机已经安装的 `git` 去跑，因此现有的 SSH 密钥、凭据助手和钩子都会继续生效。

不带子命令时，`git-forker` 打开图形界面。同一个程序也可以在终端里执行一小部分命令。

## 功能

- 提交图，包含本地分支、远程分支和标签
- 工作区与暂存区：按文件或按差异块暂存、取消暂存、丢弃更改，以及提交和修改上次提交
- 抓取、拉取、推送，并对当前上游提供快速操作
- 克隆远程仓库，或新建本地仓库
- 创建、重命名、删除和检出分支，设置或取消上游
- 合并、变基、拣选、还原，以及基本的二分查找
- 贮藏、标签、远程仓库和子模块
- 差异对比、逐行追溯，以及单个文件的历史
- 多个仓库以标签页打开，并提供命令面板和键盘快捷键
- 浅色、深色，或跟随系统的外观
- 界面语言：简体中文、繁体中文、英文、日文，或跟随系统

历史、状态、差异和逐行追溯在进程内通过 libgit2 读取。会访问远程仓库、或应该触发 Git 钩子的操作（界面里的提交、抓取、拉取、推送、合并、变基、克隆等）由系统自带的 `git` 执行。

## 环境要求

- 支持 2024 edition 的 [Rust](https://rustup.rs) stable（1.85 或更新版本）
- `PATH` 中能找到 Git。程序按这个顺序查找：设置里指定的路径、环境变量 `GIT_FORK_GIT`、然后是 `PATH`
- 提交前需要配置 `user.name` 和 `user.email`

在 Linux 上，图形界面还需要窗口系统的开发库。Debian 或 Ubuntu：

```bash
sudo apt-get install build-essential pkg-config cmake zlib1g-dev libssl-dev \
  libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libwayland-dev
```

## 编译和运行

```bash
cargo run
```

在 macOS 和 Linux 上，`./restart.sh` 会重新编译并启动。它会先停掉本仓库之前编译出来的进程，这样新的可执行文件才能覆盖旧文件。

```bash
./restart.sh                  # 调试构建，并打开本仓库
./restart.sh --release        # 发布构建
./restart.sh -- --path /path/to/repo
```

用 `--path` 打开其他仓库。默认是当前目录。

```bash
cargo run -- --path ~/code/project
```

## 命令行

下面这些子命令留在终端里执行。这里的 `commit` 通过 libgit2 写入提交，不会运行 Git 钩子；需要钩子时请用图形界面提交。

```bash
git-forker init
git-forker status
git-forker log --max 20
git-forker add src/main.rs
git-forker commit -m "说明这次更改"
git-forker branch
git-forker branch create feature
git-forker switch main
```

`--path` 对所有子命令都有效，例如 `git-forker --path ~/code/project status`。

## 打包

仓库里带了打包脚本。从源码编译时不必使用它们。

| 脚本 | 产物 |
| --- | --- |
| `./package-mac.sh` | 在 macOS 上生成 `dist/git-forker.app` 和对应的 zip |
| `./package-deb.sh` | 生成 `dist/git-forker_<版本>_amd64.deb`。在 Linux x86_64 上本地编译；在其他系统上通过 Docker 编译 `linux/amd64` |

在 macOS 上，从浏览器下载的应用会被标上隔离属性。如果系统提示应用已损坏或无法打开，执行下面的命令去掉隔离属性，然后再打开：

```bash
xattr -dr com.apple.quarantine /Applications/git-forker.app
```

如果 `git-forker.app` 不在「应用程序」里，把路径换成它的实际位置。

没有 Windows 安装包脚本。在 Windows 上执行 `cargo build --release`，然后运行 `target/release/git-forker`。

## 开发

```bash
cargo test
cargo build --release
```

界面文案在源码里使用简体中文。其他语言的译文放在 `locales/translations.tsv`。缺少译文时显示源码里的原文。

## 许可证

[MIT](LICENSE)。可以自由使用、修改和分发，包括用于商业产品，只需在副本中保留版权声明和许可证文本。
