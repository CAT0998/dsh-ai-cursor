"""Measure the minimap process's CPU load with GetProcessTimes.

    python scripts/measure-map-cpu.py bin/ai-mouse-overlay.exe [--secs 8] [--move]

GetProcessTimes returns 100ns units of kernel+user time for the process, so
CPU% = delta(cpu) / delta(wall) * 100 is "percent of one core" - the same
convention the roadmap's 42.8% baseline was recorded with (the overlay is
single threaded, so it can never exceed 100% in this unit, while a
Task-Manager style share of all 8 cores would be at most 12.5%).

--move jitters the real pointer with SetCursorPos across the screen (never
clicks) to exercise the "cursor dot moves every frame" path, and restores the
pointer afterwards.
"""

import argparse
import ctypes
import ctypes.wintypes as wt
import json
import os
import subprocess
import sys
import time

k32 = ctypes.WinDLL("kernel32", use_last_error=True)
u32 = ctypes.WinDLL("user32", use_last_error=True)

k32.OpenProcess.restype = wt.HANDLE
k32.OpenProcess.argtypes = [wt.DWORD, wt.BOOL, wt.DWORD]
k32.CloseHandle.argtypes = [wt.HANDLE]
k32.GetProcessTimes.argtypes = [
    wt.HANDLE,
    ctypes.POINTER(wt.FILETIME),
    ctypes.POINTER(wt.FILETIME),
    ctypes.POINTER(wt.FILETIME),
    ctypes.POINTER(wt.FILETIME),
]
u32.SetCursorPos.argtypes = [ctypes.c_int, ctypes.c_int]
u32.GetCursorPos.argtypes = [ctypes.POINTER(wt.POINT)]

PROCESS_QUERY_LIMITED_INFORMATION = 0x1000


def cpu_seconds(pid: int) -> float:
    h = k32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
    if not h:
        raise OSError(ctypes.get_last_error(), "OpenProcess")
    try:
        c, e, k, u = (wt.FILETIME() for _ in range(4))
        if not k32.GetProcessTimes(h, ctypes.byref(c), ctypes.byref(e), ctypes.byref(k), ctypes.byref(u)):
            raise OSError(ctypes.get_last_error(), "GetProcessTimes")
        ft = lambda f: (f.dwHighDateTime << 32) | f.dwLowDateTime
        return (ft(k) + ft(u)) / 1e7
    finally:
        k32.CloseHandle(h)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("exe")
    ap.add_argument("--secs", type=float, default=8.0)
    ap.add_argument("--warmup", type=float, default=2.5)
    ap.add_argument("--move", action="store_true", help="jitter the real pointer while sampling")
    ap.add_argument("--json", help="also write the result here")
    ap.add_argument("--keep", action="store_true", help="leave the process running")
    args = ap.parse_args()

    exe = os.path.abspath(args.exe)
    p = subprocess.Popen([exe, "--map"], stdin=subprocess.DEVNULL,
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    result = {"exe": exe, "pid": p.pid, "secs": args.secs, "move": args.move}
    try:
        time.sleep(args.warmup)
        origin = wt.POINT()
        u32.GetCursorPos(ctypes.byref(origin))
        t0, c0 = time.perf_counter(), cpu_seconds(p.pid)
        if args.move:
            # sweep a rectangle in the middle of the screen: moves the dot every frame
            step = 0
            end = time.perf_counter() + args.secs
            while time.perf_counter() < end:
                x = 400 + int(300 + 250 * __import__("math").sin(step * 0.35))
                y = 300 + int(200 * __import__("math").cos(step * 0.35))
                u32.SetCursorPos(x, y)
                step += 1
                time.sleep(0.016)
            u32.SetCursorPos(origin.x, origin.y)
            wall = time.perf_counter() - t0
            cpu = cpu_seconds(p.pid) - c0
        else:
            time.sleep(args.secs)
            wall = time.perf_counter() - t0
            cpu = cpu_seconds(p.pid) - c0
        pct_one_core = cpu / wall * 100.0
        cores = os.cpu_count() or 1
        result.update({
            "wall_s": round(wall, 3),
            "cpu_s": round(cpu, 3),
            "cpu_pct_one_core": round(pct_one_core, 1),
            "cpu_pct_all_cores": round(pct_one_core / cores, 1),
            "logical_cores": cores,
        })
        print(json.dumps(result, ensure_ascii=False, indent=2))
        if args.json:
            with open(args.json, "w", encoding="utf-8") as f:
                json.dump(result, f, ensure_ascii=False, indent=2)
    finally:
        if not args.keep:
            p.terminate()
            try:
                p.wait(timeout=5)
            except subprocess.TimeoutExpired:
                p.kill()
    return 0


if __name__ == "__main__":
    sys.exit(main())
