"""Inject a REAL mouse-wheel event over a live AiMousePanel window.

Used for the panel-scroll verification: the wheel is delivered through the OS
input path (SendInput), exactly like a hardware wheel, so a pass here means the
window's own WM_MOUSEWHEEL handler ran — not that some protocol shortcut worked.

    python scripts/verify-panel-scroll.py <pid> [notches]
"""

import ctypes
import ctypes.wintypes as wt
import sys
import time

u32 = ctypes.windll.user32

PANEL_CLASS = "AiMousePanel"
INPUT_MOUSE = 0
MOUSEEVENTF_WHEEL = 0x0800
MOUSEEVENTF_LEFTDOWN = 0x0002
MOUSEEVENTF_LEFTUP = 0x0004


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [
        ("dx", wt.LONG),
        ("dy", wt.LONG),
        ("mouseData", wt.DWORD),
        ("dwFlags", wt.DWORD),
        ("time", wt.DWORD),
        ("dwExtraInfo", ctypes.POINTER(wt.ULONG)),
    ]


class _INPUTUNION(ctypes.Union):
    _fields_ = [("mi", MOUSEINPUT), ("pad", ctypes.c_byte * 64)]


class INPUT(ctypes.Structure):
    _fields_ = [("type", wt.DWORD), ("u", _INPUTUNION)]


def panel_rect(pid: int):
    """The first visible AiMousePanel owned by pid, as (left, top, w, h)."""
    found = []

    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def cb(hwnd, _lparam):
        buf = ctypes.create_unicode_buffer(256)
        u32.GetClassNameW(hwnd, buf, 256)
        if buf.value != PANEL_CLASS:
            return True
        owner = wt.DWORD()
        u32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value != pid or not u32.IsWindowVisible(hwnd):
            return True
        r = wt.RECT()
        u32.GetWindowRect(hwnd, ctypes.byref(r))
        found.append((hwnd, r.left, r.top, r.right - r.left, r.bottom - r.top))
        return False

    u32.EnumWindows(cb, 0)
    return found[0] if found else None


def wheel(notches: int):
    """+1 = one notch up (content up), -1 = one notch down.

    `mouse_event` rather than a hand-rolled SendInput struct: the INPUT union size
    differs per architecture, and a wrong cbSize makes SendInput fail silently
    (return 0) — which looks exactly like "the window did not get the wheel".
    """
    data = (notches * 120) & 0xFFFFFFFF
    u32.mouse_event(MOUSEEVENTF_WHEEL, 0, 0, data, 0)


def click_at(screen_x: int, screen_y: int) -> None:
    """A real left click through the OS input path (down + up, same spot)."""
    u32.SetCursorPos(screen_x, screen_y)
    time.sleep(0.2)
    u32.mouse_event(MOUSEEVENTF_LEFTDOWN, 0, 0, 0, 0)
    time.sleep(0.06)
    u32.mouse_event(MOUSEEVENTF_LEFTUP, 0, 0, 0, 0)


def main() -> int:
    pid = int(sys.argv[1])
    cmd = sys.argv[2] if len(sys.argv) > 2 else "wheel"
    hit = panel_rect(pid)
    if not hit:
        print(f"no visible AiMousePanel for pid {pid}")
        return 1
    hwnd, left, top, w, h = hit
    if cmd == "click":
        # window-local coordinates, the same space a window screenshot uses
        wx, wy = int(sys.argv[3]), int(sys.argv[4])
        print(f"panel hwnd=0x{hwnd:X} rect=({left},{top},{w}x{h}) click window({wx},{wy})")
        click_at(left + wx, top + wy)
        time.sleep(0.4)
        return 0
    notches = int(cmd) if cmd.lstrip("-").isdigit() else -3
    cx, cy = left + w // 2, top + h // 3
    print(f"panel hwnd=0x{hwnd:X} rect=({left},{top},{w}x{h}) wheel at ({cx},{cy}) notches={notches}")
    u32.SetCursorPos(cx, cy)
    time.sleep(0.25)
    for _ in range(abs(notches)):
        wheel(1 if notches > 0 else -1)
        time.sleep(0.12)
    time.sleep(0.4)
    return 0


if __name__ == "__main__":
    sys.exit(main())
