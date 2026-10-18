"""Synthetic MCP peer for Rust transport tests; Python is never a product dependency."""
import json
import os
from pathlib import Path
import sys
import time

mode, directory, *literal_args = sys.argv[1:]
root = Path(directory)
root.joinpath("pid").write_text(str(os.getpid()))


def send(value):
    data = json.dumps(value).encode() + b"\n"
    # Deliberately split JSON and the final newline across writes.
    for start in range(0, len(data), 17):
        os.write(1, data[start:start + 17])


def result(request, value):
    send({"jsonrpc": "2.0", "id": request["id"], "result": value})


initialized = False
for line in sys.stdin:
    request = json.loads(line)
    with root.joinpath("requests").open("a") as log:
        log.write(json.dumps(request) + "\n")
    method = request.get("method")
    if method == "initialize":
        assert request["params"]["capabilities"] == {}
        result(request, {"protocolVersion": "2025-03-26", "capabilities": {"tools": {}},
                         "serverInfo": {"name": "synthetic", "version": "1"}})
    elif method == "notifications/initialized":
        initialized = True
    elif method == "tools/list":
        assert initialized
        if mode == "pages":
            result(request, {"tools": [], "nextCursor": "forever"})
        elif not request["params"].get("cursor"):
            result(request, {"tools": [], "nextCursor": "page-two"})
        else:
            result(request, {"tools": [{"name": "inspect", "description": "Synthetic inspection",
                "inputSchema": {"type": "object", "properties": {}},
                "annotations": {"readOnlyHint": mode != "mutating"}}]})
    elif method == "tools/call":
        root.joinpath("called").touch()
        if mode == "hang":
            time.sleep(60)
        elif mode == "exit":
            sys.exit(0)
        elif mode == "bad-json":
            os.write(1, b"not-json\n")
        elif mode == "wrong-id":
            send({"jsonrpc": "2.0", "id": 9999, "result": {}})
        elif mode == "oversized":
            os.write(1, b"x" * (256 * 1024 + 1))
        elif mode in ("flood", "byte-flood"):
            for _ in range(65):
                send({"jsonrpc": "2.0", "method": "notifications/message",
                      "params": {"data": "x" * (20000 if mode == "byte-flood" else 0)}})
        elif mode == "tool-error":
            result(request, {"isError": True, "content": [{"type": "text", "text": "failed"}]})
        else:
            # stderr must neither deadlock nor contaminate terminal/JSON output.
            os.write(2, b"private diagnostics\n" * 40000)
            send({"jsonrpc": "2.0", "method": "notifications/progress", "params": {"progress": 1}})
            send({"jsonrpc": "2.0", "id": "server-ping", "method": "ping"})
            assert json.loads(sys.stdin.readline())["result"] == {}
            send({"jsonrpc": "2.0", "id": "server-sampling", "method": "sampling/createMessage"})
            assert json.loads(sys.stdin.readline())["error"]["code"] == -32601
            result(request, {"content": [{"type": "text", "text": "ready λ"}],
                             "structuredContent": {"pid": os.getpid(), "args": literal_args,
                              "no_color": os.getenv("NO_COLOR"), "term": os.getenv("TERM"),
                              "credential": os.getenv("PEER_TOKEN"),
                              "unmapped": os.getenv("UNMAPPED_TOKEN")}})
