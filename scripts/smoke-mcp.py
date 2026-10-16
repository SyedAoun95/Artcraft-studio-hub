#!/usr/bin/env python3
"""Smoke-test the Studio Hub MCP gateway the way Claude Code / Codex use it: start `studio-hub mcp`,
speak newline-delimited JSON-RPC on its stdin/stdout, and check the answers.

    python scripts/smoke-mcp.py <path-to-studio-hub[.exe]> [--launch <app>]

Fails (exit 1) if the handshake, the tool list, suite_status (all seven apps installed) or PdfCraft's
headless tool list do not work. --launch also opens a windowed app and lists its live tools.
"""
import json, subprocess, sys, time

exe = sys.argv[1]
launch = sys.argv[sys.argv.index("--launch") + 1] if "--launch" in sys.argv else None
p = subprocess.Popen([exe, "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding="utf-8")
next_id = 0

def rpc(method, params=None):
    global next_id
    next_id += 1
    msg = {"jsonrpc": "2.0", "id": next_id, "method": method}
    if params is not None:
        msg["params"] = params
    p.stdin.write(json.dumps(msg) + "\n"); p.stdin.flush()
    while True:
        line = p.stdout.readline()
        if not line:
            sys.exit(f"FAIL: gateway closed its output during {method}")
        r = json.loads(line)
        if r.get("id") == next_id:
            if "error" in r:
                sys.exit(f"FAIL: {method}: {r['error']}")
            return r["result"]

def call(name, args):
    r = rpc("tools/call", {"name": name, "arguments": args})
    text = "".join(c.get("text", "") for c in r.get("content", []) if c.get("type") == "text")
    if r.get("isError"):
        raise RuntimeError(f"{name}: {text[:400]}")
    try:
        return json.loads(text)
    except ValueError:
        return text

init = rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "smoke", "version": "1"}})
print("server:", init["serverInfo"])
p.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n"); p.stdin.flush()
tools = [t["name"] for t in rpc("tools/list")["tools"]]
print("tools:", tools)
assert {"suite_status", "suite_launch_app", "suite_list_tools", "suite_describe_tool", "suite_call_tool"} <= set(tools), tools

status = call("suite_status", {})
apps = status["apps"] if isinstance(status, dict) else []
for a in apps:
    print(f"  {a['app']:<12} installed={a['installed']} open={a['window_open']} agent_ready={a['agent_ready']}")
missing = [a["app"] for a in apps if not a["installed"]]
assert len(apps) == 7 and not missing, f"not installed: {missing or status}"

pdf = call("suite_list_tools", {"app": "pdfcraft"})
print(f"pdfcraft (headless): {pdf['total']} tools")
assert pdf["total"] > 0

if launch:
    t0 = time.time()
    try:
        print(f"{launch}:", call("suite_launch_app", {"app": launch}), f"({time.time() - t0:.0f}s)")
        live = call("suite_list_tools", {"app": launch})
        print(f"{launch} (live window): {live['total']} tools")
    except RuntimeError as e:
        print(f"NOTE: launching {launch} did not work here: {e}")

p.stdin.close(); p.wait(timeout=30)
print("OK")
