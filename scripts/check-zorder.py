"""Report where the AI cursor sits relative to an anchor in the window z-order."""
import ctypes, ctypes.wintypes as wt, json, sys

u = ctypes.windll.user32
OVERLAY, ANCHOR = "AiMouseOverlay", int(sys.argv[1], 16)
order = []
handles = {}

@ctypes.WINFUNCTYPE(ctypes.c_bool, wt.HWND, wt.LPARAM)
def cb(h, lp):
    b = ctypes.create_unicode_buffer(64); u.GetClassNameW(h, b, 64)
    if u.IsWindowVisible(h):
        order.append(h)
        if b.value == OVERLAY:
            handles["overlay"] = h
    return True

u.EnumWindows(cb, 0)
handles["anchor"] = ANCHOR
idx_o = order.index(handles["overlay"]) if handles.get("overlay") in order else -1
idx_a = order.index(ANCHOR) if ANCHOR in order else -1
r = wt.RECT(); u.GetWindowRect(ANCHOR, ctypes.byref(r))
fg = u.GetForegroundWindow()
print(json.dumps({
    "overlay_index": idx_o, "anchor_index": idx_a,
    "relation": "ABOVE anchor" if 0 <= idx_o < idx_a else ("BELOW anchor" if idx_o > idx_a >= 0 else "unknown"),
    "anchor_rect": [r.left, r.top, r.right - r.left, r.bottom - r.top],
    "anchor_is_foreground": fg == ANCHOR,
}, ensure_ascii=False))
