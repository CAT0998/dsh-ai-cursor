"""Verify the image-sprite pipeline against the real overlay process.

Checks, in order:
  1. a GIF loads and reports its frame count
  2. a PNG with a real alpha ramp renders with per-pixel alpha (not keyed out)
  3. an SVG rasterises
  4. an animated sprite actually advances frames over time
  5. clearing a slot restores the built-in vector artwork

    python scripts/selftest-sprites.py bin/ai-mouse-overlay.exe artifacts/sprites
"""

import json
import subprocess
import sys
import time
from pathlib import Path

EXE = Path(sys.argv[1] if len(sys.argv) > 1 else "bin/ai-mouse-overlay.exe").resolve()
ASSETS = Path(sys.argv[2] if len(sys.argv) > 2 else "artifacts/sprites").resolve()
OUT = ASSETS.parent / "sprite-frames"
OUT.mkdir(parents=True, exist_ok=True)


class Overlay:
    def __init__(self, exe: Path):
        self.p = subprocess.Popen(
            [str(exe)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.PIPE, text=True, encoding="utf-8", bufsize=1,
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
        raise RuntimeError("no reply for " + cmd)

    def close(self):
        try:
            self.p.stdin.write('{"cmd":"quit"}\n')
            self.p.stdin.flush()
            self.p.wait(timeout=3)
        except Exception:
            self.p.kill()


def to_png(bmp: Path, png: Path):
    from PIL import Image
    Image.open(bmp).convert("RGB").save(png)


def alpha_pixels(png: Path):
    """Count distinct grey levels the fade PNG produced: a key-colour pipeline
    collapses this to a hard edge, real alpha keeps the ramp."""
    from PIL import Image
    im = Image.open(png).convert("RGB")
    greys = {im.getpixel((x, y)) for x in range(im.width) for y in range(im.height)}
    return len(greys)


def main():
    results = []

    def check(name, ok, detail=""):
        results.append((name, bool(ok)))
        print(("[PASS] " if ok else "[FAIL] ") + name + (" — " + detail if detail else ""))

    ov = Overlay(EXE)

    # A cursor at a fixed spot, capture-visible so the dump matches the window.
    ov.req("affinity", capture_visible=True)
    time.sleep(0.4)
    ov.req("panel", visible=False)
    ov.req("move", x=600, y=400, duration_ms=0)
    time.sleep(0.2)

    # ---- 1. animated GIF loads -------------------------------------------
    r = ov.req("sprite", slot="cursor", source=str(ASSETS / "spin.gif"), size=64,
               offset=[0, 0], hotspot=[0, 0])
    st = ov.req("status")
    cur = st["slots"]["cursor"]
    check("GIF loaded", r.get("ok") and "error" not in cur, json.dumps(cur, ensure_ascii=False))
    check("GIF frame count > 1", cur.get("frames", 0) >= 2, f"frames={cur.get('frames')}")

    # ---- 2. animation advances -------------------------------------------
    dumps = []
    for i in range(4):
        p = OUT / f"anim-{i}.bmp"
        ov.req("dump", path=str(p), target="cursor")
        to_png(p, OUT / f"anim-{i}.png")
        dumps.append(p.read_bytes())
        time.sleep(0.09)
    distinct = len({hash(d) for d in dumps})
    check("animation advances", distinct >= 2, f"{distinct} distinct frames of {len(dumps)}")

    # ---- 3. PNG alpha ramp ------------------------------------------------
    ov.req("sprite", slot="cursor", source=str(ASSETS / "fade.png"), size=80)
    time.sleep(0.2)
    p = OUT / "fade.bmp"
    ov.req("dump", path=str(p), target="cursor")
    to_png(p, OUT / "fade-out.png")
    levels = alpha_pixels(OUT / "fade-out.png")
    check("PNG alpha ramp survives", levels > 20, f"{levels} distinct colours across the ramp")

    # ---- 4. SVG ------------------------------------------------------------
    r = ov.req("sprite", slot="cursor", source=str(ASSETS / "star.svg"), size=64)
    st = ov.req("status")
    star = st["slots"]["cursor"]
    check("SVG rasterised", r.get("ok") and "error" not in star and star.get("natural"),
          json.dumps(star, ensure_ascii=False))
    p = OUT / "star.bmp"
    ov.req("dump", path=str(p), target="cursor")
    to_png(p, OUT / "star-out.png")

    # ---- 5. ornament slot --------------------------------------------------
    r = ov.req("sprite", slot="ornament", source=str(ASSETS / "star.svg"), size=40,
               offset=[-70, 0], hotspot=[0.5, 0.5], opacity=0.8)
    st = ov.req("status")
    check("ornament slot active", r.get("ok") and st["slots"]["ornament"].get("natural") is not None,
          json.dumps(st["slots"]["ornament"], ensure_ascii=False))
    p = OUT / "ornament.bmp"
    ov.req("dump", path=str(p), target="cursor")
    to_png(p, OUT / "ornament-out.png")

    # ---- 6. clearing restores the built-in art -----------------------------
    ov.req("sprite", slot="cursor", source="builtin")
    ov.req("sprite", slot="ornament", source="")
    st = ov.req("status")
    check("cursor back to builtin", st["slots"]["cursor"]["source"] == "builtin")
    check("ornament cleared", st["slots"]["ornament"]["source"] == "")

    # ---- 7. a bad path reports an error instead of crashing ----------------
    ov.req("sprite", slot="cursor", source=str(ASSETS / "does-not-exist.png"))
    st = ov.req("status")
    check("missing file reports an error", "error" in st["slots"]["cursor"],
          st["slots"]["cursor"].get("error", ""))

    ov.close()

    failed = [n for n, ok in results if not ok]
    print(f"\n{len(results) - len(failed)}/{len(results)} passed")
    for n in failed:
        print("  FAILED:", n)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
