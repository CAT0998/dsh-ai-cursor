"""Drive ai-mouse-overlay over its line protocol and render frames to PNG.

Used by the self-test suite: it talks to the real Win32 process, so a passing
run means the window really exists and really painted something.

    python scripts/selftest-overlay.py bin/ai-mouse-overlay.exe artifacts
"""

import json
import subprocess
import sys
import time
from pathlib import Path

EXE = Path(sys.argv[1] if len(sys.argv) > 1 else "bin/ai-mouse-overlay.exe").resolve()
OUT = Path(sys.argv[2] if len(sys.argv) > 2 else "artifacts").resolve()
OUT.mkdir(parents=True, exist_ok=True)


class Overlay:
    def __init__(self, exe: Path):
        self.p = subprocess.Popen(
            [str(exe)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            bufsize=1,
        )
        ready = json.loads(self.p.stdout.readline())
        assert ready.get("event") == "ready", ready
        self.ready = ready
        self.seq = 0

    def req(self, cmd, **kw):
        self.seq += 1
        msg = {"id": str(self.seq), "cmd": cmd, **kw}
        self.p.stdin.write(json.dumps(msg) + "\n")
        self.p.stdin.flush()
        for _ in range(400):
            line = self.p.stdout.readline()
            if not line:
                raise RuntimeError("overlay died: " + self.p.stderr.read())
            try:
                v = json.loads(line)
            except json.JSONDecodeError:
                continue
            if v.get("id") == msg["id"]:
                return v
        raise RuntimeError("no reply for " + cmd)

    def close(self):
        try:
            self.p.stdin.write(json.dumps({"cmd": "quit"}) + "\n")
            self.p.stdin.flush()
        except Exception:
            pass
        try:
            self.p.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.p.kill()


def bmp_to_png(bmp: Path, png: Path):
    from PIL import Image

    im = Image.open(bmp).convert("RGB")
    im.save(png)
    return im.size


def main():
    ov = Overlay(EXE)
    print("ready:", json.dumps(ov.ready, ensure_ascii=False))
    results = []

    def check(name, got, ok):
        results.append((name, bool(ok), got))
        print(("[PASS] " if ok else "[FAIL] ") + name + " -> " + json.dumps(got, ensure_ascii=False)[:220])
        return got

    def step(name, cmd, ok=lambda r: r.get("ok"), expect=None, **kw):
        r = ov.req(cmd, **kw)
        okv = ok(r)
        if expect is not None:
            okv = okv and expect(r)
        return check(name, r, okv)

    def dump(tag, target="panel"):
        p = OUT / f"{target}-{tag}.bmp"
        r = ov.req("dump", path=str(p), target=target)
        assert r["ok"] and not r.get("dump_error"), r
        bmp_to_png(p, OUT / f"{target}-{tag}.png")
        return r

    # ---- basics -------------------------------------------------------------
    step("status#0", "status")
    step("show", "show", expect=lambda r: r["visible"] is True)
    step("move", "move", x=900, y=500, duration_ms=0,
         expect=lambda r: r["x"] == 900 and r["y"] == 500)
    step("panel on", "panel", visible=True, expect=lambda r: r["panel"] is True)
    time.sleep(0.3)
    step("dump panel/default", "status")
    dump("default")

    # ---- customisation ------------------------------------------------------
    step("color #7C5CFF", "color", hex="#7C5CFF", expect=lambda r: r["color"] == "#7C5CFF")
    step("opacity 0.55", "opacity", value=0.55, expect=lambda r: abs(r["opacity"] - 0.55) < 1e-6)
    step("scale 1.5", "scale", value=1.5, expect=lambda r: abs(r["scale"] - 1.5) < 1e-6)
    step("badge off", "badge", visible=False, expect=lambda r: r["badge"] is False)
    step("theme dot", "theme", theme="dot", expect=lambda r: r["theme"] == "dot")
    time.sleep(0.2)
    dump("custom")

    # ---- cursor artwork at full opacity -------------------------------------
    step("opacity 1.0", "opacity", value=1.0)
    step("badge on", "badge", visible=True)
    step("color cyan", "color", hex="#4FC3F7")
    step("scale 1.0", "scale", value=1.0)
    for theme in ("whale", "dot", "arrow"):
        step(f"theme {theme}", "theme", theme=theme, expect=lambda r, t=theme: r["theme"] == t)
        time.sleep(0.15)
        dump(f"cursor/{theme}".replace("cursor/", ""), target="cursor")

    # ---- collapse / expand --------------------------------------------------
    step("collapse", "panel_expanded", expanded=False, expect=lambda r: r["panel_expanded"] is False)
    time.sleep(0.25)
    dump("chip")
    step("expand", "panel_expanded", expanded=True, expect=lambda r: r["panel_expanded"] is True)
    time.sleep(0.25)

    # ---- permission ladder: the agent may only lower -----------------------
    step("lower to 1", "permission", level=1, expect=lambda r: r["permission"] == 1)
    step("move denied at L1", "move", ok=lambda r: r["ok"] is False, x=10, y=10)
    step("show allowed at L1", "show", expect=lambda r: r["visible"] is True)
    step("raise denied", "permission", level=3, ok=lambda r: r["ok"] is False)
    step("customise denied at L1", "color", ok=lambda r: r["ok"] is False, hex="#FFFFFF")

    # ---- capture affinity ---------------------------------------------------
    step("capture visible", "affinity", capture_visible=True)
    time.sleep(0.6)
    step("affinity readback on", "status", expect=lambda r: r["capture_visible"] is True)
    step("capture hidden", "affinity", capture_visible=False)
    time.sleep(0.6)
    step("affinity readback off", "status", expect=lambda r: r["capture_visible"] is False)

    step("panel off", "panel", visible=False, expect=lambda r: r["panel"] is False)

    ov.close()

    # ---- stop is a one-way door for the agent -------------------------------
    # A stopped overlay must stay stopped until a human clicks 启用 in the panel:
    # the agent can shut itself down but must never restart itself. Needs an
    # overlay that still starts at the default level 3, hence a second process.
    ov = Overlay(EXE)
    print("ready #2:", json.dumps(ov.ready, ensure_ascii=False))
    step("pause", "pause", expect=lambda r: r["permission"] == 0 and r["paused"] is True)
    step("show denied while stopped", "show", ok=lambda r: r["ok"] is False)
    step("move denied while stopped", "move", ok=lambda r: r["ok"] is False, x=10, y=10)
    step("resume denied while stopped", "resume", ok=lambda r: r["ok"] is False)
    step("panel denied while stopped", "panel", ok=lambda r: r["ok"] is False, visible=True)
    step("status allowed while stopped", "status", expect=lambda r: r["permission"] == 0)

    ov.close()

    failed = [r for r in results if not r[1]]
    print(f"\n{len(results) - len(failed)}/{len(results)} passed")
    for name, _, got in failed:
        print("  FAILED:", name, json.dumps(got, ensure_ascii=False)[:220])
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
