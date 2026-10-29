"""Keep one overlay alive and mirror its state to disk so a human/agent can click it.

The mouse-driven tests need the panel to stay on screen between tool calls, so
this runs as a background job: it spawns the overlay, switches capture on (so the
panel appears in screenshots), shows the panel, then polls `status` and writes

    artifacts/live-status.json   full status + real window rectangles
    artifacts/live-panel.bmp     refreshed on demand

Stop it by creating artifacts/live-stop.
"""

import ctypes
import ctypes.wintypes as wt
import json
import subprocess
import sys
import time
from pathlib import Path

EXE = Path(sys.argv[1] if len(sys.argv) > 1 else "bin/ai-mouse-overlay.exe").resolve()
OUT = Path(sys.argv[2] if len(sys.argv) > 2 else "artifacts").resolve()
OUT.mkdir(parents=True, exist_ok=True)
STATUS = OUT / "live-status.json"
STOP = OUT / "live-stop"
DUMP = OUT / "live-dump-request"

user32 = ctypes.windll.user32


def window_rect(hwnd_hex: str):
    try:
        hwnd = int(hwnd_hex, 16)
    except (TypeError, ValueError):
        return None
    r = wt.RECT()
    if not user32.GetWindowRect(hwnd, ctypes.byref(r)):
        return None
    if not user32.IsWindowVisible(hwnd):
        return None
    return {"x": r.left, "y": r.top, "w": r.right - r.left, "h": r.bottom - r.top}


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
        self.ready = json.loads(self.p.stdout.readline())
        self.seq = 0

    def req(self, cmd, **kw):
        self.seq += 1
        msg = {"id": str(self.seq), "cmd": cmd, **kw}
        self.p.stdin.write(json.dumps(msg) + "\n")
        self.p.stdin.flush()
        for _ in range(400):
            line = self.p.stdout.readline()
            if not line:
                raise RuntimeError("overlay died")
            try:
                v = json.loads(line)
            except json.JSONDecodeError:
                continue
            if v.get("id") == msg["id"]:
                return v
        return {"ok": False, "error": "timeout"}

    def close(self):
        try:
            self.p.stdin.write('{"cmd":"quit"}\n')
            self.p.stdin.flush()
        except Exception:
            pass
        try:
            self.p.wait(timeout=2)
        except subprocess.TimeoutExpired:
            self.p.kill()


def main():
    if STOP.exists():
        STOP.unlink()
    ov = Overlay(EXE)
    print("ready", json.dumps(ov.ready, ensure_ascii=False), flush=True)

    # Show up in screenshots, then put the panel on screen with a cursor to move.
    ov.req("affinity", capture_visible=True)
    time.sleep(0.6)
    ov.req("move", x=700, y=520, duration_ms=0)
    ov.req("panel_expanded", expanded=True)
    ov.req("panel", visible=True)
    time.sleep(0.6)

    last = None
    while not STOP.exists():
        st = ov.req("status")
        st["overlay_rect"] = window_rect(st.get("hwnd"))
        st["panel_rect"] = window_rect(st.get("panel_hwnd"))
        blob = json.dumps(st, ensure_ascii=False, sort_keys=True)
        if blob != last:
            STATUS.write_text(blob, encoding="utf-8")
            print("update", blob[:200], flush=True)
            last = blob
        if DUMP.exists():
            tag = DUMP.read_text(encoding="utf-8").strip() or "live"
            DUMP.unlink()
            for target in ("panel", "cursor"):
                p = OUT / f"{target}-{tag}.bmp"
                ov.req("dump", path=str(p), target=target)
            print("dumped", tag, flush=True)
        time.sleep(0.25)

    ov.close()
    print("closed", flush=True)


if __name__ == "__main__":
    main()
