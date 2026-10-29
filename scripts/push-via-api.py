"""Push a git commit to GitHub over the REST Git Data API.

This machine can reach api.github.com but not github.com:443, so `git push` over
HTTPS times out. The API path builds the same commit server-side: one blob per
tracked file, one tree, one commit, then the branch ref.

A repository with no commits rejects the Git Data API with "Git Repository is
empty", so the first run creates one throwaway commit through the contents API
and then replaces its tree with the real one.

    python scripts/push-via-api.py
"""

import base64
import json
import subprocess
import sys
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

REPO = "CAT0998/dsh-ai-cursor"
API = "https://api.github.com"
ROOT = Path(__file__).resolve().parent.parent

BOOTSTRAP_README = b"# dsh-ai-cursor\n\nAI cursor for DeepSeek Harness.\n"


def token() -> str:
    out = subprocess.run(["gh", "auth", "token"], capture_output=True, text=True)
    if out.returncode != 0 or not out.stdout.strip():
        sys.exit("could not read a GitHub token from `gh auth token`")
    return out.stdout.strip()


TOKEN = token()


def api(method: str, path: str, payload=None):
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
            "User-Agent": "dsh-ai-cursor-push",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=90) as resp:
            return json.loads(resp.read().decode() or "{}")
    except urllib.error.HTTPError as e:
        body = e.read().decode()
        raise RuntimeError(f"{method} {path} -> {e.code}\n{body[:800]}") from None


# GitHub refuses to create or update workflow files unless the token carries the
# `workflow` scope, and it answers 404 rather than 403 so the restriction is hard
# to discover. Those paths are skipped and reported instead of failing the push;
# `gh auth refresh -s workflow` (interactive) unlocks them.
NEEDS_WORKFLOW_SCOPE = ".github/workflows/"


def tracked_files():
    out = subprocess.run(["git", "ls-files", "-z"], cwd=ROOT, capture_output=True).stdout
    files = [f.decode("utf-8") for f in out.split(b"\0") if f]
    skipped = [f for f in files if f.startswith(NEEDS_WORKFLOW_SCOPE)]
    if skipped:
        print("skipping (token needs the `workflow` scope):", ", ".join(skipped))
    return [f for f in files if not f.startswith(NEEDS_WORKFLOW_SCOPE)]


def blob_for(rel: str):
    raw = (ROOT / rel).read_bytes()
    sha = api(
        "POST",
        f"/repos/{REPO}/git/blobs",
        {"content": base64.b64encode(raw).decode(), "encoding": "base64"},
    )["sha"]
    return {"path": rel, "mode": "100644", "type": "blob", "sha": sha}


def head_sha():
    try:
        return api("GET", f"/repos/{REPO}/git/ref/heads/main")["object"]["sha"]
    except RuntimeError:
        return None


def main():
    files = tracked_files()
    if not files:
        sys.exit("no tracked files - run git add first")

    head = head_sha()
    if head is None:
        print("empty repository - creating a bootstrap commit")
        api(
            "PUT",
            f"/repos/{REPO}/contents/README.md",
            {"message": "Initial commit", "content": base64.b64encode(BOOTSTRAP_README).decode()},
        )
        head = head_sha()
        print("bootstrap commit:", head)
    else:
        print("existing head:", head)

    print(f"uploading {len(files)} blobs...")
    entries = []
    with ThreadPoolExecutor(max_workers=8) as pool:
        for i, entry in enumerate(pool.map(blob_for, files), 1):
            entries.append(entry)
            if i % 10 == 0:
                print(f"  {i}/{len(files)}")

    tree = api("POST", f"/repos/{REPO}/git/trees", {"tree": entries})
    print("tree:", tree["sha"])

    message = subprocess.check_output(
        ["git", "log", "-1", "--pretty=%B"], cwd=ROOT, text=True, encoding="utf-8"
    ).strip()
    commit = api(
        "POST",
        f"/repos/{REPO}/git/commits",
        {"message": message, "tree": tree["sha"], "parents": [head]},
    )
    print("commit:", commit["sha"])

    api("PATCH", f"/repos/{REPO}/git/refs/heads/main", {"sha": commit["sha"], "force": True})
    print("updated refs/heads/main")

    repo = api("GET", f"/repos/{REPO}")
    print("default branch:", repo.get("default_branch"), "| pushed:", repo.get("pushed_at"))
    print("url:", repo.get("html_url"))


if __name__ == "__main__":
    main()
