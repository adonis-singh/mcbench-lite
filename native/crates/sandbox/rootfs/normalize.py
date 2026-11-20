"""normalize.py EXPORT ENV PASTA IMAGE DOCKERFILE_SHA256 PASST CREATED PASTA_AVX2 > rootfs.tar

The root filesystem's tarball from `docker export`: every entry under
rootfs/, sorted by name, owned by 0:0 with no owner names, /.dockerenv and
device nodes and fifos left out, mtimes kept. A hard link group's first
name (in sorted order) holds the data and the others link to it, so a link
never comes before its target. image.json (the image's environment and what
it was built from), pasta and, when PASTA_AVX2 is not empty, pasta.avx2
(both 0755) come first, with mtime 0.
"""
import io
import json
import sys
import tarfile


def clean(name):
    # docker export names entries without a leading ./, but strip one if
    # there; lstrip('./') would also eat a dot-file's own dot.
    if name.startswith('./'):
        name = name[2:]
    return name.rstrip('/')


export, env_file, pasta, image, dockerfile, passt, created, pasta_avx2 = sys.argv[1:9]
created = int(created)
source = tarfile.open(export)
members = {}
data = {}
for member in source:
    name = clean(member.name)
    if not name or name == '.dockerenv':
        continue
    if member.ischr() or member.isblk() or member.isfifo():
        continue
    member.name = name
    members[name] = member
    if member.isreg():
        data[name] = source.extractfile(member).read()

# Hard link groups, each led by its first name.
groups = {}
for name, member in members.items():
    if member.islnk():
        target = clean(member.linkname)
        groups.setdefault(target, [target]).append(name)
leader = {}
for target, names in groups.items():
    names.sort()
    for name in names:
        leader[name] = names[0]

out = tarfile.open(fileobj=sys.stdout.buffer, mode='w|', format=tarfile.GNU_FORMAT)


def plain(info):
    info.uid = info.gid = 0
    info.uname = info.gname = ''
    info.pax_headers = {}
    return info


env = json.load(open(env_file))
meta = json.dumps({'env': env, 'image': image, 'dockerfile_sha256': dockerfile, 'pasta': f'passt {passt}'}, indent=2).encode() + b'\n'
entries = [('image.json', meta, 0o644), ('pasta', open(pasta, 'rb').read(), 0o755)]
if pasta_avx2:
    entries.append(('pasta.avx2', open(pasta_avx2, 'rb').read(), 0o755))
for name, blob, mode in entries:
    info = plain(tarfile.TarInfo(name))
    info.size, info.mode, info.mtime = len(blob), mode, 0
    out.addfile(info, io.BytesIO(blob))

root = plain(tarfile.TarInfo('rootfs'))
root.type, root.mode, root.mtime = tarfile.DIRTYPE, 0o755, 0
out.addfile(root)
for name in sorted(members):
    member = plain(members[name])
    # What docker export made itself (/dev, /etc/hosts, ...) carries the
    # export's time: no mtime is later than the image's creation.
    member.mtime = min(member.mtime, created)
    blob = None
    if name in leader:
        first = leader[name]
        if name == first:
            target = first if not members[first].islnk() else clean(members[first].linkname)
            blob = data[target]
            member.type, member.linkname, member.size = tarfile.REGTYPE, '', len(blob)
        else:
            member.type, member.linkname, member.size = tarfile.LNKTYPE, 'rootfs/' + first, 0
    elif member.isreg():
        blob = data[name]
    member.name = 'rootfs/' + name
    out.addfile(member, io.BytesIO(blob) if blob is not None else None)
out.close()
