"""Stage the awesome-dsh-plugin submission on our own fork.

Creates the fork, a branch, and data/plugins/CAT0998__dsh-ai-cursor.yml. The pull
request itself is a separate, deliberate step: the upstream CI rejects entries
whose repository is younger than one day, so opening it immediately would only
add a knowingly-red PR to their queue.

    python scripts/stage-marketplace-entry.py            # prepare fork + branch
    python scripts/stage-marketplace-entry.py --pr       # also open the PR
"""

import base64
import json
import sys
import time
import urllib.error
import urllib.request

UPSTREAM = "awesome-dsh-plugin/awesome-dsh-plugin"
FORK_FROM = "CAT0998"
FORK = f"{FORK_FROM}/awesome-dsh-plugin"
BRANCH = "add-dsh-ai-cursor"
PATH = "data/plugins/CAT0998__dsh-ai-cursor.yml"
API = "https://api.github.com"

ENTRY = """url: https://github.com/CAT0998/dsh-ai-cursor
name: CAT0998/dsh-ai-cursor
category: tools
description:
  en: A second, agent-owned cursor with a floating control panel for opacity, colour, size, badge and a permission ladder only a human can raise. It never moves the real pointer.
  zh: 给 AI 的第二只光标加一个浮动控制面板，可调透明度、颜色、大小与提示显隐，并按只能由人提高的权限等级限制它；从不移动你的真实鼠标指针。
tarball: https://github.com/CAT0998/dsh-ai-cursor/releases/latest/download/dsh-ai-cursor.tgz
"""

PR_BODY = """## What this adds

`CAT0998/dsh-ai-cursor` — an AI-owned second cursor for DeepSeek Harness on Windows,
plus a floating, collapsible control panel.

- **Cursor overlay**: click-through, tool-window, topmost, excluded from screen capture
  by default; three artworks (whale / dot / arrow). It never moves the real pointer and
  never takes keyboard focus.
- **Floating panel** (`452x396` expanded, `132x40` chip when collapsed, draggable header):
  显示 / 隐藏 / 停止 / 启用, badge on/off, opacity 15-100%, 8 colour swatches + hex,
  cursor style, size 60-200%, and the permission ladder.
- **Permission ladder 0-3** (停止 / 只读 / 操作 / 管理员): the agent may lower its own
  ceiling but never raise it — only a human clicking in the panel can hand control back.
- 14 `ai_mouse_*` tools; a JSON line protocol; Rust/Win32 overlay process + TypeScript
  Cordis plugin shell.

Verified on Windows: `cargo build --release` with 0 warnings, `tsc --noEmit` clean,
11/11 vitest, 35/35 overlay protocol self-test, and every panel control clicked with a
real mouse on the live desktop (collapse, expand, colour, opacity, style, size, badge,
stop, admin, header drag).

Installable: `dsh.bundle` manifest present; a prebuilt tarball is attached to
[v0.2.0](https://github.com/CAT0998/dsh-ai-cursor/releases/tag/v0.2.0) so users need no
Rust toolchain. Windows 10 2004+ only; macOS/Linux are not implemented and the README
says so.
"""


def token():
    import subprocess

    out = subprocess.run(["gh", "auth", "token"], capture_output=True, text=True)
    if out.returncode != 0 or not out.stdout.strip():
        sys.exit("no GitHub token from `gh auth token`")
    return out.stdout.strip()


TOKEN = token()


def api(method, path, payload=None, ok=(200, 201, 204)):
    data = json.dumps(payload).encode() if payload is not None else None
    req = urllib.request.Request(
        f"{API}{path}",
        data=data,
        method=method,
        headers={
            "Authorization": f"Bearer {TOKEN}",
            "Accept": "application/vnd.github+json",
            "X-GitHub-Api-Version": "2022-11-28",
            "Content-Type": "application/json",
            "User-Agent": "dsh-ai-cursor-market",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=90) as resp:
            body = resp.read().decode()
            return resp.status, (json.loads(body) if body.strip() else {})
    except urllib.error.HTTPError as e:
        body = e.read().decode()
        return e.code, {"error": body[:600]}


def main():
    want_pr = "--pr" in sys.argv

    status, _ = api("GET", f"/repos/{FORK}")
    if status == 404:
        print("creating fork…")
        print(api("POST", f"/repos/{UPSTREAM}/forks", {"default_branch_only": True})[0])
        for _ in range(30):
            time.sleep(3)
            if api("GET", f"/repos/{FORK}")[0] == 200:
                break
        else:
            sys.exit("fork did not become available in time")
    print("fork ready:", FORK)

    status, repo = api("GET", f"/repos/{FORK}")
    default = repo.get("default_branch", "main")
    status, ref = api("GET", f"/repos/{FORK}/git/ref/heads/{default}")
    head = ref["object"]["sha"]
    print(f"fork head ({default}):", head)

    status, _ = api("GET", f"/repos/{FORK}/git/ref/heads/{BRANCH}")
    if status == 200:
        print(f"branch {BRANCH} already exists")
    else:
        status, out = api(
            "POST", f"/repos/{FORK}/git/refs", {"ref": f"refs/heads/{BRANCH}", "sha": head}
        )
        print("branch:", status)
        if status not in (200, 201):
            sys.exit(out)

    status, out = api(
        "PUT",
        f"/repos/{FORK}/contents/{PATH}",
        {
            "message": "Add CAT0998/dsh-ai-cursor",
            "content": base64.b64encode(ENTRY.encode()).decode(),
            "branch": BRANCH,
        },
    )
    print("entry file:", status)
    if status not in (200, 201):
        sys.exit(out)
    print("entry committed:", out.get("commit", {}).get("html_url"))

    if not want_pr:
        print("\nnot opening the PR: upstream CI requires the repository to be at least")
        print("1 day old. Run this script with --pr after that.")
        return

    status, pr = api(
        "POST",
        f"/repos/{UPSTREAM}/pulls",
        {
            "title": "Add CAT0998/dsh-ai-cursor",
            "head": f"{FORK_FROM}:{BRANCH}",
            "base": "main",
            "body": PR_BODY,
            "maintainer_can_modify": True,
        },
    )
    print("PR:", status)
    print(json.dumps(pr, ensure_ascii=False)[:600])


if __name__ == "__main__":
    main()
