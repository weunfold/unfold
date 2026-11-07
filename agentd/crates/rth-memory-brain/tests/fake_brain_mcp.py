#!/usr/bin/python3
# A fake brain-mcp for the forget-mirror tests: no Brain, no database. It keeps one brain's
# memories and revisions in $HOME/fake-brain.json (so a restart, a new process, sees them) and
# follows Brain's revision seam for the tools RTH dispatches (brain src/lib/brain-memory):
# remember (with its content dedup), get, history, update (People tests), invalidate (store.ts:1010-1077) and restore (store.ts:1288-1370), with
# operation-key replay checked before the revision (revisions.ts:394-414). It is a fixture: it
# proves what RTH sends and how RTH handles the answers, not Brain's semantics.
#
# Every tools/call is logged to $HOME/fake-brain-calls.jsonl. $HOME/fake-brain-fail holds lines
# `<tool>:before` (answer a JSON-RPC error, write nothing) or `<tool>:after` (write, then answer
# the error: a lost answer).
import json, os, sys

home = os.environ["HOME"]
STATE = os.path.join(home, "fake-brain.json")
CALLS = os.path.join(home, "fake-brain-calls.jsonl")
FAIL = os.path.join(home, "fake-brain-fail")
BRAIN = os.environ.get("BRAIN_ID", "brn-fake")


def load():
    try:
        with open(STATE) as f:
            return json.load(f)
    except FileNotFoundError:
        return {"next": 1, "mems": {}, "ops": {}}


def save(st):
    tmp = STATE + ".tmp"
    with open(tmp, "w") as f:
        json.dump(st, f)
    os.replace(tmp, STATE)


def fail_mode(tool):
    try:
        with open(FAIL) as f:
            for line in f:
                t, _, mode = line.strip().partition(":")
                if t == tool:
                    return mode
    except FileNotFoundError:
        pass
    return None


class Refused(Exception):
    def __init__(self, body):
        self.body = body


def digest(d):
    return json.dumps(d, sort_keys=True)


def replay(st, key, dg):
    op = st["ops"].get(key) if key else None
    if op is None:
        return None
    if op["digest"] != dg:
        raise Refused({"code": "IDEMPOTENCY_CONFLICT", "operation_key": key})
    return op


def head_of(st, mid):
    m = st["mems"].get(mid)
    if m is None:
        raise Refused({"code": "MEMORY_NOT_FOUND", "memory_id": mid})
    return m["rows"][-1]


def record(row):
    return {k: row[k] for k in ("memory_id", "revision", "content", "metadata", "invalidated_at", "created_at")} | {"brain_id": BRAIN}


def call(st, tool, a):
    if tool == "brain_remember":
        key = a.get("operation_key")
        op = replay(st, key, digest({"content": a["content"]}))
        if op:
            return {"brain_id": BRAIN, "memory_id": op["memory_id"], "revision": op["revision"], "action": "existing"}
        # Brain's dedup hit (store.ts planRememberDedupeUpdate): the same content is the same
        # memory; supplied metadata that differs replaces the stored object in a new revision.
        same = next((m for m, v in st["mems"].items() if v["rows"][-1]["content"] == a["content"]), None)
        if same is not None:
            head = st["mems"][same]["rows"][-1]
            if "metadata" not in a or a["metadata"] == head["metadata"]:
                return {"brain_id": BRAIN, "memory_id": same, "revision": head["revision"], "action": "existing", "changed_fields": []}
            row = dict(head, revision=head["revision"] + 1, metadata=a["metadata"], operation_key=key, restored_from_revision=None)
            st["mems"][same]["rows"].append(row)
            if key:
                st["ops"][key] = {"digest": digest({"content": a["content"]}), "memory_id": same, "revision": row["revision"]}
            return {"brain_id": BRAIN, "memory_id": same, "revision": row["revision"], "action": "existing", "changed_fields": ["metadata"]}
        mid = str(st["next"])
        st["next"] += 1
        row = {"memory_id": mid, "revision": 1, "content": a["content"], "metadata": a.get("metadata", {}), "invalidated_at": None, "created_at": "2026-10-25T00:00:00.000Z", "operation_key": key, "restored_from_revision": None}
        st["mems"][mid] = {"rows": [row]}
        if key:
            st["ops"][key] = {"digest": digest({"content": a["content"]}), "memory_id": mid, "revision": 1}
        return {"brain_id": BRAIN, "memory_id": mid, "revision": 1, "action": "inserted"}
    if tool == "brain_get":
        return record(head_of(st, a["memory_id"]))
    if tool == "brain_memory_history":
        mid = a["memory_id"]
        head_of(st, mid)
        rows = list(reversed(st["mems"][mid]["rows"]))
        off, lim = a.get("offset", 0), a.get("limit", 20)
        return {"brain_id": BRAIN, "memory_id": mid, "revisions": rows[off:off + lim], "limit": lim, "offset": off}
    if tool == "brain_invalidate":
        mid, key = a["memory_id"], a.get("operation_key")
        dg = digest({"memory_id": mid, "replacement_id": a.get("replacement_id"), "expected_revision": a.get("expected_revision")})
        op = replay(st, key, dg)
        if op:
            return {"memory_id": mid, "invalidated": True, "replacement_id": None, "revision": op["revision"]}
        head = head_of(st, mid)
        if "expected_revision" in a and a["expected_revision"] != head["revision"]:
            raise Refused({"code": "REVISION_CONFLICT", "expected_revision": a["expected_revision"], "actual_revision": head["revision"]})
        row = dict(head, revision=head["revision"] + 1, invalidated_at="2026-10-25T00:00:01.000Z", operation_key=key, restored_from_revision=None)
        st["mems"][mid]["rows"].append(row)
        if key:
            st["ops"][key] = {"digest": dg, "memory_id": mid, "revision": row["revision"]}
        return {"memory_id": mid, "invalidated": True, "replacement_id": None, "revision": row["revision"]}
    if tool == "brain_memory_restore":
        mid, key, target = a["memory_id"], a.get("operation_key"), a["revision"]
        dg = digest({"memory_id": mid, "revision": target, "expected_revision": a["expected_revision"]})
        op = replay(st, key, dg)
        if op:
            row = st["mems"][mid]["rows"][op["revision"] - 1]
            return {"brain_id": BRAIN, "memory_id": mid, "restored_from_revision": target, "revision": op["revision"], "replayed": True, "record": record(row)}
        head = head_of(st, mid)
        if a["expected_revision"] != head["revision"]:
            raise Refused({"code": "REVISION_CONFLICT", "expected_revision": a["expected_revision"], "actual_revision": head["revision"]})
        rows = st["mems"][mid]["rows"]
        if not 1 <= target <= len(rows):
            raise Refused({"code": "REVISION_NOT_FOUND", "memory_id": mid, "revision": target})
        src = rows[target - 1]
        row = dict(head, revision=head["revision"] + 1, content=src["content"], metadata=src["metadata"], invalidated_at=src["invalidated_at"], operation_key=key, restored_from_revision=target)
        rows.append(row)
        if key:
            st["ops"][key] = {"digest": dg, "memory_id": mid, "revision": row["revision"]}
        return {"brain_id": BRAIN, "memory_id": mid, "restored_from_revision": target, "revision": row["revision"], "replayed": False, "record": record(row)}
    if tool == "brain_update":
        # Brain's update of `content` (store.ts update path): the next revision keeps the item's
        # metadata, compare-and-set on `expected_revision`.
        mid, key = a["memory_id"], a.get("operation_key")
        dg = digest({"memory_id": mid, "content": a["content"], "expected_revision": a.get("expected_revision")})
        op = replay(st, key, dg)
        if op:
            return {"brain_id": BRAIN, "memory_id": mid, "revision": op["revision"], "action": "existing"}
        head = head_of(st, mid)
        if "expected_revision" in a and a["expected_revision"] != head["revision"]:
            raise Refused({"code": "REVISION_CONFLICT", "expected_revision": a["expected_revision"], "actual_revision": head["revision"]})
        row = dict(head, revision=head["revision"] + 1, content=a["content"], operation_key=key, restored_from_revision=None)
        st["mems"][mid]["rows"].append(row)
        if key:
            st["ops"][key] = {"digest": dg, "memory_id": mid, "revision": row["revision"]}
        return {"brain_id": BRAIN, "memory_id": mid, "revision": row["revision"], "action": "updated"}
    raise Refused({"message": tool + " is not served by this fake"})


for line in sys.stdin:
    r = json.loads(line)
    if "id" not in r:
        continue
    if r["method"] != "tools/call":
        print(json.dumps({"jsonrpc": "2.0", "id": r["id"], "result": {}}), flush=True)
        continue
    tool, args = r["params"]["name"], r["params"]["arguments"]
    with open(CALLS, "a") as f:
        f.write(json.dumps({"tool": tool, "arguments": args}) + "\n")
    mode = fail_mode(tool)
    if mode == "before":
        print(json.dumps({"jsonrpc": "2.0", "id": r["id"], "error": {"code": -32000, "message": "fake: storage unavailable"}}), flush=True)
        continue
    st = load()
    try:
        v = call(st, tool, args)
        save(st)
        result = {"content": [{"type": "text", "text": json.dumps(v)}]}
    except Refused as e:
        result = {"isError": True, "content": [{"type": "text", "text": tool + " failed: " + json.dumps(e.body)}]}
    if mode == "after":
        print(json.dumps({"jsonrpc": "2.0", "id": r["id"], "error": {"code": -32000, "message": "fake: answer lost after the write"}}), flush=True)
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": r["id"], "result": result}), flush=True)
