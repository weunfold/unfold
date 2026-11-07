"""A minimal MCP server over stdio for rth-mcp's tests: the 2025-11-25 initialize handshake,
one tool, newline-delimited JSON-RPC. It answers server/discover as an unknown method, the way a
server from before the 2026-07-28 spec does."""
import json
import os
import sys

TOOL = {
    "name": "echo",
    "description": "Echo the text and the names of the environment variables this server sees.",
    "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}, "required": ["text"]},
}

if os.environ.get("ECHO_MARK"):
    pids = [os.getpid()]
    if os.environ.get("ECHO_GRANDCHILD"):
        import subprocess
        pids.append(subprocess.Popen(["/bin/sleep", "30"]).pid)
    with open(os.environ["ECHO_MARK"], "a") as f:
        f.write("started " + " ".join(map(str, pids)) + "\n")

for line in sys.stdin:
    msg = json.loads(line)
    if "id" not in msg:
        continue
    method = msg.get("method")
    if method == "initialize":
        out = {"result": {"protocolVersion": "2025-11-25", "capabilities": {"tools": {}}, "serverInfo": {"name": "echo-fixture", "version": "1"}}}
    elif method == "tools/list":
        out = {"result": {"tools": [TOOL]}}
    elif method == "tools/call":
        text = msg["params"]["arguments"]["text"]
        out = {"result": {"content": [{"type": "text", "text": text + "|" + ",".join(sorted(os.environ))}]}}
    elif method == "ping":
        out = {"result": {}}
    else:
        out = {"error": {"code": -32601, "message": "method not found"}}
    out.update({"jsonrpc": "2.0", "id": msg["id"]})
    sys.stdout.write(json.dumps(out) + "\n")
    sys.stdout.flush()
