#!/usr/bin/env bash
# The .deb on a real Ubuntu, in a QEMU virtual machine run without root:
#
#   MCBENCH_DATA_BUNDLE=<bundle> native/packaging/test/linux/ubuntu-vm.sh <release> <deb> <rootfs> <work>
#
# <release> is an Ubuntu release with a cloud image (24.04, 26.04); <deb> is
# mcbench-lite_amd64.deb; <rootfs> is the x86_64 root filesystem
# native/sandbox-assets.lock names (its sha256 is checked against the lock);
# MCBENCH_DATA_BUNDLE is the Minecraft data bundle (its tables are the
# headless app's MCBENCH_TABLES, and its blocks.json the data folder's).
# <work> keeps the images, the test build and the results between runs. It needs an x86_64 Linux host with /dev/kvm open
# to the user, qemu-system-x86_64 and qemu-img, Docker, a systemd user
# session, at least 12 GB of memory available, and 127.0.0.1:3191 free.
#
# What it does, in order, and what it prints into <work>/results-<release>.txt:
#
# 1. The cloud image: ubuntu-<release>-server-cloudimg-amd64.img and its
#    SHA256SUMS from cloud-images.ubuntu.com, fetched once into
#    <work>/images/<release>, checked against the SHA256SUMS on every run and
#    kept read-only.
# 2. The test binaries of mcbench-sandbox (`linux_box`, `network`), built in
#    the release build's image (BUILD_IMAGE, pins.sh) so that they run on
#    Ubuntu's glibc, with the target folder /home/user/target: the tests
#    start the agent at the path cargo compiled into them, which the VM's
#    user then fills with the installed agent.
# 3. The VM: a qcow2 overlay over the image, a NoCloud seed (QEMU's FAT
#    drive labelled CIDATA) that creates the user `user` with sudo and an
#    ssh key made in <work>, 4 vCPUs and 4 GB, user-mode networking with ssh
#    forwarded from 127.0.0.1:3191, started headless inside `systemd-run
#    --user --scope --slice=mcbench-s5.slice`. It is stopped on every exit
#    path by the pid QEMU wrote to its pid file, and the overlay deleted.
# 4. Inside, as `user`: the plain facts before and after `sudo apt install`
#    of <deb> (the apparmor and bubblewrap versions, the userns sysctls,
#    aa-status, `bwrap --unshare-user true` and a bwrap with the root bound,
#    with their errors and the kernel's AppArmor lines), then the tests
#    with `--ignored` three ways: `agent` (through the installed
#    /usr/lib/mcbench-lite/mcbench-sandbox-agent serve), `app` (through
#    /usr/lib/mcbench-lite/mcbench-lite sandbox-agent serve) and
#    `unprofiled` (the test binary makes the boxes, with a copy of the agent
#    in the home folder: no profile covers either, as on the install.sh and
#    AppImage routes); then the installed app itself through `mcbench-lite
#    headless turn`: a `bash` call, a networked `bash` call and a `build`
#    call (the build child under bubblewrap), and the same with the three
#    programs copied to ~/.local/lib/mcbench-lite, where install.sh puts
#    them and no profile attaches (recorded, not judged). Then whether the
#    system's own AppArmor profiles (the files under /etc/apparmor.d and the
#    loaded profiles, the app's aside) are the same as before the install,
#    and `apt purge`, after which the app's profile must be gone from both.
#
# It exits 0 when the `agent` and `app` tests and the installed app's calls
# pass and the system's profiles are unchanged; the `unprofiled` results and
# the copy's calls are recorded, not judged.
set -euo pipefail
[ $# -eq 4 ] || { echo "usage: MCBENCH_DATA_BUNDLE=<bundle> native/packaging/test/linux/ubuntu-vm.sh <release> <deb> <rootfs> <work>" >&2; exit 2; }
release=$1
deb=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
rootfs=$(cd "$(dirname "$3")" && pwd)/$(basename "$3")
mkdir -p "$4"
work=$(cd "$4" && pwd)
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../../.." && pwd)
. "$repo/native/packaging/linux/pins.sh"
port=3191

[ -f "$deb" ] || { echo "$deb is not a file" >&2; exit 1; }
[ -n "${MCBENCH_DATA_BUNDLE:-}" ] && [ -f "$MCBENCH_DATA_BUNDLE" ] || { echo "MCBENCH_DATA_BUNDLE does not name the Minecraft data bundle" >&2; exit 1; }
sha=$(awk '$1 == "rootfs" && $2 == "x86_64" { print $4 }' "$repo/native/sandbox-assets.lock")
echo "$sha  $rootfs" | sha256sum -c --quiet - || { echo "$rootfs is not the root filesystem native/sandbox-assets.lock names (sha256 $sha)" >&2; exit 1; }
available=$(awk '/^MemAvailable:/ { print int($2 / 1048576) }' /proc/meminfo)
[ "$available" -ge 12 ] || { echo "this host has $available GB of memory available; the VM is started with at least 12" >&2; exit 1; }
! ss -ltnH "sport = :$port" | grep -q . || { echo "127.0.0.1:$port is in use" >&2; exit 1; }

# 1. The image.
images=$work/images/$release
image=$images/ubuntu-$release-server-cloudimg-amd64.img
mkdir -p "$images"
if [ ! -f "$image" ]; then
  curl -fsSL -o "$images/SHA256SUMS" "https://cloud-images.ubuntu.com/releases/$release/release/SHA256SUMS"
  curl -fsSL -o "$image.part" "https://cloud-images.ubuntu.com/releases/$release/release/$(basename "$image")"
  mv "$image.part" "$image"
fi
(cd "$images" && grep " \*$(basename "$image")\$" SHA256SUMS | sed 's/ \*/  /' | sha256sum -c --quiet -) \
  || { echo "$image is not the image its SHA256SUMS names" >&2; exit 1; }
chmod 444 "$image"

# 2. The test binaries.
tests=$work/tests
mkdir -p "$tests/cache" "$tests/target"
fetch "$RUSTUP_INIT_URL_x86_64" "$RUSTUP_INIT_SHA256_x86_64" "$tests/cache/rustup-init"
chmod 755 "$tests/cache/rustup-init"
docker run --rm --name "mcbn-s5-ubuntu-tests-$$" --cpus 2 --memory 8g -v "$repo:/src:ro" -v "$tests/cache:/cache" \
  -v "$tests/target:/home/user/target" -e HOST_UID="$(id -u)" -e HOST_GID="$(id -g)" "$BUILD_IMAGE" bash -euo pipefail -c '
trap "chown -R $HOST_UID:$HOST_GID /cache /home/user/target" EXIT
printf "deb http://archive.debian.org/debian buster main\ndeb http://archive.debian.org/debian-security buster/updates main\n" > /etc/apt/sources.list
apt-get -qq update && DEBIAN_FRONTEND=noninteractive apt-get -qq install -y --no-install-recommends ca-certificates curl gcc libc6-dev >/dev/null
export RUSTUP_HOME=/cache/rustup CARGO_HOME=/cache/cargo PATH=/cache/cargo/bin:$PATH
[ -x /cache/cargo/bin/rustup ] || /cache/rustup-init -y --quiet --no-modify-path --profile minimal --default-toolchain none
cd /src/native
rustup show active-toolchain >/dev/null 2>&1 || rustup install
cargo test --locked -p mcbench-sandbox --test linux_box --test network --no-run --target-dir /home/user/target --message-format=json 2>/dev/null \
  | grep -o "\"executable\":\"[^\"]*/deps/[^\"]*\"" | cut -d\" -f4 > /home/user/target/executables
while read -r exe; do name=$(basename "$exe"); cp "$exe" "/home/user/target/${name%-*}"; done < /home/user/target/executables
ls /home/user/target/linux_box /home/user/target/network >/dev/null'

# 3. The VM.
vm=$work/vm-$release
mkdir -p "$vm/seed"
[ -f "$work/key" ] || ssh-keygen -q -t ed25519 -N '' -C mcbench-ubuntu-vm -f "$work/key"
sshv() { ssh -q -i "$work/key" -p "$port" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=5 -o BatchMode=yes -o ServerAliveInterval=15 user@127.0.0.1 "$@"; }
scpv() { scp -q -i "$work/key" -P "$port" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o BatchMode=yes "$@"; }
stop() {
  if [ -f "$vm/qemu.pid" ]; then
    local pid
    pid=$(cat "$vm/qemu.pid")
    timeout 20 ssh -q -i "$work/key" -p "$port" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=5 -o BatchMode=yes user@127.0.0.1 'sudo systemctl poweroff' 2>/dev/null || true
    for _ in $(seq 60); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
    if kill -0 "$pid" 2>/dev/null; then kill "$pid"; fi
    for _ in $(seq 20); do kill -0 "$pid" 2>/dev/null || break; sleep 1; done
    if kill -0 "$pid" 2>/dev/null; then kill -KILL "$pid"; fi
    /bin/rm -f "$vm/qemu.pid"
  fi
  /bin/rm -rf "$vm/overlay.qcow2" "$vm/seed"
}
[ ! -e "$vm/qemu.pid" ] || { echo "$vm/qemu.pid exists: a VM of an earlier run may still be running (its pid is in that file)" >&2; exit 1; }
trap stop EXIT
qemu-img create -q -f qcow2 -F qcow2 -b "$image" "$vm/overlay.qcow2" 20G
printf 'instance-id: mcbench-ubuntu-%s\nlocal-hostname: ubuntu-%s\n' "${release//./}" "${release//./}" > "$vm/seed/meta-data"
printf '#cloud-config\nusers:\n  - name: user\n    shell: /bin/bash\n    sudo: ALL=(ALL) NOPASSWD:ALL\n    ssh_authorized_keys:\n      - %s\nssh_pwauth: false\n' "$(cat "$work/key.pub")" > "$vm/seed/user-data"
systemd-run --user --scope --quiet --slice=mcbench-s5.slice -- qemu-system-x86_64 -name "mcbench-ubuntu-$release" \
  -machine q35,accel=kvm -cpu host -smp 4 -m 4096 -display none -daemonize -pidfile "$vm/qemu.pid" \
  -serial file:"$vm/console.log" -device virtio-rng-pci \
  -drive if=virtio,file="$vm/overlay.qcow2",format=qcow2 \
  -drive if=virtio,format=raw,file=fat:ro:"$vm/seed",file.label=CIDATA,readonly=on \
  -nic user,model=virtio-net-pci,hostfwd=tcp:127.0.0.1:$port-:22
for _ in $(seq 75); do sshv true 2>/dev/null && break; sleep 4; done
sshv 'cloud-init status --wait' >/dev/null

# 4. Inside.
bundle=$work/tables.tar
(cd "$work" && /bin/rm -rf tables && mkdir tables && zstd -qdc "$MCBENCH_DATA_BUNDLE" | tar -x -C tables blocks.txt loot.json datafix.txt blocks.json && tar -cf "$bundle" tables)
sshv 'mkdir -p target'
scpv "$deb" user@127.0.0.1:mcbench-lite_amd64.deb
scpv "$rootfs" "$bundle" user@127.0.0.1:
scpv "$tests/target/linux_box" "$tests/target/network" user@127.0.0.1:target/
results=$work/results-$release.txt
set +e
sshv "tar -xf tables.tar && bash -s -- $sha" > "$results" 2>&1 <<'INSIDE'
set -u
sha=$1
cd "$HOME"
step() { printf '\n==== %s\n' "$*"; }
facts() {
  step "facts: $1"
  . /etc/os-release
  echo "$PRETTY_NAME, kernel $(uname -r)"
  dpkg-query -W -f '${Package} ${Version}\n' apparmor bubblewrap mcbench-lite 2>&1
  sysctl kernel.apparmor_restrict_unprivileged_userns kernel.apparmor_restrict_unprivileged_unconfined 2>&1
  sudo aa-status 2>&1 | grep -E '^[0-9]+ profiles are (loaded|in enforce|in unconfined)'
  echo "loaded profiles naming bwrap, userns or mcbench:"
  sudo cat /sys/kernel/security/apparmor/profiles | grep -E 'bwrap|userns|mcbench' | sort
  ls -l /etc/apparmor.d/mcbench-lite 2>&1
  local since
  since=$(date '+%Y-%m-%d %H:%M:%S')
  sleep 1
  echo '$ bwrap --unshare-user true'; bwrap --unshare-user true; echo "exit $?"
  echo '$ bwrap --unshare-user --ro-bind / / true'; bwrap --unshare-user --ro-bind / / true; echo "exit $?"
  aa_lines "$since"
}
aa_lines() {
  sleep 1
  echo "AppArmor lines since then:"
  sudo journalctl -k --since "$1" --no-pager | grep -o 'apparmor="[A-Z]*".*' \
    | sed 's/ pid=[0-9]*//; s/ name="proc\/[0-9]*\// name="proc\/<pid>\//' | sort | uniq -c | sort -rn | head -12
}
system_profiles() {
  (cd /etc/apparmor.d && sudo find . -path ./mcbench-lite -prune -o -type f -print0 | sort -z | sudo xargs -0 sha256sum)
  sudo cat /sys/kernel/security/apparmor/profiles | grep -v '^mcbench-lite ' | sort
}
export MCBENCH_SANDBOX_ROOTFS=$HOME/rootfs-x86_64.tar.zst MCBENCH_SANDBOX_ROOTFS_SHA256=$sha
export MCBENCH_SANDBOX_SCRATCH=$HOME/scratch MCBENCH_SANDBOX_NETWORK=1
mkdir -p "$MCBENCH_SANDBOX_SCRATCH"
judged=0

facts "stock, before the install"
system_profiles > profiles-before.txt
step "sudo apt install ./mcbench-lite_amd64.deb"
sudo apt-get -qq update -o DPkg::Lock::Timeout=600
sudo DEBIAN_FRONTEND=noninteractive apt-get -qq install -y -o DPkg::Lock::Timeout=600 ./mcbench-lite_amd64.deb > apt.log 2>&1; echo "exit $?"
grep -i -E 'apparmor|error' apt.log
facts "after the install"
step "aa-status names the app's profile"
sudo aa-status | grep -E '^ +mcbench-lite$|profiles are in (enforce|unconfined) mode'
system_profiles > profiles-after.txt
step "the system's own profiles, before and after the install"
if diff profiles-before.txt profiles-after.txt; then echo "unchanged ($(wc -l < profiles-before.txt) lines)"; else echo changed; judged=1; fi

agent=$HOME/target/debug/mcbench-sandbox-agent
mkdir -p "$(dirname "$agent")"
for mode in agent app unprofiled; do
  rm -f "$agent"
  case $mode in
    agent) ln -s /usr/lib/mcbench-lite/mcbench-sandbox-agent "$agent"; export MCBENCH_SANDBOX_VIA_AGENT=1 ;;
    app) printf '#!/bin/sh\nexec /usr/lib/mcbench-lite/mcbench-lite sandbox-agent "$@"\n' > "$agent"; chmod 755 "$agent"; export MCBENCH_SANDBOX_VIA_AGENT=1 ;;
    unprofiled) cp /usr/lib/mcbench-lite/mcbench-sandbox-agent "$agent"; unset MCBENCH_SANDBOX_VIA_AGENT ;;
  esac
  since=$(date '+%Y-%m-%d %H:%M:%S')
  sleep 1
  for t in linux_box network; do
    step "tests, $mode: $t"
    timeout 900 "target/$t" --ignored --test-threads 1 > "$mode-$t.log" 2>&1
    code=$?
    grep -E '^test |^test result' "$mode-$t.log"
    grep -E -A3 '^---- ' "$mode-$t.log" | cut -c1-600
    echo "exit $code"
    if [ "$mode" != unprofiled ] && [ "$code" != 0 ]; then judged=1; fi
  done
  aa_lines "$since"
done

cat > headless.py <<'PY'
import json, os, subprocess, sys
program, data = sys.argv[1], sys.argv[2]
app = subprocess.Popen([program, "headless", "turn"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
n = 0
def ask(op, **fields):
    global n
    n += 1
    app.stdin.write(json.dumps(dict(id=n, op=op, **fields)) + "\n")
    app.stdin.flush()
    while True:
        line = app.stdout.readline()
        if not line:
            sys.exit("mcbench-lite headless turn ended")
        answer = json.loads(line)
        if "event" not in answer:
            return answer
chat = ask("create", data=data, provider="openai", model="gpt-5")["chat"]
failed = False
tower = 'module.exports = mc => { mc.fill([0, 0, 0], [4, 0, 4], "stone"); mc.fill([2, 12, 2], [2, 14, 2], "sand"); };'
for name, arguments, wanted in [
    ("bash", {"command": "echo in the box; id -u; head -1 /etc/os-release"}, "in the box"),
    ("bash", {"command": "curl -sS -o /dev/null -w 'status %{http_code}\\n' https://example.com/", "network": True}, "status 200"),
    ("build", {"program": tower}, None),
]:
    answer = ask("call", data=data, chat=chat, name=name, arguments=arguments, imageInput=False, turnChat=False)
    content = answer.get("content", answer.get("error"))
    ok = "error" not in answer and (wanted in content if wanted else answer["result"].get("ok") is True)
    failed |= not ok
    print(f"{name} {json.dumps(arguments)[:90]}: {'pass' if ok else 'FAIL'}\n  {content[:600]}")
app.stdin.close()
app.wait(60)
sys.exit(1 if failed else 0)
PY
# headless <program> <data folder>: a bash call, a networked bash call and a
# build call through <program> headless turn.
headless() {
  mkdir -p "$2/assets/1.21.10"
  cp tables/blocks.json "$2/assets/1.21.10/"
  local since code
  since=$(date '+%Y-%m-%d %H:%M:%S')
  sleep 1
  MCBENCH_TABLES=$HOME/tables timeout 300 python3 -I headless.py "$1" "$2"
  code=$?
  echo "exit $code"
  aa_lines "$since"
  return $code
}
step "the installed app: /usr/lib/mcbench-lite/mcbench-lite headless turn"
headless /usr/lib/mcbench-lite/mcbench-lite "$HOME/appdata" || judged=1
step "the same programs where install.sh puts them (no profile attaches): ~/.local/lib/mcbench-lite/mcbench-lite headless turn"
mkdir -p .local/lib
cp -a /usr/lib/mcbench-lite .local/lib/
headless "$HOME/.local/lib/mcbench-lite/mcbench-lite" "$HOME/appdata-script"

step "sudo apt purge mcbench-lite"
sudo DEBIAN_FRONTEND=noninteractive apt-get -qq purge -y mcbench-lite > purge.log 2>&1; echo "exit $?"
grep -i -E 'apparmor|error' purge.log
facts "after the purge"
system_profiles > profiles-purged.txt
step "the system's own profiles, before the install and after the purge"
if diff profiles-before.txt profiles-purged.txt; then echo unchanged; else echo changed; judged=1; fi
if sudo grep -q '^mcbench-lite ' /sys/kernel/security/apparmor/profiles || [ -e /etc/apparmor.d/mcbench-lite ]; then echo "the app's profile is still there"; judged=1; fi
step "judged: $judged"
exit $judged
INSIDE
code=$?
set -e
grep -E '^==== |^test result|^exit |: (pass|FAIL)$|: pass|: FAIL|^unchanged|^changed' "$results" | sed 's/^/  /'
echo "$results: exit $code"
exit "$code"
