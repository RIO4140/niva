# Python LSP driver: talks to niva-lsp over stdio (Content-Length
# framing) and checks diagnostics, hover, symbols, completion.
# Usage: python3 test-lsp.py
import json
import subprocess
import sys

BIN = "target/release/niva-lsp"
URI = "file:///tmp/test.نيفا"

proc = subprocess.Popen([BIN], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
_next_id = [1]
failures = []


def send(obj):
    body = json.dumps(obj).encode("utf-8")
    proc.stdin.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
    proc.stdin.flush()


def read_msg():
    headers = {}
    while True:
        line = proc.stdout.readline().decode("utf-8").strip()
        if line == "":
            break
        k, v = line.split(":", 1)
        headers[k.strip()] = v.strip()
    n = int(headers["Content-Length"])
    return json.loads(proc.stdout.read(n).decode("utf-8"))


def request(method, params):
    rid = _next_id[0]
    _next_id[0] += 1
    send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params})
    # Skip notifications until the matching response arrives.
    notifs = []
    while True:
        msg = read_msg()
        if "id" in msg and msg["id"] == rid:
            return msg, notifs
        notifs.append(msg)


def notify(method, params):
    send({"jsonrpc": "2.0", "method": method, "params": params})


def check(label, cond, extra=""):
    print(("PASS " if cond else "FAIL ") + label + ("" if cond else " :: " + str(extra)))
    if not cond:
        failures.append(label)


# --- initialize ---
resp, _ = request("initialize", {
    "processId": None, "rootUri": None,
    "capabilities": {},
})
check("initialize ok", "result" in resp, resp)
caps = resp.get("result", {}).get("capabilities", {})
check("hover capability", caps.get("hoverProvider") is True, caps)
check("symbol capability", caps.get("documentSymbolProvider") is True, caps)
check("completion capability", "completionProvider" in caps, caps)
notify("initialized", {})

GOOD = """# برنامج سليم
دالة جمع(أ، ب):
    ارجع أ + ب
فئة نقطة:
    دالة أنشئ(س، ص):
        نفس.س = س
        نفس.ص = ص
اطبع جمع(٣، ٤)
"""

BAD_SEMANTIC = """اطبع س
"""
BAD_PARSE = """دالة جمع(أ، ب)
    ارجع أ + ب
"""
BAD_LEX = """اطبع "نص بلا إغلاق
"""

notify("textDocument/didOpen", {
    "textDocument": {"uri": URI, "languageId": "niva", "version": 1, "text": GOOD},
})
msg = read_msg()
check("good file: publishDiagnostics", msg.get("method") == "textDocument/publishDiagnostics", msg)
check("good file: 0 diagnostics", msg.get("params", {}).get("diagnostics") == [], msg)

# Hover on اطبع (line 7, char 1 — the GOOD file's last line
# is `اطبع جمع(٣، ٤)`; line 4 is `دالة أنشئ`, not اطبع).
resp, _ = request("textDocument/hover", {
    "textDocument": {"uri": URI},
    "position": {"line": 7, "character": 1},
})
check("hover اطبع", resp.get("result") and "يطبع" in resp["result"]["contents"]["value"], resp)
resp, _ = request("textDocument/hover", {
    "textDocument": {"uri": URI},
    "position": {"line": 1, "character": 7},
})
check("hover جمع (no doc, plain ident)", resp.get("result") is None, resp)

# Symbols: جمع (function) + نقطة (class).
resp, _ = request("textDocument/documentSymbol", {"textDocument": {"uri": URI}})
syms = resp.get("result", [])
names = [(s["name"], s["kind"]) for s in syms]
check("symbols: جمع + نقطة", ("جمع", 12) in names and ("نقطة", 5) in names, names)
check("symbol جمع line", any(s["name"] == "جمع" and s["selectionRange"]["start"]["line"] == 1 for s in syms), syms)

# Completion (plain): keywords + builtins present.
resp, _ = request("textDocument/completion", {
    "textDocument": {"uri": URI},
    "position": {"line": 4, "character": 0},
})
labels = [i["label"] for i in resp.get("result", [])]
check("completion has اطبع + دالة + طول", all(x in labels for x in ["اطبع", "دالة", "طول"]), labels[:10])
check("completion count >= 50", len(labels) >= 50, len(labels))

# Completion after a dot: methods present.
resp, _ = request("textDocument/completion", {
    "textDocument": {"uri": URI},
    "position": {"line": 4, "character": 0},
    "context": {"triggerKind": 2, "triggerCharacter": "."},
})
labels = [i["label"] for i in resp.get("result", [])]
check("dot completion has كبير + أضف", all(x in labels for x in ["كبير", "أضف"]), labels)

# --- diagnostics: change to each bad file ---
for label, src, want in [
    ("semantic error", BAD_SEMANTIC, "متغير غير معرف: س"),
    ("parse error", BAD_PARSE, None),
    ("lex error", BAD_LEX, None),
]:
    notify("textDocument/didChange", {
        "textDocument": {"uri": URI, "version": 2},
        "contentChanges": [{"text": src}],
    })
    msg = read_msg()
    diags = msg.get("params", {}).get("diagnostics", [])
    check(label + ": exactly 1 diagnostic", len(diags) == 1, diags)
    if want:
        check(label + ": message", want in diags[0]["message"], diags[0]["message"])
    check(label + ": range line 0", diags and diags[0]["range"]["start"]["line"] == 0, diags)
    check(label + ": severity error", diags and diags[0]["severity"] == 1, diags)

# Back to good: diagnostics clear.
notify("textDocument/didChange", {
    "textDocument": {"uri": URI, "version": 3},
    "contentChanges": [{"text": GOOD}],
})
msg = read_msg()
check("fixed file: 0 diagnostics", msg.get("params", {}).get("diagnostics") == [], msg)

# Unknown method -> MethodNotFound error.
resp, _ = request("textDocument/formatting", {"textDocument": {"uri": URI}})
check("unknown method -> error", "error" in resp and resp["error"]["code"] == -32601, resp)

# Shutdown + exit.
resp, _ = request("shutdown", None)
check("shutdown ok", resp.get("result") is None and "error" not in resp, resp)
notify("exit", None)
proc.wait(timeout=5)
check("exit code 0", proc.returncode == 0, proc.returncode)

print()
print("FAILURES:", failures if failures else "none")
sys.exit(1 if failures else 0)
