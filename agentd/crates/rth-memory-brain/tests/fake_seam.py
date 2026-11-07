#!/usr/bin/python3
# A fake memory seam for the People tests: no Brain, no bun. It answers every search with every
# RTH item the fake brain-mcp (fake_brain_mcp.py) holds in $HOME/fake-brain.json, newest first,
# whatever the query: it proves what RTH keeps from a ranked hit list, not Brain's ranking.
import json, os, sys

STATE = os.path.join(os.environ["HOME"], "fake-brain.json")
print(json.dumps({"ready": True}, separators=(",", ":")), flush=True)
for line in sys.stdin:
    r = json.loads(line)
    try:
        with open(STATE) as f:
            mems = json.load(f)["mems"]
    except FileNotFoundError:
        mems = {}
    refs = sorted((m for m, v in mems.items() if "rth_memory_id" in v["rows"][-1]["metadata"]), key=int, reverse=True)
    print(json.dumps({"id": r["id"], "ok": True, "hits": [{"brain_ref": m} for m in refs[: r["limit"]]]}, separators=(",", ":")), flush=True)
