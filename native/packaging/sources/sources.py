"""The source bundle of the sandbox's guest base and root filesystem (sources.sh says what it makes).

    python3 -I sources.py [--check] LISTS LOCK OUT

LISTS is the lists/ folder `native/packaging/sandbox-assets.sh build` writes
(guest-base-<arch>.packages.tsv, rootfs-<arch>.dpkg-status and
rootfs-<arch>.pip.tsv for x86_64 and aarch64); LOCK is the
sandbox-assets.lock of the same build, which names the release, with the
root filesystems it names beside it (their packages' copyright files). Without
--check it fetches every Debian source package from snapshot.debian.org,
writes OUT/MANIFEST.tsv and OUT/README.md, then runs the check. With --check
it only runs the check, which needs no network: the rows of MANIFEST.tsv
name exactly the packages of the lists and other.tsv's parts, every file a
row names is in OUT with that sha256, every file a .dsc names is in a row,
and every file in OUT is in a row.
"""

import hashlib
import json
import os
import re
import subprocess
import sys
import tarfile
import time
import tomllib
import urllib.error
import urllib.parse
import urllib.request
import zlib

SNAPSHOT = "https://snapshot.debian.org"
PAUSE = 0.5  # seconds before every request to snapshot.debian.org
HERE = os.path.dirname(os.path.abspath(__file__))
NATIVE = os.path.dirname(os.path.dirname(HERE))
ARCHES = {"x86_64": "amd64", "aarch64": "arm64"}
COLUMNS = ["userland", "arch", "package", "version", "architecture", "source", "source_version", "licence_family", "licences", "file", "sha256"]
KEY = COLUMNS[:7]


def die(message):
    sys.exit(f"sources.py: {message}")


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def tsv(value):
    return re.sub(r"\s+", " ", value).strip()


def no_epoch(version):
    return version.split(":", 1)[1] if ":" in version else version


def table(path, columns):
    """A tab-separated list with # comment lines, as rows of the given columns."""
    if not os.path.isfile(path):
        die(f"{path} is missing (sandbox-assets.sh build writes it)")
    return [dict(zip(columns, line.rstrip("\n").split("\t"), strict=True)) for line in open(path) if line.strip() and not line.startswith("#")]


def deb_fields(paragraph):
    fields, key = {}, None
    for line in paragraph.split("\n"):
        if not line:
            continue
        if line[0] in " \t":
            fields[key] += "\n" + line
        else:
            key, _, value = line.partition(":")
            fields[key] = value.strip()
    return fields


# The lists.

def read_lists(lists):
    """Every package the lists name, per userland and architecture."""
    found = {}
    for arch, deb_arch in ARCHES.items():
        guest = table(os.path.join(lists, f"guest-base-{arch}.packages.tsv"), ["package", "version", "architecture", "file", "sha256"])
        snapshot = re.search(r"snapshot\.debian\.org (\d{8}T\d{6}Z)", open(os.path.join(lists, f"guest-base-{arch}.packages.tsv")).readline())
        if not snapshot:
            die(f"guest-base-{arch}.packages.tsv's first line names no snapshot instant")
        rootfs = []
        status = open(os.path.join(lists, f"rootfs-{arch}.dpkg-status"), "rb").read()
        for paragraph in status.decode().split("\n\n"):
            if not paragraph.strip():
                continue
            fields = deb_fields(paragraph)
            if fields["Status"] != "install ok installed":
                die(f"rootfs-{arch}.dpkg-status: {fields['Package']} has status {fields['Status']}")
            source, _, source_version = fields.get("Source", fields["Package"]).partition(" ")
            rootfs.append({"package": fields["Package"], "version": fields["Version"], "architecture": fields["Architecture"], "source": source, "source_version": source_version.strip("()") or fields["Version"]})
        pip = table(os.path.join(lists, f"rootfs-{arch}.pip.tsv"), ["name", "version", "folder"])
        found[("guest", arch)] = {"debian": guest, "pip": [], "snapshot": snapshot.group(1), "deb_arch": deb_arch}
        found[("rootfs", arch)] = {"debian": rootfs, "pip": pip, "deb_arch": deb_arch, "status": status}
    return found


def read_lock(path):
    """The release's name (sandbox-assets.sh's rule: sha256 of the lock without its url line) and its
    assets' lines (kind, architecture, file, sha256, size); the assets are in the lock's folder."""
    text = open(path).read()
    body, _, url = text.rpartition("url\t")
    if not url.strip():
        die(f"{path} has no url line")
    assets = [line.split("\t") for line in body.split("\n") if line and not line.startswith("#")]
    return "sandbox-assets-" + hashlib.sha256(body.encode()).hexdigest()[:16], assets


# The parts that are not Debian packages.

def passt_version():
    """pasta's version as rootfs/build.sh pins it."""
    found = re.search(r"^PASST=(\S+)$", open(os.path.join(NATIVE, "crates/sandbox/rootfs/build.sh")).read(), re.M)
    if not found:
        die("rootfs/build.sh pins no PASST")
    return found.group(1)


def read_other(found):
    """other.tsv's rows, its pip rows checked against each pip list and pasta's against rootfs/build.sh."""
    rows = table(os.path.join(HERE, "other.tsv"), ["userland", "component", "version", "pip", "licence", "family", "source"])
    for (userland, arch), lists in found.items():
        listed = {(p["name"], p["version"]) for p in lists["pip"]}
        described = {(o["pip"], o["version"]) for o in rows if o["userland"] == userland and o["pip"] != "-"}
        if listed != described:
            die(f"rootfs-{arch}.pip.tsv lists {sorted(listed)}, other.tsv describes {sorted(described)}: look at what the new distributions bundle, their licences included, and change other.tsv")
    pasta = [o for o in rows if o["source"].startswith("release:passt-")]
    if len(pasta) != 1 or pasta[0]["version"] != passt_version() or pasta[0]["source"] != f"release:passt-{passt_version()}.tar.gz":
        die(f"other.tsv's pasta row is not passt {passt_version()}, which rootfs/build.sh pins")
    return rows


# snapshot.debian.org.

def request(url):
    """GET url, one at a time after a pause; a rate limit or a server error is waited out up to five times."""
    for attempt in range(1, 7):
        time.sleep(PAUSE)
        try:
            return urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "mcbench-lite-sources/1"}), timeout=120)
        except urllib.error.HTTPError as e:
            if e.code not in (429, 500, 502, 503, 504) or attempt == 6:
                die(f"{url} answered {e.code}")
            wait = int(e.headers.get("Retry-After", "0") or 0) or 30 * attempt
        except (urllib.error.URLError, TimeoutError) as e:
            if attempt == 6:
                die(f"{url}: {e}")
            wait = 30 * attempt
        print(f"[sources] {url}: waiting {wait} s, attempt {attempt + 1}", file=sys.stderr)
        time.sleep(wait)


def get_json(path):
    with request(SNAPSHOT + path) as r:
        return json.load(r)


def quote(text):
    return urllib.parse.quote(text, safe="")


def resolve_guest(package):
    """A guest package's source and version, from snapshot.debian.org's record of that binary package
    (the list's version is the pool file name's, which has no epoch)."""
    result = get_json(f"/mr/binary/{quote(package['package'])}/")["result"]
    found = [r for r in result if no_epoch(r["binary_version"]) == no_epoch(package["version"])]
    if len(found) != 1:
        die(f"snapshot.debian.org knows {len(found)} binary packages {package['package']} {package['version']}")
    return {"package": package["package"], "version": package["version"], "architecture": package["architecture"], "source": found[0]["source"], "source_version": found[0]["version"]}


def fetch(dest, sha1, sha256=None):
    """The file of sha1 at dest, fetched unless dest already has it; sha256 checked when given."""
    if os.path.exists(dest):
        if (sha256 and sha256_file(dest) == sha256) or (not sha256 and hashlib.sha1(open(dest, "rb").read()).hexdigest() == sha1):
            return
        os.remove(dest)
    print(f"[sources] fetching {os.path.basename(dest)}", file=sys.stderr)
    h1, h256 = hashlib.sha1(), hashlib.sha256()
    with request(f"{SNAPSHOT}/file/{sha1}") as r, open(dest + ".part", "wb") as out:
        for block in iter(lambda: r.read(1 << 20), b""):
            h1.update(block)
            h256.update(block)
            out.write(block)
    if h1.hexdigest() != sha1:
        die(f"{dest}: snapshot.debian.org sent sha1 {h1.hexdigest()}, not {sha1}")
    if sha256 and h256.hexdigest() != sha256:
        die(f"{dest}: sha256 {h256.hexdigest()}, the .dsc names {sha256}")
    os.replace(dest + ".part", dest)


def dsc_files(text):
    """The .dsc's fields and (name, sha1, sha256) of every file it names."""
    fields = deb_fields(re.sub(r"\A-----BEGIN PGP SIGNED MESSAGE-----\nHash: \S+\n\n", "", text).split("\n\n")[0])
    def sums(key):
        return {parts[2]: parts[0] for parts in (line.split() for line in fields[key].split("\n")[1:])}
    sha1, sha256 = sums("Checksums-Sha1"), sums("Checksums-Sha256")
    if set(sha1) != set(sha256):
        die("a .dsc whose Checksums-Sha1 and Checksums-Sha256 name different files")
    return fields, [(name, sha1[name], sha256[name]) for name in sorted(sha256)]


def fetch_source(out, source, version):
    """A source package into OUT/debian/<source>_<version without epoch>/: its folder and (file, sha256)."""
    folder = os.path.join(out, "debian", f"{source}_{no_epoch(version)}")
    os.makedirs(folder, exist_ok=True)
    info = get_json(f"/mr/package/{quote(source)}/{quote(version)}/srcfiles?fileinfo=1")["fileinfo"]
    dsc_name = f"{source}_{no_epoch(version)}.dsc"
    dsc_sha1 = [h for h, seen in info.items() if any(s["name"] == dsc_name for s in seen)]
    if len(dsc_sha1) != 1:
        die(f"snapshot.debian.org has {len(dsc_sha1)} files {dsc_name}")
    dsc_path = os.path.join(folder, dsc_name)
    fetch(dsc_path, dsc_sha1[0])
    fields, files = dsc_files(open(dsc_path, encoding="utf-8", errors="replace").read())
    if fields["Source"] != source or fields["Version"] != version:
        die(f"{dsc_name} is the source of {fields['Source']} {fields['Version']}")
    for name, sha1, sha256 in files:
        fetch(os.path.join(folder, name), sha1, sha256)
    return folder, [(dsc_name, sha256_file(dsc_path))] + [(name, sha256) for name, _, sha256 in files]


# Licences. A root filesystem package's are in its usr/share/doc/<package>/copyright, read from
# the asset (some source packages write that file while they build, libxcrypt's from a template
# and upstream's LICENSING, so a source package's own debian/copyright can be missing or
# incomplete). The guest base keeps no documentation, so its packages' are read from their
# source packages' debian/copyright, which all seven have.

def source_copyright(folder, files):
    """The source package's debian/copyright, from its Debian tarball, its diff or its native tarball."""
    names = [n for n, _ in files]
    debian = [n for n in names if re.search(r"\.debian\.tar\.(xz|gz|bz2)$", n)]
    diff = [n for n in names if n.endswith(".diff.gz")]
    if debian:
        with tarfile.open(os.path.join(folder, debian[0])) as tar:
            for info in tar:
                if info.isfile() and info.name.removeprefix("./") == "debian/copyright":
                    return tar.extractfile(info).read().decode("utf-8", "replace")
    elif diff:
        text = zlib.decompress(open(os.path.join(folder, diff[0]), "rb").read(), 47).decode("utf-8", "replace")
        hunk = re.search(r"^\+\+\+ [^/\n]+/debian/copyright\n@@[^\n]*\n((?:[+ \\][^\n]*\n)*)", text, re.M)
        if hunk:
            return "".join(line[1:] + "\n" for line in hunk.group(1).split("\n") if line.startswith("+"))
    else:
        tars = [n for n in names if re.search(r"\.tar\.(xz|gz|bz2)$", n)]
        if len(tars) != 1:
            die(f"{folder}: a native source package with {len(tars)} tarballs")
        with tarfile.open(os.path.join(folder, tars[0])) as tar:
            for info in tar:
                if info.isfile() and re.fullmatch(r"[^/]+/debian/copyright", info.name):
                    return tar.extractfile(info).read().decode("utf-8", "replace")
    die(f"{folder} has no debian/copyright")


def rootfs_copyrights(path, status):
    """Every package's usr/share/doc/<package>/copyright in a root filesystem asset, by package;
    its dpkg status must be the list's byte for byte."""
    copyrights, links, found = {}, {}, None
    zstd = subprocess.Popen(["zstd", "-dc", path], stdout=subprocess.PIPE)
    with tarfile.open(fileobj=zstd.stdout, mode="r|") as tar:
        for info in tar:
            if info.name == "rootfs/var/lib/dpkg/status":
                found = tar.extractfile(info).read()
            elif info.isreg() and re.fullmatch(r"rootfs/usr/share/doc/[^/]+/copyright", info.name):
                copyrights[info.name] = tar.extractfile(info).read().decode("utf-8", "replace")
            elif info.issym() and info.name.startswith("rootfs/usr/share/doc/"):
                links[info.name] = info.linkname
    zstd.stdout.close()
    if zstd.wait() != 0:
        die(f"zstd could not read {path}")
    if found != status:
        die(f"{path}'s dpkg status is not the list's")
    def of(package):
        # Debian's policy: the folder may be a link to another package's of the same source.
        doc = f"rootfs/usr/share/doc/{package}"
        while doc in links:
            target = links[doc]
            doc = "rootfs" + target if target.startswith("/") else os.path.normpath(os.path.join(os.path.dirname(doc), target))
        if f"{doc}/copyright" not in copyrights:
            die(f"{path} has no usr/share/doc/{package}/copyright")
        return copyrights[f"{doc}/copyright"]
    return of


OTHER_COPYLEFT = re.compile(r"^(AGPL|MPL|EPL|CDDL|CPL|OSL|CeCILL|EUPL|CC-BY-SA|GFDL|Sleepycat|QPL|APSL|Artistic-1)", re.I)


def licence_of(text):
    """(family, licences): family is GPL, LGPL, copyleft-other joined by +, or none; licences the copyright file's names."""
    families = set()
    if re.search(r"^Format:\s*\S*copyright-format", text, re.M | re.I):
        names = sorted({tsv(v) for v in re.findall(r"^License:[ \t]*(\S[^\n]*)$", text, re.M)})
        for name in names:
            for token in re.split(r"[\s,()/]+|\bor\b|\band\b|\bwith\b", name):
                if re.match(r"LGPL", token, re.I):
                    families.add("LGPL")
                elif re.match(r"GPL", token, re.I):
                    families.add("GPL")
                elif OTHER_COPYLEFT.match(token):
                    families.add("copyleft-other")
        licences = "; ".join(names)
    else:
        if re.search(r"L(esser|ibrary) General Public|\bLGPL", text):
            families.add("LGPL")
        if re.search(r"(?<!Lesser )(?<!Library )GNU General Public License|\bGPL", text):
            families.add("GPL")
        if re.search(r"Mozilla Public License|GNU Free Documentation|Affero", text):
            families.add("copyleft-other")
        licences = "(not machine-readable; its text names: " + (", ".join(sorted(families)) or "no copyleft licence") + ")"
    return "+".join(f for f in ["GPL", "LGPL", "copyleft-other"] if f in families) or "none", licences


# The bundle.

def expected_rows(found, others):
    """Every binary package and part, as the KEY columns, from the lists and other.tsv (no network:
    a guest package's source is taken from MANIFEST.tsv, and the check holds it to the list's name
    and version)."""
    keys = set()
    for (userland, arch), lists in found.items():
        for p in lists["debian"]:
            keys.add((userland, arch, p["package"], p["version"], p["architecture"]) + ((p["source"], p["source_version"]) if userland == "rootfs" else ()))
        for o in others:
            if o["userland"] == userland:
                keys.add((userland, arch, o["component"], o["version"], lists["deb_arch"], o["component"], o["version"]))
    return keys


def build(found, others, release, assets, lock, out):
    copyrights = {}
    for kind, arch, name, sha256, size in assets:
        if kind == "rootfs":
            path = os.path.join(os.path.dirname(os.path.abspath(lock)), name)
            if not os.path.isfile(path) or os.path.getsize(path) != int(size) or sha256_file(path) != sha256:
                die(f"{path} is missing or is not the lock's ({sha256}, {size} bytes)")
            print(f"[sources] reading the copyright files in {name}", file=sys.stderr)
            copyrights[arch] = rootfs_copyrights(path, found[("rootfs", arch)]["status"])
    binaries = []
    for (userland, arch), lists in sorted(found.items()):
        for p in lists["debian"]:
            b = {"userland": userland, "arch": arch, **(resolve_guest(p) if userland == "guest" else p)}
            if userland == "rootfs":
                b["licence_family"], b["licences"] = licence_of(copyrights[arch](p["package"]))
            binaries.append(b)
    sources = sorted({(b["source"], b["source_version"]) for b in binaries})
    print(f"[sources] {len(binaries)} binary packages from {len(sources)} source packages", file=sys.stderr)
    fetched = {}
    for i, (source, version) in enumerate(sources, 1):
        print(f"[sources] {i}/{len(sources)} {source} {version}", file=sys.stderr)
        fetched[(source, version)] = fetch_source(out, source, version)
    for b in binaries:
        if b["userland"] == "guest":
            b["licence_family"], b["licences"] = licence_of(source_copyright(*fetched[(b["source"], b["source_version"])]))
    rows = []
    for b in binaries:
        folder, files = fetched[(b["source"], b["source_version"])]
        for name, sha256 in files:
            rows.append({**b, "file": os.path.relpath(os.path.join(folder, name), out), "sha256": sha256})
    for (userland, arch), lists in sorted(found.items()):
        for o in others:
            if o["userland"] == userland:
                rows.append({"userland": userland, "arch": arch, "package": o["component"], "version": o["version"], "architecture": lists["deb_arch"], "source": o["component"], "source_version": o["version"], "licence_family": o["family"], "licences": o["licence"], "file": o["source"], "sha256": "-"})
    rows.sort(key=lambda r: [r[c] for c in COLUMNS])
    with open(os.path.join(out, "MANIFEST.tsv"), "w") as f:
        f.write("\t".join(COLUMNS) + "\n")
        for r in rows:
            f.write("\t".join(tsv(r[c]) for c in COLUMNS) + "\n")
    write_readme(found, others, release, assets, out, rows, fetched)


def human(size):
    return f"{size / 1e6:,.1f} MB"


def write_readme(found, others, release, assets, out, rows, fetched):
    binaries = {(r["userland"], r["arch"], r["package"]): r for r in rows if r["file"].startswith("debian/")}
    families = {}
    for b in binaries.values():
        families[b["licence_family"]] = families.get(b["licence_family"], 0) + 1
    def size(keys):
        return sum(os.path.getsize(os.path.join(fetched[k][0], n)) for k in keys for n, _ in fetched[k][1])
    copyleft = sorted({(b["source"], b["source_version"]) for b in binaries.values() if b["licence_family"] != "none"})
    snapshot = found[("guest", "x86_64")]["snapshot"]
    lines = [
        "# Sources of the bash sandbox's Linux userlands",
        "",
        f"This bundle belongs to the release `{release}` of mcbench-lite's sandbox assets. It holds the source code of every Debian package in the Linux userlands that release holds. The guest base is the small Linux system the macOS app boots in a virtual machine and the Windows app imports as a WSL distribution; the macOS and Windows apps ship it. The root filesystem is the Linux system the model's bash shell runs in; the app downloads it from that release. The source of pasta, the one other program in them whose licence asks for its source, is `{next(o['source'] for o in others if o['source'].startswith('release:')).removeprefix('release:')}` in the same release.",
        "",
        "| Asset of the release | sha256 | Size in bytes |",
        "|---|---|---|",
    ]
    lines += [f"| `{a[2]}` | `{a[3]}` | {a[4]} |" for a in assets]
    lines += [
        "",
        f"It holds {len(fetched)} Debian source packages, {human(size(fetched))}, for {len(binaries)} binary packages counted once per userland and architecture. By the licences each binary package's copyright file names:",
        "",
        "| Licence family | Binary packages |",
        "|---|---|",
    ]
    lines += [f"| {family} | {count} |" for family, count in sorted(families.items())]
    lines += [
        "",
        f"The {len(copyleft)} source packages of the binary packages whose copyright file names a copyleft licence (GPL, LGPL or another) are {human(size(copyleft))} of it. `none` means that the copyright file names none. Every source package is here, so that the bundle does not depend on how complete each copyright file is.",
        "",
        "## How it was made",
        "",
        f"`native/packaging/sources.sh` in mcbench-lite's repository made it from the package lists that `native/packaging/sandbox-assets.sh build` read from the assets: each guest base's `manifest.json` (the `.deb` files `native/crates/sandbox/guest/build.py` pins by sha256 at snapshot.debian.org {snapshot}), each root filesystem's `var/lib/dpkg/status` and the Python distributions pip installed in it. A root filesystem package's source package and version are its `Source` field (its own name and version when it has none); a guest base package's are snapshot.debian.org's record of that binary package and version. Each source package's `.dsc` and every file it names were fetched from snapshot.debian.org, which keeps every file the Debian archive has published, and every file's sha256 was checked against the `.dsc`. The script's last step checks that `MANIFEST.tsv` names exactly the packages of those lists, that every file a row names is here with that sha256, and that every file here is named by a row.",
        "",
        "A source package's folder is `debian/<source>_<version>/`, the version without its epoch as in Debian's own file names. `dpkg-source -x <the .dsc>` unpacks it with Debian's changes applied.",
        "",
        "## MANIFEST.tsv",
        "",
        "One row per binary package and file of its source, tab-separated, with a header line. `userland` is `guest` or `rootfs`; `arch` is `x86_64` or `aarch64`; `version` and `architecture` are the binary package's as the lists give them (a guest base package's version is its pool file name's, which leaves out an epoch); `licence_family` is `GPL`, `LGPL`, `copyleft-other` (AGPL, MPL, EPL, CDDL, GFDL and similar), several of these joined by `+`, or `none`; `licences` lists the licence names in the binary package's copyright file: for the root filesystem its `usr/share/doc/<package>/copyright`, for the guest base, which keeps no documentation, its source package's `debian/copyright` (or, when that file is not in Debian's machine-readable format, which families its text names). A row whose `file` is a URL or `release:<file>` names a part that is not a Debian package: its source is at that address or is that file of the same release.",
        "",
        "## Parts that are not Debian packages",
        "",
        "| Userland | Part | Version | Licence | Source |",
        "|---|---|---|---|---|",
    ]
    for o in others:
        where = f"`{o['source'].removeprefix('release:')}` in the same release" if o["source"].startswith("release:") else o["source"]
        lines.append(f"| {o['userland']} | {o['component']} | {o['version']} | {o['licence']} | {where} |")
    lines.append("")
    with open(os.path.join(out, "README.md"), "w") as f:
        f.write("\n".join(lines))


# The completeness check.

def check(found, others, out):
    expected = expected_rows(found, others)
    with open(os.path.join(out, "MANIFEST.tsv")) as f:
        if f.readline().rstrip("\n").split("\t") != COLUMNS:
            die("MANIFEST.tsv's header is not " + " ".join(COLUMNS))
        rows = [dict(zip(COLUMNS, line.rstrip("\n").split("\t"), strict=True)) for line in f]
    have = set()
    for r in rows:
        key = tuple(r[c] for c in KEY)
        # A guest package's source comes from snapshot.debian.org, not the list.
        have.add(key[:5] if r["userland"] == "guest" and r["file"].startswith("debian/") else key)
    missing, extra = sorted(expected - have), sorted(have - expected)
    if missing:
        die(f"{len(missing)} packages of the lists have no row in MANIFEST.tsv: {missing[:10]}")
    if extra:
        die(f"{len(extra)} rows of MANIFEST.tsv name a package the lists do not: {extra[:10]}")
    named = {}
    for r in rows:
        if re.match(r"https?://|release:", r["file"]):
            continue
        if named.setdefault(r["file"], r["sha256"]) != r["sha256"]:
            die(f"MANIFEST.tsv gives {r['file']} two sha256")
    for path, sha256 in sorted(named.items()):
        full = os.path.join(out, path)
        if not os.path.isfile(full):
            die(f"{path} is in MANIFEST.tsv and not in {out}")
        if sha256_file(full) != sha256:
            die(f"{path} does not have the sha256 MANIFEST.tsv gives")
        if path.endswith(".dsc"):
            folder = os.path.dirname(path)
            for name, _, dsc_sha256 in dsc_files(open(full, encoding="utf-8", errors="replace").read())[1]:
                if named.get(f"{folder}/{name}") != dsc_sha256:
                    die(f"{path} names {name} with sha256 {dsc_sha256}, which no row of MANIFEST.tsv has")
    present = {os.path.relpath(os.path.join(root, name), out) for root, _, files in os.walk(out) for name in files}
    unnamed = sorted(present - set(named) - {"MANIFEST.tsv", "README.md"})
    if unnamed:
        die(f"{len(unnamed)} files in {out} are in no row of MANIFEST.tsv: {unnamed[:10]}")
    sources = {(r["source"], r["source_version"]) for r in rows if r["file"].startswith("debian/")}
    print(f"check: {len(expected)} packages and parts, {len(sources)} Debian source packages, {len(named)} files, {sum(os.path.getsize(os.path.join(out, p)) for p in named)} bytes: complete")


def main():
    args = sys.argv[1:]
    only_check = args[:1] == ["--check"]
    args = args[1:] if only_check else args
    if len(args) != 3:
        die("usage: sources.py [--check] LISTS LOCK OUT")
    lists, lock, out = args[0], args[1], os.path.abspath(args[2])
    found = read_lists(lists)
    others = read_other(found)
    release, assets = read_lock(lock)
    os.makedirs(out, exist_ok=True)
    if not only_check:
        build(found, others, release, assets, lock, out)
    check(found, others, out)


main()
