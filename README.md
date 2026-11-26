# git-forker

[English](README.md) | [简体中文](README.zh-CN.md)

A cross-platform desktop Git client. Open a repository, read the commit graph, stage changes, and run everyday Git operations from one window. Network commands and anything that should run hooks go through the `git` already installed on your machine, so existing SSH keys, credential helpers, and hooks keep working.

Without a subcommand, `git-forker` opens the graphical interface. The same binary also accepts a small set of terminal commands.

## Features

- Commit graph with local branches, remote branches, and tags
- Working tree and index: stage or unstage whole files or individual hunks, discard changes, and commit or amend
- Fetch, pull, and push, including quick actions for the current upstream
- Clone a remote repository, or create a new local one
- Create, rename, delete, and check out branches; set or clear an upstream
- Merge, rebase, cherry-pick, revert, and a basic bisect flow
- Stashes, tags, remotes, and submodules
- Side-by-side diffs, blame, and per-file history
- Several repositories open as tabs, plus a command palette and keyboard shortcuts
- Light, dark, or system appearance
- Interface languages: Simplified Chinese, Traditional Chinese, English, and Japanese, or follow the system locale

History, status, diffs, and blame are read in-process with libgit2. Operations that talk to a remote or should invoke Git hooks — commit, fetch, pull, push, merge, rebase, clone, and similar commands in the interface — are executed with the system `git`.

## Requirements

- [Rust](https://rustup.rs) stable with the 2024 edition (1.85 or newer)
- Git on `PATH`. The app looks up `git` in this order: the path set in Settings, the `GIT_FORK_GIT` environment variable, then `PATH`
- A configured `user.name` and `user.email` before committing

On Linux, the interface also needs the usual windowing libraries. On Debian or Ubuntu:

```bash
sudo apt-get install build-essential pkg-config cmake zlib1g-dev libssl-dev \
  libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libwayland-dev
```

## Build and run

```bash
cargo run
```

On macOS and Linux, `./restart.sh` rebuilds and starts the app. It stops a previous build of this repository first, so the binary can be replaced.

```bash
./restart.sh                  # debug build, open this repository
./restart.sh --release        # release build
./restart.sh -- --path /path/to/repo
```

Open another repository with `--path`. The default is the current directory.

```bash
cargo run -- --path ~/code/project
```

## Command line

These subcommands stay in the terminal. `commit` here writes the commit through libgit2 and does not run Git hooks; use the graphical interface when hooks should run.

```bash
git-forker init
git-forker status
git-forker log --max 20
git-forker add src/main.rs
git-forker commit -m "Explain the change"
git-forker branch
git-forker branch create feature
git-forker switch main
```

`--path` is global, so `git-forker --path ~/code/project status` works the same way.

## Packages

Packaging scripts live in the repository. They are not required to build from source.

| Script | Output |
| --- | --- |
| `./package-mac.sh` | `dist/git-forker.app` and a zip, on macOS |
| `./package-deb.sh` | `dist/git-forker_<version>_amd64.deb`. On Linux x86_64 it builds locally; elsewhere it builds `linux/amd64` with Docker |

On macOS, a copy downloaded from the browser is quarantined. If the system says the app is damaged or cannot be opened, clear that attribute and open it again:

```bash
xattr -dr com.apple.quarantine /Applications/git-forker.app
```

If `git-forker.app` is not in Applications, use its actual path.

There is no Windows installer script. Build with `cargo build --release` and run `target/release/git-forker`.

## Development

```bash
cargo test
cargo build --release
```

Interface copy is written in Simplified Chinese in the source. Other languages are filled in from `locales/translations.tsv`. A missing translation falls back to the source string.

## License

[MIT](LICENSE). You may use, modify, and distribute this software, including in commercial products, as long as the copyright notice and license text stay with the copies.

