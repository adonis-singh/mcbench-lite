#!/usr/bin/env python3
"""Exports a COPY of a data folder's database to the folder of JSON the
stand-in core reads (`mcbench-ui-dev --data <folder>`).

    export-data.py <copy of mcbench.sqlite3> <out folder> [<images folder>] [<attachments folder>]

Give it a copy (`cp -c`), never the backup in place and never a live data
folder: it opens the file read-only, but SQLite may still touch its -wal
and -shm files. The out folder is the owner's chats: keep it in scratch,
never commit it.

Writes, in the shapes of `mcbench_ui`'s seam (`src/lib.rs`):

    lists.json        the list stream's line: chats, benches, promptTitles, archived
    chats/<id>.json   per chat (archived ones too): header, messages (every
                      branch's rows: id, parentId, position and payload_json
                      text), tree (headId and each row's role)
    archive.json      GET /api/archive: conversations and benches, epoch ms
    settings.json     what the interface reads, made from the database alone
                      (a data folder's own settings.json holds provider
                      keys and is never read): every provider a chat ran
                      on, enabled, with a key marked as saved; no hidden
                      models, no model order, no favorites
    images/<chat>/    the shot images of the exported chats, when an images
                      folder is given
    attachments/      the stored attachments, when an attachments folder is
                      given (the stand-in answers /api/attachments/ from it)

Standard library only.
"""

import json
import os
import shutil
import sqlite3
import sys

ARCHIVE_DAYS = 7
DAY_MS = 24 * 60 * 60 * 1000


def rows(db, sql, *args):
    cur = db.execute(sql, args)
    names = [d[0] for d in cur.description]
    return [dict(zip(names, r)) for r in cur.fetchall()]


def has_table(db, name):
    return bool(db.execute("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?", (name,)).fetchone())


def active_branch(messages, head):
    by_id = {m["id"]: m for m in messages}
    branch = []
    node = by_id.get(head)
    while node:
        branch.append(node)
        node = by_id.get(node["parent_id"])
    return list(reversed(branch))


def round_chain(nodes, head):
    by_id = {n["id"]: n for n in nodes}
    chain = []
    node = by_id.get(head)
    while node:
        chain.append(node)
        node = by_id.get(node.get("parentId"))
    return list(reversed(chain))


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    database, out = sys.argv[1], sys.argv[2]
    images = sys.argv[3] if len(sys.argv) > 3 else None
    attachments = sys.argv[4] if len(sys.argv) > 4 else None
    db = sqlite3.connect(f"file:{database}?mode=ro", uri=True)
    os.makedirs(os.path.join(out, "chats"), exist_ok=True)

    prompts = {}
    titles = {}
    if has_table(db, "message_prompts"):
        for r in rows(db, """SELECT m.conversation_id AS chat, p.prompt_id AS id, p.title AS title
                             FROM message_prompts p JOIN messages m ON m.id = p.message_id ORDER BY p.rowid"""):
            prompts.setdefault(r["chat"], [])
            if r["id"] not in prompts[r["chat"]]:
                prompts[r["chat"]].append(r["id"])
            titles[r["id"]] = r["title"]

    conversations = rows(db, "SELECT * FROM conversations ORDER BY created_at DESC")
    benches = rows(db, "SELECT * FROM benches ORDER BY created_at DESC")
    listed_chats, archived_chats = [], []
    for c in conversations:
        messages = rows(db, """SELECT id, parent_id, position, payload_json FROM messages
                               WHERE conversation_id = ? ORDER BY position""", c["id"])
        branch = active_branch(messages, c["current_message_id"])
        payloads = [json.loads(m["payload_json"]) for m in branch]
        tree = {
            "headId": c["current_message_id"],
            "messages": [
                {
                    "id": m["id"],
                    "parentId": m["parent_id"],
                    "position": m["position"],
                    "role": json.loads(m["payload_json"]).get("role"),
                    "branchable": json.loads(m["payload_json"]).get("role") == "user"
                    and not json.loads(m["payload_json"]).get("seeBuild"),
                }
                for m in messages
            ],
        }
        header = {
            "id": c["id"],
            "title": c["title"],
            "provider": c["provider"],
            "model": c["model"],
            "effort": c["effort"] or None,
            "runningSince": None,
        }
        with open(os.path.join(out, "chats", f"{c['id']}.json"), "w") as f:
            json.dump(
                {
                    "header": header,
                    # Every branch's rows; the active branch is the head's line.
                    "messages": [
                        {"id": m["id"], "parentId": m["parent_id"], "position": m["position"], "payload": m["payload_json"]}
                        for m in messages
                    ],
                    "tree": tree,
                },
                f,
            )
        if c["archived_at"] is not None:
            if c["bench_id"] is None:
                archived_chats.append({
                    "id": c["id"], "title": c["title"], "provider": c["provider"], "model": c["model"],
                    "archivedAt": c["archived_at"], "expiresAt": c["archived_at"] + ARCHIVE_DAYS * DAY_MS,
                })
            continue
        if not messages:
            continue
        seat = None
        if c["bench_id"] is not None:
            last = payloads[-1] if payloads else {}
            halted = "failed" if last.get("role") == "error" else "stopped" if last.get("stopped") else None
            seat = {
                "bench": c["bench_id"],
                "slot": c["bench_slot"],
                "round": sum(1 for p in payloads if p.get("role") == "user" and not p.get("seeBuild")),
                "halted": halted,
            }
        listed_chats.append({
            "id": c["id"], "title": c["title"], "provider": c["provider"], "model": c["model"],
            "effort": c["effort"] or None, "resolution": c["resolution"], "createdAt": c["created_at"],
            "bench": seat,
            "unread": (c["finished_at"] or 0) > (c["seen_at"] or 0) and c["finished_at"] is not None,
            "messageCount": len(messages), "prompts": prompts.get(c["id"], []), "runningSince": None,
        })

    listed_benches, archived_benches = [], []
    for b in benches:
        nodes = json.loads(b["rounds_json"])
        members = [c for c in conversations if c["bench_id"] == b["id"]]
        if b["archived_at"] is not None:
            archived_benches.append({
                "id": b["id"], "title": b["title"], "archivedAt": b["archived_at"],
                "expiresAt": b["archived_at"] + ARCHIVE_DAYS * DAY_MS,
                # How many models it ran, as GET /api/archive gives it.
                "models": len(members),
            })
            continue
        for node in nodes:
            for link in node.get("prompts") or []:
                titles[link["id"]] = link["title"]
        listed_benches.append({
            "id": b["id"], "title": b["title"], "createdAt": b["created_at"], "head": b["head_round_id"],
            "resolution": max([m["resolution"] for m in members] or [1]),
            "rounds": len(round_chain(nodes, b["head_round_id"])),
            "prompts": sorted({link["id"] for n in nodes for link in (n.get("prompts") or [])}),
            "blind": bool(b["blind"]), "revealed": b["revealed_at"] is not None,
            "order": json.loads(b["order_json"]),
        })

    with open(os.path.join(out, "lists.json"), "w") as f:
        json.dump({
            "chats": listed_chats, "benches": listed_benches, "promptTitles": titles,
            "archived": {"conversations": len(archived_chats), "benches": len(archived_benches)},
        }, f)
    with open(os.path.join(out, "archive.json"), "w") as f:
        json.dump({"conversations": archived_chats, "benches": archived_benches}, f)

    providers = {
        pid: {"enabled": True, "hiddenModels": [], "modelOrder": [], "keySaved": True}
        for pid in sorted({c["provider"] for c in conversations} - {"cursor"})
    }
    with open(os.path.join(out, "settings.json"), "w") as f:
        json.dump({"providers": providers, "modelFavorites": []}, f)

    if images:
        for c in conversations:
            source = os.path.join(images, c["id"])
            if os.path.isdir(source):
                shutil.copytree(source, os.path.join(out, "images", c["id"]), dirs_exist_ok=True)

    if attachments:
        shutil.copytree(attachments, os.path.join(out, "attachments"), dirs_exist_ok=True)

    print(f"{len(listed_chats)} listed chats, {len(archived_chats)} archived, "
          f"{len(listed_benches)} benches, {len(archived_benches)} archived, into {out}")


main()
