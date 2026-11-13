#!/usr/bin/env bash
# Installs mcbench-lite and starts it:
#   curl -fsSL https://raw.githubusercontent.com/adonis-singh/mcbench-lite/main/install.sh | bash
#
# It checks what only you can install, and git, and that bubblewrap can make
# its sandbox, as ./start does. It clones the repository into MCBENCH_DIR
# (default ~/mcbench-lite), or updates it with `git pull --ff-only` when that
# folder already is this repository, then runs ./start there. Arguments go to ./start (`| bash -s -- --no-open`).
#
# Every line is a function until the last, which calls main: a download cut
# short runs nothing.
#
# For the tests only: MCBENCH_REPO is the repository cloned.
set -euo pipefail

# --- prerequisites: the same in start and install.sh (test/start-script.test.js checks) ---

# The platform as Node.js names it: darwin or linux, arm64 or x64. Any other
# is refused.
detect_platform() {
  case "$(uname -s)" in
    Darwin) os=darwin ;;
    Linux) os=linux ;;
    *) os= ;;
  esac
  case "$(uname -m)" in
    arm64 | aarch64) arch=arm64 ;;
    x86_64 | amd64) arch=x64 ;;
    *) arch= ;;
  esac
  if [ -z "$os" ] || [ -z "$arch" ]; then
    echo "mcbench-lite runs on macOS and Linux, on arm64 or x64; this is $(uname -s) $(uname -m)." >&2
    exit 1
  fi
}

# What only the user can install. On macOS the Xcode Command Line Tools give
# cc, which rustc links with, git and python3. On Linux the server needs cc,
# python3 (the Claude Code and Codex CLI harnesses' MCP bridge),
# /usr/bin/bwrap and /usr/bin/prlimit (server/build-sandbox.js), and start
# needs curl and xz for the Node.js download. Each argument is one more
# command to require (install.sh requires git). Exits 1, saying what is
# missing and the command that installs it, when anything is.
check_prerequisites() {
  if [ "$os" = darwin ]; then
    if ! xcode-select -p > /dev/null 2>&1; then
      echo "mcbench-lite needs the Xcode Command Line Tools (they give cc, git and python3). Install them with:" >&2
      echo "  xcode-select --install" >&2
      echo "Then run this again." >&2
      exit 1
    fi
    return
  fi
  local system_root="${MCBENCH_SYSTEM_ROOT:-}" manager= sudo= missing= packages= item name package
  if command -v apt-get > /dev/null; then manager=apt-get
  elif command -v dnf > /dev/null; then manager=dnf
  elif command -v pacman > /dev/null; then manager=pacman
  fi
  [ "$EUID" -eq 0 ] || sudo='sudo '
  for item in cc python3 curl xz /usr/bin/bwrap /usr/bin/prlimit "$@"; do
    case "$item" in
      /*) [ -x "$system_root$item" ] && continue ;;
      *) command -v "$item" > /dev/null && continue ;;
    esac
    case "$item" in
      cc) name='a C compiler (cc)'; package=gcc; [ "$manager" = apt-get ] && package=build-essential ;;
      python3) name=python3; package=python3; [ "$manager" = pacman ] && package=python ;;
      xz) name=xz; package=xz; [ "$manager" = apt-get ] && package=xz-utils ;;
      /usr/bin/bwrap) name='bubblewrap (/usr/bin/bwrap)'; package=bubblewrap ;;
      /usr/bin/prlimit) name='prlimit (/usr/bin/prlimit, from util-linux)'; package=util-linux ;;
      *) name="$item"; package="$item" ;;
    esac
    missing="$missing${missing:+, }$name"
    packages="$packages${packages:+ }$package"
  done
  [ -z "$missing" ] && return
  echo "mcbench-lite needs these, which are not installed: $missing." >&2
  case "$manager" in
    apt-get) echo "Install them with:"$'\n'"  ${sudo}apt-get update && ${sudo}apt-get install -y $packages" >&2 ;;
    dnf) echo "Install them with:"$'\n'"  ${sudo}dnf install -y $packages" >&2 ;;
    pacman) echo "Install them with:"$'\n'"  ${sudo}pacman -S --needed $packages" >&2 ;;
    *) echo "Install them with your system's package manager (the packages are usually called: $packages)." >&2 ;;
  esac
  echo "Then run this again." >&2
  exit 1
}

# Build programs run under bubblewrap in their own user, PID, network, IPC,
# UTS and cgroup namespaces with a new /proc and /dev (server/build-sandbox.js).
# This runs `true` in such a sandbox; when bubblewrap cannot make it, exits 1
# with bubblewrap's message and, for each kernel setting known to forbid
# unprivileged user namespaces that is set so, the command that allows them.
check_sandbox() {
  [ "$os" = linux ] || return 0
  local system_root="${MCBENCH_SYSTEM_ROOT:-}" sudo= error
  error="$("$system_root/usr/bin/bwrap" --unshare-user --unshare-ipc --unshare-pid --unshare-net --unshare-uts --unshare-cgroup \
    --die-with-parent --ro-bind / / --proc /proc --dev /dev -- true 2>&1)" && return
  [ "$EUID" -eq 0 ] || sudo='sudo '
  echo "mcbench-lite runs build programs in a bubblewrap sandbox, which cannot be made here. bubblewrap says:" >&2
  echo "  $error" >&2
  # When the sysctl $1 is set to $2, says what that does ($5) and how to set
  # it to $3 for good, in /etc/sysctl.d/$4.
  remedy() {
    local file="$system_root/proc/sys/${1//.//}"
    [ -r "$file" ] && [ "$(< "$file")" = "$2" ] || return 0
    echo "$5 ($1=$2). To allow them from now on:" >&2
    echo "  echo $1=$3 | ${sudo}tee /etc/sysctl.d/$4 && ${sudo}sysctl -p /etc/sysctl.d/$4" >&2
  }
  # Ubuntu 24.04 and later: an unconfined program may make a user namespace
  # but has no capabilities in it. Lifting this applies to every program.
  remedy kernel.apparmor_restrict_unprivileged_userns 1 0 60-apparmor-namespace.conf 'AppArmor restricts unprivileged user namespaces'
  # Debian's own switch.
  remedy kernel.unprivileged_userns_clone 0 1 60-userns.conf 'The kernel forbids unprivileged user namespaces'
  remedy user.max_user_namespaces 0 1000 60-max-userns.conf 'The kernel allows no user namespaces'
  echo "Then run this again." >&2
  exit 1
}

# --- end of prerequisites ---

main() {
  local repo="${MCBENCH_REPO:-https://github.com/adonis-singh/mcbench-lite.git}"
  local dir="${MCBENCH_DIR:-$HOME/mcbench-lite}"
  local shown="$dir"
  case "$dir" in "$HOME"/*) shown="~${dir#"$HOME"}" ;; esac
  detect_platform
  check_prerequisites git
  check_sandbox
  if [ ! -e "$dir" ]; then
    echo "Cloning mcbench-lite into $shown"
    git clone --quiet "$repo" "$dir"
  elif [ "$(git -C "$dir" rev-parse --show-toplevel 2> /dev/null)" = "$(cd "$dir" && pwd -P)" ] \
    && [ "$(git -C "$dir" remote get-url origin)" = "$repo" ]; then
    echo "Updating mcbench-lite in $shown"
    git -C "$dir" pull --quiet --ff-only
  else
    echo "$shown already exists and is not a clone of $repo. Move it away, or set MCBENCH_DIR to another folder, and run this again." >&2
    exit 1
  fi
  echo "To start mcbench-lite again later, run: $shown/start"
  exec "$dir/start" "$@"
}

main "$@"
