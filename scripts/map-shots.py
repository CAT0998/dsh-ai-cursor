"""Screenshot the minimap window in several states (GPU path, GDI fallback, zooms).

The minimap is a separate process (`--map`) whose only inputs are its stdin line
protocol and the window position, so this drives the real process and grabs the
real screen - no simulation of the drawing itself.

    python scripts/map-shots.py bin/ai-mouse-overlay.exe artifacts

Writes <out>/map-<state>.png, cropped to the window and upscaled 2x so the
thumbnail detail is visible.
"""

import json
import os
import subprocess
import sys
import time
from pathlib import Path

from PIL import Image, ImageGrab

EXE = Path(sys.argv[1] if len(sys.argv) > 1 else "bin/ai-mouse-overlay.exe").resolve()
OUT = Path(sys.argv[2] if len(sys.argv) > 2 else "artifacts").resolve()
OUT.mkdir(parents=True, exist_ok=True)

WIN_X, WIN_Y = 64, 64      # MAP_X / MAP_Y in overlay/src/main.rs
BASE_W, BASE_H = 380, 260  # MAP_W / MAP_H


def grab(zoom: float, name: str, pad: bool = False) -> Path:
    w, h = int(BASE_W * zoom), int(BASE_H * zoom)
    x0, y0 = WIN_X, WIN_Y
    # the map window is resized by UpdateLayeredWindow, so follow the zoom
    img = ImageGrab.grab(bbox=(x0, y0, x0 + w, y0 + h), all_screens=True).convert("RGB")
    img = img.resize((img.width * 2, img.height * 2), Image.NEAREST)
    path = OUT / f"map-{name}.png"
    img.save(path)
    # a real desktop thumbnail has thousands of colours; a blank panel has a handful
    colours = img.getcolors(maxcolors=1 << 22)
    print(f"{name}: {path} {img.size} colours={len(colours) if colours else '>4M'}")
    return path


class Map:
    def __init__(self, env=None):
        e = dict(os.environ)
        if env:
            e.update(env)
        self.p = subprocess.Popen(
            [str(EXE), "--map"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            bufsize=1,
            env=e,
        )
        ready = json.loads(self.p.stdout.readline())
        assert ready.get("event") == "ready", ready
        self.seq = 0

    def req(self, cmd, **kw):
        self.seq += 1
        msg = {"id": str(self.seq), "cmd": cmd, **kw}
        self.p.stdin.write(json.dumps(msg) + "\n")
        self.p.stdin.flush()
        self.p.stdout.readline()

    def close(self):
        try:
            self.p.stdin.write('{"cmd":"quit"}\n')
            self.p.stdin.flush()
        except Exception:
            pass
        time.sleep(0.2)
        self.p.terminate()
        try:
            self.p.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.p.kill()


def main() -> int:
    which = sys.argv[3] if len(sys.argv) > 3 else "all"

    if which in ("all", "gpu"):
        m = Map()
        time.sleep(2.5)
        grab(1.0, "gpu")
        # scale ratio = minimap size / desktop size: same capture, two sizes
        m.req("map_zoom", value=0.55)
        time.sleep(1.2)
        grab(0.55, "gpu-zoom05")
        m.req("map_zoom", value=2.0)
        time.sleep(1.2)
        grab(2.0, "gpu-zoom20")
        m.close()

    if which in ("all", "gdi"):
        m = Map({"AI_MOUSE_MINIMAP_GPU": "gdi"})
        time.sleep(2.5)
        grab(1.0, "gdi-fallback")
        m.close()

    if which in ("all", "warp"):
        m = Map({"AI_MOUSE_MINIMAP_GPU": "warp"})
        time.sleep(2.5)
        grab(1.0, "warp-fallback")
        m.close()

    return 0


if __name__ == "__main__":
    sys.exit(main())
