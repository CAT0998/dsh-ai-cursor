"""Prove the minimap's blue dot tracks the real pointer.

The thumbnail is cached, so the dot is the part that must move every frame even
when the desktop does not. This moves the real pointer to known screen positions,
grabs the minimap window and locates the accent-coloured dot, comparing it with
the position `MapGeom` says it should land on.

    python scripts/verify-map-dot.py <absolute path to ai-mouse-overlay.exe>

Pass an absolute path: CreateProcess does not resolve relative ones from a
non-ASCII working directory.
"""
import ctypes, json, os, subprocess, sys, time
from PIL import ImageGrab

exe = sys.argv[1] if len(sys.argv) > 1 else "bin/ai-mouse-overlay.exe"
exe = os.path.abspath(exe)
k32 = ctypes.windll.user32
p = subprocess.Popen([exe, "--map"], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                     stderr=subprocess.DEVNULL, text=True, encoding="utf-8", bufsize=1)
ready = json.loads(p.stdout.readline())
assert ready.get("event") == "ready", ready
time.sleep(2.5)

WX, WY, WW, WH = 64, 64, 380, 260
ACCENT = (0x4F, 0xC3, 0xF7)          # default accent 0x4FC3F7
# MapGeom for zoom 1.0 on a 1920x1080 desktop: k = min(364/1920, 244/1080), centred
k = min((380 - 16) / 1920, (260 - 16) / 1080)
ox = 8 + int(((380 - 16) - 1920 * k) / 2)
oy = 8 + int(((260 - 16) - 1080 * k) / 2)

def dot_of(img):
    px = img.load()
    pts = [(x, y) for y in range(img.height) for x in range(img.width) if px[x, y] == ACCENT]
    if not pts:
        return None
    return (round(sum(x for x, _ in pts) / len(pts)), round(sum(y for _, y in pts) / len(pts))), len(pts)

ok = True
for (cx, cy) in [(300, 200), (1600, 900), (960, 540)]:
    k32.SetCursorPos(cx, cy)
    time.sleep(0.35)
    img = ImageGrab.grab(bbox=(WX, WY, WX + WW, WY + WH), all_screens=True).convert("RGB")
    img.save(f"artifacts/dot-{cx}-{cy}.png")
    got = dot_of(img)
    want = (ox + int(cx * k), oy + int(cy * k))
    d = None if not got else max(abs(got[0][0] - want[0]), abs(got[0][1] - want[1]))
    verdict = "PASS" if (got and d <= 3) else "FAIL"
    ok &= verdict == "PASS"
    print(f"{verdict} cursor=({cx},{cy}) dot={got[0] if got else None} px={got[1] if got else 0} expected={want} max_delta={d}")
p.terminate()
print("ALL PASS" if ok else "SOME FAILED")
