#!/usr/bin/env bash
# 把 git-forker 打成 linux/amd64 的 .deb。
# 在 Linux x86_64 上直接编译；在 macOS 或其他架构上通过 Docker 编译。
# 用法：./package-deb.sh
# 产物：dist/git-forker_<版本>_amd64.deb
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$root"

version=$(awk -F '"' '/^version = / { print $2; exit }' "$root/Cargo.toml")
if [ -z "$version" ]; then
  printf '无法从 Cargo.toml 读取版本号\n' >&2
  exit 1
fi

if [ -z "${DEB_MAINTAINER:-}" ]; then
  name=$(git -C "$root" config user.name 2>/dev/null || true)
  email=$(git -C "$root" config user.email 2>/dev/null || true)
  if [ -n "$name" ] && [ -n "$email" ]; then
    DEB_MAINTAINER="$name <$email>"
  else
    DEB_MAINTAINER="git-forker maintainers <git-forker@localhost>"
  fi
fi

build_deps='build-essential ca-certificates cmake pkg-config zlib1g-dev libssl-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libxkbcommon-dev libwayland-dev dpkg-dev file binutils'

install_build_deps() {
  if [ "$(id -u)" -ne 0 ]; then
    printf '缺少编译依赖。请用 root 安装：\n  sudo apt-get install %s\n' "$build_deps" >&2
    exit 1
  fi
  export DEBIAN_FRONTEND=noninteractive
  apt-get update
  # shellcheck disable=SC2086
  apt-get install -y --no-install-recommends $build_deps
}

ensure_native_tools() {
  missing=
  for cmd in cargo dpkg-deb pkg-config strip file; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
      missing="$missing $cmd"
    fi
  done
  if [ -n "$missing" ]; then
    printf '缺少工具：%s\nDebian/Ubuntu 可安装：\n  sudo apt-get install %s\n并安装 Rust：https://rustup.rs\n' "$missing" "$build_deps" >&2
    exit 1
  fi
  if ! pkg-config --exists xcb xkbcommon 2>/dev/null; then
    printf '缺少图形界面开发库。请安装：\n  sudo apt-get install %s\n' "$build_deps" >&2
    exit 1
  fi
}

# 清空 dist 里的全部内容，但保留目录本身（Docker 挂载点不能删掉）。
clean_dist() {
  mkdir -p "$root/dist"
  find "$root/dist" -mindepth 1 -delete
}

pack_deb() {
  clean_dist
  if [ -z "${CARGO_TARGET_DIR:-}" ]; then
    export CARGO_TARGET_DIR="$root/target"
  fi
  printf '正在为 x86_64-unknown-linux-gnu 编译发布版本…\n'
  cargo build --release --locked --target x86_64-unknown-linux-gnu

  bin="$CARGO_TARGET_DIR/x86_64-unknown-linux-gnu/release/git-forker"
  if [ ! -f "$bin" ]; then
    printf '没有找到编译产物：%s\n' "$bin" >&2
    exit 1
  fi
  if command -v file >/dev/null 2>&1 && ! file "$bin" | grep -q 'x86-64'; then
    printf '产物不是 amd64 可执行文件：%s\n' "$(file "$bin")" >&2
    exit 1
  fi

  stage=$(mktemp -d)
  shlib_work=$(mktemp -d)
  trap 'rm -rf "$stage" "$shlib_work"' EXIT
  install -d -m 0755 \
    "$stage/DEBIAN" \
    "$stage/usr/bin" \
    "$stage/usr/share/applications" \
    "$stage/usr/share/icons/hicolor/scalable/apps"
  install -m 0755 "$bin" "$stage/usr/bin/git-forker"
  strip --strip-unneeded "$stage/usr/bin/git-forker"

  cat > "$stage/usr/share/applications/git-forker.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=git-forker
GenericName=Git Client
Comment=跨平台的本地 Git 客户端
Exec=git-forker
Icon=git-forker
Terminal=false
Categories=Development;RevisionControl;
Keywords=git;
StartupWMClass=git-forker
EOF

  install -m 0644 "$root/assets/git-forker.svg" \
    "$stage/usr/share/icons/hicolor/scalable/apps/git-forker.svg"

  shlibs=
  if command -v dpkg-shlibdeps >/dev/null 2>&1; then
    # dpkg-shlibdeps 会读取当前目录的 debian/control，不能放进打包目录。
    mkdir -p "$shlib_work/debian"
    printf 'Source: git-forker\n\nPackage: git-forker\nArchitecture: amd64\n' > "$shlib_work/debian/control"
    set +e
    shlibs_out=$(
      cd "$shlib_work"
      dpkg-shlibdeps --ignore-missing-info -O "$stage/usr/bin/git-forker" 2>"$shlib_work/shlibdeps.err"
    )
    shlibs_status=$?
    set -e
    if [ "$shlibs_status" -ne 0 ] && [ -s "$shlib_work/shlibdeps.err" ]; then
      printf 'dpkg-shlibdeps 提示：\n' >&2
      cat "$shlib_work/shlibdeps.err" >&2
    fi
    shlibs=$(printf '%s\n' "$shlibs_out" | sed -n 's/^shlibs:Depends=//p')
  fi
  if [ -n "$shlibs" ]; then
    depends="git, $shlibs"
  else
    depends="git, libc6 (>= 2.34), zlib1g, libgcc-s1, libstdc++6"
  fi

  installed_size=$(du -sk "$stage/usr" | awk '{print $1}')
  cat > "$stage/DEBIAN/control" <<EOF
Package: git-forker
Version: $version
Section: vcs
Priority: optional
Architecture: amd64
Maintainer: $DEB_MAINTAINER
Installed-Size: $installed_size
Depends: $depends
Recommends: libvulkan1, mesa-vulkan-drivers, libfontconfig1, libx11-6, libxkbcommon0, libwayland-client0
Homepage: https://coding.jd.com/duhaifeng/git-fork
Description: cross-platform local Git client
 图形界面的本地 Git 客户端，仓库操作通过系统自带的 git 完成。
EOF

  chmod 755 "$stage"
  out="$root/dist/git-forker_${version}_amd64.deb"
  deb_args=
  if dpkg-deb --help 2>&1 | grep -q root-owner-group; then
    deb_args=--root-owner-group
  fi
  # shellcheck disable=SC2086
  dpkg-deb $deb_args --build "$stage" "$out"
  if [ -n "${HOST_UID:-}" ] && [ -n "${HOST_GID:-}" ]; then
    chown "$HOST_UID:$HOST_GID" "$out" "$root/dist" 2>/dev/null || true
  fi
  printf '已生成 %s\n' "$out"
}

run_in_docker() {
  if ! command -v docker >/dev/null 2>&1; then
    printf '当前系统不是 Linux amd64，需要 Docker 才能打 amd64 deb。\n' >&2
    exit 1
  fi
  if ! docker info >/dev/null 2>&1; then
    printf 'Docker 没有在运行。请先启动 Docker 后再执行 ./package-deb.sh\n' >&2
    exit 1
  fi
  printf '使用 Docker（linux/amd64）打包…\n'
  mkdir -p "$root/dist"
  # 只挂载源码和产物目录，避免把本机 target 送进容器。
  docker run --rm --platform linux/amd64 \
    -e IN_DOCKER=1 \
    -e DEB_MAINTAINER="$DEB_MAINTAINER" \
    -e HOST_UID="$(id -u)" \
    -e HOST_GID="$(id -g)" \
    -e CARGO_TARGET_DIR=/tmp/git-forker-target \
    -v "$root/Cargo.toml:/work/Cargo.toml:ro" \
    -v "$root/Cargo.lock:/work/Cargo.lock:ro" \
    -v "$root/src:/work/src:ro" \
    -v "$root/assets:/work/assets:ro" \
    -v "$root/package-deb.sh:/work/package-deb.sh:ro" \
    -v "$root/dist:/work/dist" \
    -v git-forker-cargo-registry:/usr/local/cargo/registry \
    -v git-forker-cargo-git:/usr/local/cargo/git \
    -w /work \
    rust:1-bookworm \
    bash /work/package-deb.sh --inside
}

if [ "${1:-}" = "--inside" ] || [ "${IN_DOCKER:-}" = "1" ]; then
  if [ "$(uname -s)" != "Linux" ] || [ "$(uname -m)" != "x86_64" ]; then
    printf '打包步骤必须在 Linux x86_64 环境中运行。\n' >&2
    exit 1
  fi
  if [ "${IN_DOCKER:-}" = "1" ]; then
    install_build_deps
  else
    ensure_native_tools
  fi
  if command -v rustup >/dev/null 2>&1; then
    rustup target add x86_64-unknown-linux-gnu
  fi
  pack_deb
  exit 0
fi

if [ "$(uname -s)" = "Linux" ] && [ "$(uname -m)" = "x86_64" ]; then
  ensure_native_tools
  if command -v rustup >/dev/null 2>&1; then
    rustup target add x86_64-unknown-linux-gnu
  fi
  pack_deb
else
  run_in_docker
fi
