#!/usr/bin/env python3
"""The bash sandbox's guest base, from pinned Debian 13 packages.

    python3 -I build.py OUT ARCH AGENT [DOWNLOADS]

ARCH is aarch64 or x86_64; AGENT is the static mcbench-sandbox-agent for it
(build.sh builds it); DOWNLOADS is a folder of the .deb files already fetched
(default OUT/debs; a missing one is fetched from snapshot.debian.org and
checked against its pin). It writes

    OUT/mcbench-sandbox-guest-ARCH/kernel        Debian's cloud kernel (the macOS VM boots it)
    OUT/mcbench-sandbox-guest-ARCH/initramfs     newc cpio: the userland, its /init, the kernel modules the boxes need
    OUT/mcbench-sandbox-guest-ARCH/userland.tar  the same userland as a tar (the WSL2 distro imports it)
    OUT/mcbench-sandbox-guest-ARCH/manifest.json the pins, what each file is, sha256 of each
    OUT/mcbench-sandbox-guest-ARCH.tar           the four files, the release asset before zstd

Every byte is a function of the pins and AGENT: entries sorted by name,
owned by 0:0 (the agent's user is chosen at boot), mtime 0, no compression
but zstd's of the asset (build.sh). Python's standard library only.

The userland is what runs outside the boxes: busybox (the init's shell and
tools), bubblewrap and the C libraries it links, and the agent. The boxes'
own userland is the pinned root filesystem (rootfs/build.sh), with pasta and
iptables (the networked box's rules run with the root filesystem's iptables,
so the guest needs only the kernel's nf_tables, nft_compat and REJECT).
"""

import hashlib
import io
import json
import lzma
import os
import sys
import tarfile
import urllib.request

SNAPSHOT = "https://snapshot.debian.org/archive/"
# One instant of Debian's archive and its security archive: every pin below
# is a file there (snapshot.debian.org keeps every file it ever served).
WHEN = "20261117T000000Z"
KERNEL = "6.12.117+deb13-cloud"

# (archive, pool path, sha256) per architecture; Debian's own Packages
# indexes of trixie and trixie-security at WHEN give these sha256.
PINS = {
    "aarch64": {
        "deb": "arm64",
        "triplet": "aarch64-linux-gnu",
        "loader": "usr/lib/ld-linux-aarch64.so.1",
        "packages": {
            "linux-image": ("debian-security", "pool/updates/main/l/linux/linux-image-6.12.117+deb13-cloud-arm64-unsigned_6.12.117-1_arm64.deb", "e0261b7e2ab499379b8b2c384fc28d9a51f762f40d071c8ed51e05e6e83ba879"),
            "busybox-static": ("debian", "pool/main/b/busybox/busybox-static_1.37.0-6+b9_arm64.deb", "c833be48abfa16bc19c4966ec93e289ff1ce5d2f1476cad3a57bd105378cd15c"),
            "bubblewrap": ("debian-security", "pool/updates/main/b/bubblewrap/bubblewrap_0.12.0-1~deb13u1_arm64.deb", "d1ac1d0d81c815fc15842b9f04fdf414830ed14a0a602bf6344740f90bb36c00"),
            "libc6": ("debian", "pool/main/g/glibc/libc6_2.41-12+deb13u4_arm64.deb", "8784eda966b189c777a384dac5ce009e8fc9b52d006926c5a013e7fa8aa688cc"),
            "libcap2": ("debian", "pool/main/libc/libcap2/libcap2_2.75-10+deb13u1+b3_arm64.deb", "863aa5b9cdf6f80010b48982d4639126b09a89eeef2c0c5c7be226297c811581"),
            "libselinux1": ("debian", "pool/main/libs/libselinux/libselinux1_3.8.1-1_arm64.deb", "fc13da4c7783b8f4361d92ef1045931f298d1b0ac78ed1a85c2232615f49fc6f"),
            "libpcre2-8-0": ("debian-security", "pool/updates/main/p/pcre2/libpcre2-8-0_10.46-1~deb13u3_arm64.deb", "1a02b7129990690ea095fd35d7ace6853742923d158019e1fb4ef27efd2c51a7"),
        },
    },
    "x86_64": {
        "deb": "amd64",
        "triplet": "x86_64-linux-gnu",
        "loader": "usr/lib64/ld-linux-x86-64.so.2",
        "packages": {
            "linux-image": ("debian-security", "pool/updates/main/l/linux/linux-image-6.12.117+deb13-cloud-amd64-unsigned_6.12.117-1_amd64.deb", "4113a615a1f1535ed4356ca03d428c74536b00f6ccb58e049a2ffd6a1cb3f1ad"),
            "busybox-static": ("debian", "pool/main/b/busybox/busybox-static_1.37.0-6+b9_amd64.deb", "598a3fd92bdafc34cd81196b2952ad36e910e47a4ecf162f8ce5e8e262598e53"),
            "bubblewrap": ("debian-security", "pool/updates/main/b/bubblewrap/bubblewrap_0.12.0-1~deb13u1_amd64.deb", "70aca4fa8daeacb677ec00e8063eb586f08ae3d94b1f11e684370b5524c43431"),
            "libc6": ("debian", "pool/main/g/glibc/libc6_2.41-12+deb13u4_amd64.deb", "967aa62605721081c3eb2a17650611a792aa802d76a6511d1840242623d204c9"),
            "libcap2": ("debian", "pool/main/libc/libcap2/libcap2_2.75-10+deb13u1+b3_amd64.deb", "89fc4d34fc7a28ad6f0fcd0c561ab253b9dedf6f77f5a000b47c276c8295bf67"),
            "libselinux1": ("debian", "pool/main/libs/libselinux/libselinux1_3.8.1-1_amd64.deb", "68bb8d32bd8d6d7d2f5952a169db03d1484b46ae1e52abccdec42a19dccea5d5"),
            "libpcre2-8-0": ("debian-security", "pool/updates/main/p/pcre2/libpcre2-8-0_10.46-1~deb13u3_amd64.deb", "e226f661d918f04daf38cdbc4806b7ed7d6ef95c7eb0ade692fc350e31970040"),
            # The WSL2 distro also runs the Linux build child under the Linux
            # build sandbox (mcbench-build's sandbox/linux.rs): prlimit, and
            # libgcc_s, which the child links beside the C library.
            "util-linux": ("debian", "pool/main/u/util-linux/util-linux_2.41.5-0+deb13u1_amd64.deb", "7d5b1176cc3d8cc6b290b3ec17af458f362d9200845f30da22057a8198744430"),
            "libsmartcols1": ("debian", "pool/main/u/util-linux/libsmartcols1_2.41.5-0+deb13u1_amd64.deb", "d96bac9c8805685f2734885098a214b82950da6ebb182c27a6b7a124a0323392"),
            "libgcc-s1": ("debian", "pool/main/g/gcc-14/libgcc-s1_14.2.0-19_amd64.deb", "3c71917b490d1a17aed43196a2787a256ecf060526cdb20216a74bedc061b150"),
        },
    },
}

# The packages of the build child's sandbox in the WSL2 distro (x86_64 only:
# Windows on ARM is not built), and what is taken from each.
BUILD_CHILD = {
    "util-linux": lambda p: p == "usr/bin/prlimit",
    "libsmartcols1": None,
    "libgcc-s1": None,
}

# The kernel modules the boxes need (the rest of what they need is built
# into Debian's cloud kernel: ext4, virtiofs, the virtio console, cgroup v2
# with memory, pids and cpu, user namespaces). Their dependencies are added
# from each module's own .modinfo, and the order is dependencies first.
# binfmt_misc (a module of the cloud kernel) is deliberately not here: with
# it absent no handler can be registered in the VM at all, and the agent's
# box start mounts an instance of its own where the kernel has one
# (linux::keeper::own_binfmt).
MODULES = [
    "virtio_blk",  # /dev/vda, the agent's disk
    "virtio_net",  # eth0 on the NAT
    "virtio_rng",  # the host's entropy
    "vmw_vsock_virtio_transport",  # the host's connection to the agent
    "tun",  # pasta's tap device
    "nft_compat",  # iptables-nft's targets (REJECT) through nf_tables
    "ipt_REJECT",  # today's EGRESS_RULES' -j REJECT
]


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def fetch(arch, name, downloads):
    archive, path, pin = PINS[arch]["packages"][name]
    file = os.path.join(downloads, os.path.basename(path))
    if not os.path.exists(file):
        url = f"{SNAPSHOT}{archive}/{WHEN}/{path}"
        print(f"[guest] fetching {url}", file=sys.stderr)
        with urllib.request.urlopen(url) as response:
            data = response.read()
        with open(file + ".part", "wb") as out:
            out.write(data)
        os.replace(file + ".part", file)
    data = open(file, "rb").read()
    got = sha256_bytes(data)
    if got != pin:
        sys.exit(f"build.py: {file} has sha256 {got}, not the pinned {pin}")
    return data


def deb_data(deb):
    """The members of a .deb's data.tar.xz: path -> (TarInfo, bytes or None)."""
    if not deb.startswith(b"!<arch>\n"):
        sys.exit("build.py: not a .deb (ar archive)")
    at = 8
    while at < len(deb):
        header = deb[at:at + 60]
        name = header[:16].decode().strip().rstrip("/")
        size = int(header[48:58].decode().strip())
        body = deb[at + 60:at + 60 + size]
        at += 60 + size + (size & 1)
        if name.startswith("data.tar"):
            if name != "data.tar.xz":
                sys.exit(f"build.py: a .deb with {name}; only data.tar.xz is read")
            members = {}
            with tarfile.open(fileobj=io.BytesIO(lzma.decompress(body))) as tar:
                for info in tar.getmembers():
                    path = os.path.normpath(info.name).lstrip("./")
                    if not path or path == ".":
                        continue
                    content = tar.extractfile(info).read() if info.isfile() else None
                    members[path] = (info, content)
            return members
    sys.exit("build.py: a .deb without data.tar")


class Tree:
    """The guest's files: path -> (kind, mode, content or link target)."""

    def __init__(self):
        self.entries = {}

    def dir(self, path, mode=0o755):
        parts = path.split("/")
        for i in range(1, len(parts) + 1):
            sub = "/".join(parts[:i])
            if sub not in self.entries:
                self.entries[sub] = ("dir", mode, None)

    def file(self, path, data, mode):
        self.dir(os.path.dirname(path)) if os.path.dirname(path) else None
        self.entries[path] = ("file", mode, data)

    def link(self, path, target):
        self.dir(os.path.dirname(path)) if os.path.dirname(path) else None
        self.entries[path] = ("link", 0o777, target.encode())

    def node(self, path, mode, major, minor):
        self.dir(os.path.dirname(path))
        self.entries[path] = ("char", mode, (major, minor))

    def from_deb(self, members, keep):
        for path, (info, content) in sorted(members.items()):
            if not keep(path):
                continue
            if info.isdir():
                self.dir(path, info.mode & 0o7777)
            elif info.isfile():
                self.file(path, content, info.mode & 0o7777)
            elif info.issym():
                self.link(path, info.linkname)
            elif info.islnk():
                target = os.path.normpath(info.linkname).lstrip("./")
                self.file(path, members[target][1], info.mode & 0o7777)


def modinfo(ko):
    """A module's `name` and `depends` from its .modinfo strings."""
    fields = {}
    for key in (b"name=", b"depends="):
        at = ko.find(b"\0" + key)
        if at >= 0:
            end = ko.index(b"\0", at + 1)
            fields[key[:-1].decode()] = ko[at + 1 + len(key):end].decode()
    return fields.get("name"), [d for d in fields.get("depends", "").split(",") if d]


def modules(kernel_members, wanted):
    """The modules in `wanted` and their dependencies, decompressed, in load order."""
    prefix = f"usr/lib/modules/"
    found = {}
    for path, (info, content) in kernel_members.items():
        if path.startswith(prefix) and path.endswith(".ko.xz"):
            ko = lzma.decompress(content)
            name, depends = modinfo(ko)
            found[name] = (ko, depends)
    order = []

    def add(name):
        if name in order:
            return
        if name not in found:
            sys.exit(f"build.py: no module {name} in the kernel package (built in, or misnamed?)")
        for dep in found[name][1]:
            add(dep)
        order.append(name)

    for name in wanted:
        add(name)
    return [(name, found[name][0]) for name in order]


def cpio(tree):
    """newc cpio of the tree, sorted, owned by 0:0, mtime 0."""
    out = io.BytesIO()
    ino = 1
    for path in sorted(tree.entries):
        kind, mode, content = tree.entries[path]
        rdev = (0, 0)
        if kind == "dir":
            mode |= 0o040000
            data = b""
        elif kind == "file":
            mode |= 0o100000
            data = content
        elif kind == "link":
            mode |= 0o120000
            data = content
        else:
            mode |= 0o020000
            data = b""
            rdev = content
        name = path.encode() + b"\0"
        fields = [ino, mode, 0, 0, 2 if kind == "dir" else 1, 0, len(data), 0, 0, rdev[0], rdev[1], len(name), 0]
        out.write(b"070701" + b"".join(b"%08X" % f for f in fields) + name)
        out.write(b"\0" * (-(110 + len(name)) % 4))
        out.write(data)
        out.write(b"\0" * (-len(data) % 4))
        ino += 1
    trailer = b"TRAILER!!!\0"
    out.write(b"070701" + b"".join(b"%08X" % f for f in [0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, len(trailer), 0]) + trailer)
    out.write(b"\0" * (-(110 + len(trailer)) % 4))
    out.write(b"\0" * (-out.tell() % 512))
    return out.getvalue()


def tar_of(files):
    """A tar of (name, mode, bytes or None for a folder, link target) in order, 0:0, mtime 0."""
    out = io.BytesIO()
    with tarfile.open(fileobj=out, mode="w", format=tarfile.GNU_FORMAT) as tar:
        for name, kind, mode, content in files:
            info = tarfile.TarInfo(name)
            info.mode = mode
            info.uid = info.gid = 0
            info.uname = info.gname = "root"
            info.mtime = 0
            if kind == "dir":
                info.type = tarfile.DIRTYPE
                tar.addfile(info)
            elif kind == "link":
                info.type = tarfile.SYMTYPE
                info.linkname = content.decode()
                tar.addfile(info)
            elif kind == "char":
                info.type = tarfile.CHRTYPE
                info.devmajor, info.devminor = content
                tar.addfile(info)
            else:
                info.size = len(content)
                tar.addfile(info, io.BytesIO(content))
    return out.getvalue()


def main():
    if len(sys.argv) not in (4, 5):
        sys.exit(__doc__)
    out, arch, agent = sys.argv[1], sys.argv[2], sys.argv[3]
    if arch not in PINS:
        sys.exit(f"build.py: ARCH is aarch64 or x86_64, not {arch}")
    downloads = sys.argv[4] if len(sys.argv) == 5 else os.path.join(out, "debs")
    os.makedirs(downloads, exist_ok=True)
    here = os.path.dirname(os.path.abspath(__file__))
    pins = PINS[arch]
    triplet = pins["triplet"]
    debs = {name: deb_data(fetch(arch, name, downloads)) for name in pins["packages"]}

    tree = Tree()
    # Debian 13's merged /usr.
    for top in ("bin", "sbin", "lib"):
        tree.link(top, f"usr/{top}")
    if arch == "x86_64":
        tree.link("lib64", "usr/lib64")
    for path in ("dev", "proc", "sys", "tmp", "run", "mnt", "var", "etc", "root"):
        tree.dir(path)
    tree.node("dev/console", 0o600, 5, 1)
    tree.node("dev/null", 0o666, 1, 3)
    tree.from_deb(debs["busybox-static"], lambda p: p == "usr/bin/busybox")
    tree.from_deb(debs["bubblewrap"], lambda p: p == "usr/bin/bwrap")
    # The C library's loader and its shared objects (no gconv, locales or
    # documentation): what bubblewrap links, and the four a program built
    # against an older glibc names besides `libc.so.6`. The release's build
    # child is built against glibc 2.28 (the Linux release's baseline) and
    # names `librt.so.1`, `libpthread.so.0`, `libdl.so.2` and `libm.so.6`;
    # since glibc 2.34 the first three are compatibility stubs of libc6.
    # Without them the WSL distro's `build` failed to load the child.
    libdir = f"usr/lib/{triplet}/"
    glibc = ("libc.so", "ld-linux", "libm.so", "librt.so", "libpthread.so", "libdl.so")
    tree.from_deb(debs["libc6"], lambda p: p == pins["loader"] or (p.startswith(libdir) and "/gconv" not in p and os.path.basename(p).startswith(glibc)))
    for name in ("libcap2", "libselinux1", "libpcre2-8-0"):
        tree.from_deb(debs[name], lambda p: p.startswith(libdir) and ".so" in p)
    for name, keep in BUILD_CHILD.items():
        if name in debs:
            tree.from_deb(debs[name], keep or (lambda p: p.startswith(libdir) and ".so" in p))
    tree.file("usr/bin/mcbench-sandbox-agent", open(agent, "rb").read(), 0o755)
    tree.file("init", open(os.path.join(here, "init"), "rb").read(), 0o755)
    tree.file("usr/share/mcbench/udhcpc.sh", open(os.path.join(here, "udhcpc.sh"), "rb").read(), 0o755)
    tree.file("etc/passwd", b"root:x:0:0:root:/root:/bin/sh\n", 0o644)
    tree.file("etc/group", b"root:x:0:\n", 0o644)
    tree.file("etc/resolv.conf", b"", 0o644)

    userland = tar_of([(p,) + tree.entries[p] for p in sorted(tree.entries)])

    kernel_members = debs["linux-image"]
    kernel_name = [p for p in kernel_members if p.startswith("boot/vmlinuz-")]
    if len(kernel_name) != 1:
        sys.exit(f"build.py: the kernel package holds {kernel_name}")
    kernel = kernel_members[kernel_name[0]][1]
    loaded = modules(kernel_members, MODULES)
    for name, ko in loaded:
        tree.file(f"usr/lib/mcbench/modules/{name}.ko", ko, 0o644)
    tree.file("usr/lib/mcbench/modules.order", "".join(f"{name}\n" for name, _ in loaded).encode(), 0o644)
    initramfs = cpio(tree)

    folder = os.path.join(out, f"mcbench-sandbox-guest-{arch}")
    os.makedirs(folder, exist_ok=True)
    manifest = {
        "arch": arch,
        "debian_snapshot": WHEN,
        "kernel": kernel_name[0].removeprefix("boot/"),
        "packages": {name: {"file": os.path.basename(p[1]), "sha256": p[2]} for name, p in pins["packages"].items()},
        "modules": [name for name, _ in loaded],
        "agent_sha256": sha256_bytes(open(agent, "rb").read()),
        "files": {
            "kernel": sha256_bytes(kernel),
            "initramfs": sha256_bytes(initramfs),
            "userland.tar": sha256_bytes(userland),
        },
    }
    manifest_bytes = (json.dumps(manifest, indent=2, sort_keys=True) + "\n").encode()
    files = [("kernel", kernel), ("initramfs", initramfs), ("userland.tar", userland), ("manifest.json", manifest_bytes)]
    for name, data in files:
        with open(os.path.join(folder, name), "wb") as f:
            f.write(data)
    asset = tar_of([(name, "file", 0o644, data) for name, data in files])
    with open(os.path.join(out, f"mcbench-sandbox-guest-{arch}.tar"), "wb") as f:
        f.write(asset)
    for name, data in files:
        print(f"{name} {len(data)} bytes sha256 {sha256_bytes(data)}")
    print(f"tar {len(asset)} bytes sha256 {sha256_bytes(asset)}")


main()
