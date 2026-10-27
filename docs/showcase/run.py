#!/usr/bin/env python3
"""run.py FILE.jsonl [PORT] — run control requests from a JSON-lines file ({"method":..., "params":...} or {"sleep": s}).

PORT is the app's control port (default 7980; agents use their own, 18000–19999). Authenticates with
$LIGHTCRAFT_CONTROL_TOKEN, else the token the app wrote to control-token-PORT in its
settings folder (docs/control-protocol.md → Authentication)."""
import json, os, socket, sys, time
from pathlib import Path
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 7980
def token():
    if os.environ.get("LIGHTCRAFT_CONTROL_TOKEN", "").strip():
        return os.environ["LIGHTCRAFT_CONTROL_TOKEN"].strip()
    if "LIGHTCRAFT_CONFIG_DIR" in os.environ:
        folder = Path(os.environ["LIGHTCRAFT_CONFIG_DIR"])
    elif sys.platform == "win32":
        folder = Path(os.environ.get("APPDATA", "")) / "LightCraft"
    elif sys.platform == "darwin":
        folder = Path.home() / "Library/Application Support/LightCraft"
    else:
        folder = Path(os.environ.get("XDG_CONFIG_HOME") or Path.home() / ".config") / "lightcraft"
    return (folder / f"control-token-{PORT}").read_text().strip()
def line(s, msg):
    s.sendall((json.dumps(msg) + "\n").encode())
    buf = b""
    while not buf.endswith(b"\n"):
        c = s.recv(1 << 20)
        if not c: break
        buf += c
    return json.loads(buf)
def call(method, params):
    s = socket.create_connection(("127.0.0.1", PORT), timeout=180)
    auth = line(s, {"id": 0, "method": "auth", "params": {"token": token()}})
    if not auth.get("ok"):
        return auth
    return line(s, {"id": 1, "method": method, "params": params})
for text in open(sys.argv[1]):
    text = text.strip()
    if not text or text.startswith("#"): continue
    r = json.loads(text)
    if "sleep" in r:
        time.sleep(r["sleep"]); continue
    out = call(r["method"], r.get("params", {}))
    print(r["method"], json.dumps(out)[:160])
