#!/bin/sh
# mcbench-lite installer for macOS and Linux, run as
#   curl -fsSL https://adonis-singh.github.io/mcbench-lite/install.sh | sh
# and removed with
#   curl -fsSL https://adonis-singh.github.io/mcbench-lite/install.sh | sh -s -- --uninstall
#
# macOS: ~/Applications/mcbench-lite.app. Linux (glibc, x86_64 or aarch64):
# the payload folder ~/.local/lib/mcbench-lite, the symlink
# ~/.local/bin/mcbench-lite, and the desktop entry and icon from the archive's
# share/ folder under ${XDG_DATA_HOME:-~/.local/share}. The archive is checked
# against the release's SHA256SUMS before anything is unpacked, and an existing
# install is renamed aside and removed only after the new one is in place.
# No sudo, except on Ubuntu 23.10 and later, where it asks once whether to
# install Ubuntu's AppArmor profile for bubblewrap (ubuntu_bwrap_profile).
# --uninstall removes what this script installed, what macOS keeps for the
# app's bundle identifier (with its Launch Services entry and Metal shader
# cache), ~/.cache/mcbench and the app's leftover temporary folders, and
# names what it keeps: the data folder, Claude Code's transcripts of the
# app's chats, the Codex sign-in file and Ubuntu's profile for bwrap.
#
# MCBENCH_LITE_RELEASES names the folder the release assets are fetched from.
# The whole body is one function called on the last line, so a download cut
# off part way runs nothing.

main() {
  set -u
  releases=${MCBENCH_LITE_RELEASES:-https://github.com/adonis-singh/mcbench-lite/releases/latest/download}
  uninstall=0
  open_app=1
  for arg in "$@"; do
    case "$arg" in
      --uninstall) uninstall=1 ;;
      --no-open) open_app=0 ;;
      *) die "unknown argument $arg (the arguments are --uninstall and --no-open)" ;;
    esac
  done

  os=$(uname -s)
  case "$os" in
    Darwin)
      # hw.optional.arm64 is 1 on Apple silicon, also when this shell runs
      # translated by Rosetta, and absent on Intel.
      if [ "$(sysctl -n hw.optional.arm64 2>/dev/null)" = 1 ]; then
        asset=mcbench-lite-macos-aarch64.tar.gz
      else
        asset=mcbench-lite-macos-x86_64.tar.gz
      fi
      parent=$HOME/Applications
      payload=$parent/mcbench-lite.app
      exe=$payload/Contents/MacOS/mcbench-lite
      data_dir="$HOME/Library/Application Support/mcbench-lite"
      tmp_dir=${TMPDIR:-$(getconf DARWIN_USER_TEMP_DIR)}
      ;;
    Linux)
      cpu=$(uname -m)
      case "$cpu" in
        x86_64 | aarch64) ;;
        *) die "there is no Linux build for $cpu; the builds are for x86_64 and aarch64" ;;
      esac
      [ -e "/lib/ld-musl-$cpu.so.1" ] && die "this system uses the musl C library and mcbench-lite is built for glibc systems, so it cannot run here"
      asset=mcbench-lite-linux-$cpu.tar.gz
      parent=$HOME/.local/lib
      payload=$parent/mcbench-lite
      exe=$payload/mcbench-lite
      bin_link=$HOME/.local/bin/mcbench-lite
      share=${XDG_DATA_HOME:-$HOME/.local/share}
      data_dir=$share/mcbench-lite
      tmp_dir=${TMPDIR:-/tmp}
      ;;
    *) die "this script installs on macOS and Linux, and this system is $os; on Windows use the installer or winget" ;;
  esac

  if [ "$uninstall" = 1 ]; then
    remove_install
  else
    install_release
  fi
}

die() {
  echo "mcbench-lite install: $*" >&2
  exit 1
}

sha256_of() {
  if [ "$os" = Darwin ]; then
    shasum -a 256 "$1" | cut -d ' ' -f 1
  else
    sha256sum "$1" | cut -d ' ' -f 1
  fi
}

fetch() {
  curl -fsL -o "$2" "$releases/$1" || die "could not download $releases/$1 (curl exit $?)"
}

install_release() {
  command -v curl >/dev/null || die "curl is not installed"
  if [ -e "$payload" ] && app_running; then
    die "mcbench-lite is running from $exe; quit it and run the command again (the app updates itself, so reinstalling is not needed for a new version)"
  fi
  mkdir -p "$parent" || die "could not create $parent"
  stage=$(mktemp -d "$parent/.mcbench-lite-install.XXXXXX") || die "could not create a folder in $parent"
  # On a refusal this also removes the folders mkdir -p made for nothing.
  trap 'rm -rf "$stage"; prune_empty "$parent" "$HOME"' EXIT
  trap 'exit 1' HUP INT TERM

  echo "Downloading $releases/$asset"
  fetch "$asset" "$stage/$asset"
  fetch SHA256SUMS "$stage/SHA256SUMS"
  want=$(awk -v f="$asset" '$2 == f { print $1 }' "$stage/SHA256SUMS")
  [ -n "$want" ] || die "$asset is not listed in the release's SHA256SUMS"
  [ "$(sha256_of "$stage/$asset")" = "$want" ] || die "the sha256 of the downloaded $asset does not match the release's SHA256SUMS, so nothing was installed"
  echo "Checked the sha256 of $asset against SHA256SUMS"

  mkdir "$stage/new" || die "could not create a folder in $stage"
  tar -xzf "$stage/$asset" -C "$stage/new" 2>/dev/null || die "could not unpack $asset"
  new=$stage/new/$(basename "$payload")
  [ -d "$new" ] || die "$asset holds no $(basename "$payload") at its root"
  if [ "$os" = Linux ]; then
    [ -d "$new/share" ] || die "$asset holds no mcbench-lite/share folder"
    needs_linux_libraries "$new/mcbench-lite"
  fi

  swap_in "$new"

  if [ "$os" = Darwin ]; then
    echo "Installed $payload"
    if [ "$open_app" = 1 ]; then
      open "$payload" || die "installed $payload but could not start it with open"
      echo "Started mcbench-lite"
    fi
  else
    echo "Installed $payload"
    place_linux_files
    echo "Linked $bin_link to $exe"
    case ":${PATH:-}:" in
      *":$HOME/.local/bin:"*) ;;
      *) echo "$HOME/.local/bin is not on your PATH, so a terminal will not find mcbench-lite by name; add it to PATH in your shell's startup file, or start mcbench-lite from your applications menu" ;;
    esac
    command -v bwrap >/dev/null || echo "bubblewrap is not installed, and mcbench-lite needs it for the model's bash shell: install the package bubblewrap with your distribution's package manager"
    ubuntu_bwrap_profile
    echo "Start mcbench-lite from your applications menu, or run: mcbench-lite"
  fi
  echo "To uninstall: curl -fsSL https://adonis-singh.github.io/mcbench-lite/install.sh | sh -s -- --uninstall"
}

# Ubuntu 23.10 and later restrict unprivileged user namespaces through
# AppArmor (kernel.apparmor_restrict_unprivileged_userns=1), and bubblewrap,
# which the model's bash and the build tool run in, then works only under
# Ubuntu's own profile for it, bwrap-userns-restrict (the profiles bwrap and
# unpriv_bwrap): loaded by default from 26.04 on, and before that shipped
# unloaded in the apparmor-profiles package. The .deb loads a profile for its
# programs in /usr; this install's programs are in the home folder, and a
# profile for a path the user can write would be unsafe, so where Ubuntu's is
# not loaded this asks once, on the terminal (stdin is curl's pipe), whether
# to install that one file from the system's own package with sudo. No, no
# terminal, or a failure leaves the install as it is and says what does not
# work and the command that fixes it.
ubuntu_bwrap_profile() {
  [ "$(cat /proc/sys/kernel/apparmor_restrict_unprivileged_userns 2>/dev/null)" = 1 ] || return 0
  # shellcheck disable=SC1091 # the system's own file
  version=$(. /etc/os-release && [ "$ID" = ubuntu ] && echo "$VERSION_ID") || return 0
  major=${version%%.*} minor=${version#*.}
  [ "$major" -gt 23 ] || { [ "$major" = 23 ] && [ "$minor" -ge 10 ]; } || return 0
  bwrap_profile_loaded && return 0
  echo "The model's bash shell and build tool run in bubblewrap, which this Ubuntu's AppArmor blocks unless Ubuntu's profile for bwrap is loaded"
  answer=
  if (true </dev/tty) 2>/dev/null; then
    printf "Install Ubuntu's profile for bwrap, /etc/apparmor.d/bwrap-userns-restrict from its apparmor-profiles package, with sudo? [y/N] " >/dev/tty
    read -r answer </dev/tty
  fi
  case "$answer" in
    y | Y | yes | Yes) install_bwrap_profile && return 0 ;;
  esac
  # The same steps as install_bwrap_profile. Not `apt install apparmor-profiles`,
  # which also puts its 21 other profiles into /etc/apparmor.d.
  # shellcheck disable=SC2016 # a command printed for the user to run
  fix='(cd "$(mktemp -d)" && apt-get download apparmor-profiles && dpkg-deb -x apparmor-profiles_*.deb . && sudo install -m 0644 usr/share/apparmor/extra-profiles/bwrap-userns-restrict /etc/apparmor.d/bwrap-userns-restrict && sudo apparmor_parser -r /etc/apparmor.d/bwrap-userns-restrict)'
  echo "Without that profile the model's bash and build will not work. To add it later: $fix"
}

# Whether a profile named bwrap is loaded. The profile list itself
# (/sys/kernel/security/apparmor/profiles) is root's, but each loaded
# profile's name file under policy/profiles is readable by every user.
bwrap_profile_loaded() {
  for f in /sys/kernel/security/apparmor/policy/profiles/*/name; do
    [ "$(cat "$f" 2>/dev/null)" = bwrap ] && return 0
  done
  return 1
}

# The file exactly as Ubuntu ships it: from the installed apparmor-profiles
# package, or else from that package for this release, fetched from the
# system's own apt sources into the stage folder and unpacked there.
install_bwrap_profile() {
  file=/usr/share/apparmor/extra-profiles/bwrap-userns-restrict
  if [ ! -f "$file" ]; then
    mkdir "$stage/apparmor-profiles" || return 1
    (cd "$stage/apparmor-profiles" && apt-get -qq download apparmor-profiles) || return 1
    dpkg-deb -x "$stage/apparmor-profiles"/apparmor-profiles_*.deb "$stage/apparmor-profiles/root" || return 1
    file=$stage/apparmor-profiles/root$file
  fi
  sudo install -m 0644 "$file" /etc/apparmor.d/bwrap-userns-restrict || return 1
  sudo apparmor_parser -r /etc/apparmor.d/bwrap-userns-restrict || return 1
  echo "Installed /etc/apparmor.d/bwrap-userns-restrict from Ubuntu's apparmor-profiles package and loaded it"
}

# Refuses, before anything is swapped in, when the unpacked executable links
# against a shared library this system lacks (as ldd reports it), since the
# app could not start. bubblewrap is only a notice later, since the app
# starts without it.
needs_linux_libraries() {
  libs=$(ldd "$1" 2>/dev/null | awk '$2 == "=>" && $3 == "not" { printf "%s%s", sep, $1; sep = ", " }')
  [ -z "$libs" ] || die "nothing was installed, because mcbench-lite needs the libraries $libs, which ldd reports not found on this system; install them from your distribution's packages and run the command again"
}

# Puts the unpacked payload folder at $payload with renames only: an existing
# install moves into the stage folder first and goes back if the new one
# cannot be moved in. The stage folder, with the old install in it, is
# removed on exit.
swap_in() {
  old=
  if [ -e "$payload" ] || [ -L "$payload" ]; then
    old=$stage/old
    mv "$payload" "$old" 2>/dev/null || die "could not move the existing $payload aside"
  fi
  if ! mv "$1" "$payload" 2>/dev/null; then
    [ -n "$old" ] && mv "$old" "$payload"
    die "could not move the new install into $payload; the previous install is unchanged"
  fi
}

place_linux_files() {
  mkdir -p "$(dirname "$bin_link")" || die "could not create $(dirname "$bin_link")"
  ln -sfn "$exe" "$bin_link" || die "could not create the symlink $bin_link"
  (cd "$payload/share" && find . -type f) | while IFS= read -r f; do
    f=${f#./}
    mkdir -p "$share/$(dirname "$f")" || die "could not create $share/$(dirname "$f")"
    case "$f" in
      applications/*.desktop)
        sed "s|^Exec=[^ ]*|Exec=\"$exe\"|" "$payload/share/$f" >"$share/$f" || die "could not write $share/$f"
        ;;
      *) cp "$payload/share/$f" "$share/$f" || die "could not copy $share/$f" ;;
    esac
    echo "Added $share/$f"
  done || exit 1
}

app_running() {
  if [ "$os" = Darwin ]; then
    pgrep -f "$exe" >/dev/null
  else
    for p in /proc/[0-9]*/exe; do
      [ "$(readlink "$p" 2>/dev/null)" = "$exe" ] && return 0
    done
    return 1
  fi
}

# Removes $1 and its parents up to, not including, $2 while they are empty:
# the folders the install created for its files.
prune_empty() {
  d=$1
  while [ "$d" != "$2" ] && rmdir "$d" 2>/dev/null; do
    d=$(dirname "$d")
  done
}

# Everything the app's updater makes beside the payload <name> is named
# .<name>.<something> or <name>.old (swap.rs's beside and old_path in
# native/crates/updater). The prefix .<name>. is the app's own, and the rule
# is used in place of a list because the updater adds names from release to
# release. The glob cannot match the payload, . or .., and an unmatched
# pattern stays literal and fails the -e test, so nothing is removed then.
remove_update_leftovers() {
  name=$(basename "$payload")
  for f in "$parent/.$name."* "$parent/$name.old"; do
    if [ -e "$f" ] || [ -L "$f" ]; then
      rm -rf "$f" || die "could not remove $f"
      echo "Removed $f"
    fi
  done
}

remove_install() {
  [ -e "$payload" ] || die "nothing to uninstall: $payload does not exist"
  app_running && die "mcbench-lite is running from $exe; quit it and run the uninstall again"

  if [ "$os" = Darwin ]; then
    id=io.github.adonis-singh.mcbench-lite
    # lsregister is found by name, with its folder last on PATH. Unregistering
    # is best effort: an app that was never opened is not registered, and the
    # tool reports that as an error (-10814), so the app is removed either way.
    PATH=$PATH:/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support
    if command -v lsregister >/dev/null && lsregister -u "$payload" >/dev/null 2>&1; then
      echo "Unregistered $payload from Launch Services"
    fi
    rm -rf "$payload" || die "could not remove $payload"
    echo "Removed $payload"
    remove_update_leftovers
    prune_empty "$parent" "$HOME"
    shader_cache=$(getconf DARWIN_USER_CACHE_DIR)$id
    if [ -e "$shader_cache" ]; then
      rm -rf "$shader_cache" || die "could not remove $shader_cache"
      echo "Removed $shader_cache"
    fi
    for p in "Saved Application State/$id.savedState" "Caches/$id" "Preferences/$id.plist" \
      "HTTPStorages/$id" "HTTPStorages/$id.binarycookies" "WebKit/$id"; do
      if [ -e "$HOME/Library/$p" ]; then
        rm -rf "$HOME/Library/$p" || die "could not remove $HOME/Library/$p"
        echo "Removed $HOME/Library/$p"
      fi
    done
  else
    (cd "$payload/share" && find . -type f) | while IFS= read -r f; do
      f=${f#./}
      if [ -e "$share/$f" ]; then
        rm -f "$share/$f" || die "could not remove $share/$f"
        echo "Removed $share/$f"
        prune_empty "$(dirname "$share/$f")" "$share"
      fi
    done || exit 1
    if [ "$(readlink "$bin_link" 2>/dev/null)" = "$exe" ]; then
      rm -f "$bin_link" || die "could not remove $bin_link"
      echo "Removed $bin_link"
      prune_empty "$(dirname "$bin_link")" "$HOME"
    fi
    rm -rf "$payload" || die "could not remove $payload"
    echo "Removed $payload"
    remove_update_leftovers
    prune_empty "$parent" "$HOME"
  fi

  # What the app itself leaves outside the data folder: its cache folder and,
  # after a crash, its own folders in the temporary folder (the one Rust's
  # std::env::temp_dir names: TMPDIR, else the account's temporary folder).
  # A temporary folder is removed only when nothing in it, the folder itself
  # included, was modified in the last day (a folder's own time changes only
  # when entries are added or removed, so its contents are checked too): for
  # one release the localhost app still exists beside the native app and
  # makes folders of the same names (mcb-??????) for its running command-line
  # programs, and deleting one under a running turn would break it, while this
  # script only knows that the native app is not running.
  if [ -e "$HOME/.cache/mcbench" ]; then
    rm -rf "$HOME/.cache/mcbench" || die "could not remove $HOME/.cache/mcbench"
    echo "Removed $HOME/.cache/mcbench"
    prune_empty "$HOME/.cache" "$HOME"
  fi
  find "${tmp_dir%/}" -maxdepth 1 -mindepth 1 -type d -user "$(id -u)" -mtime +0 \
    \( -name 'mcb-??????' -o -name 'mcbench-codex-??????' -o -name 'mcbench-engine-child-*' -o -name 'mcbench-lite-logs' \) |
    while IFS= read -r d; do
      [ -z "$(find "$d" -mtime -1 | head -n 1)" ] || continue
      rm -rf "$d" || die "could not remove $d"
      echo "Removed $d"
    done || exit 1

  if [ -e "$data_dir" ]; then
    echo "Kept the data folder $data_dir, which holds your chats, worlds and settings. To delete it too: rm -rf \"$data_dir\""
  fi
  projects=$HOME/.claude/projects/$(printf '%s' "$HOME/.cache/mcbench/claude-cwd" | sed 's/[^A-Za-z0-9]/-/g')
  for d in "$projects" "$projects"-*; do
    [ -e "$d" ] && echo "Kept Claude Code's transcripts of the app's chats in $d, which belong to your Claude Code. To delete them too: rm -rf \"$d\""
  done
  if [ -e "$HOME/.codex/auth.json" ]; then
    echo "Kept the Codex sign-in file $HOME/.codex/auth.json, which your own Codex CLI also uses. To sign Codex out on this computer: rm \"$HOME/.codex/auth.json\""
  fi
  # The install may have added Ubuntu's profile for bwrap (ubuntu_bwrap_profile).
  # It is a system file that other programs using bubblewrap may now rely on,
  # so it stays; one that a package owns (Ubuntu 26.04's apparmor) is not named.
  profile=/etc/apparmor.d/bwrap-userns-restrict
  if [ -e "$profile" ] && ! dpkg -S "$profile" >/dev/null 2>&1; then
    echo "Kept Ubuntu's AppArmor profile for bwrap, $profile, which other programs that use bubblewrap may rely on. To remove it too: sudo apparmor_parser -R $profile && sudo rm $profile"
  fi
}

main "$@"
