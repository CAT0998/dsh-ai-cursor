"""Live panel verification: scroll, viewport hit-clipping, section split, naming.

The panel is a real Win32 window, so this harness drives the REAL process over its
line protocol for state, and the REAL mouse (SetCursorPos + mouse_event) for input.
That split matters: a protocol call can prove state, never that the wheel or a click
works.

Targets are found by widget **id** through the panel's own `hits` diagnostic, so the
checks survive a layout change instead of hard-coding coordinates.

    python scripts/live-panel-check.py bin/ai-mouse-overlay.exe artifacts
"""

import ctypes
import importlib.util
import json
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("wheels", HERE / "verify-panel-scroll.py")
wheels = importlib.util.module_from_spec(spec)
spec.loader.exec_module(wheels)  # type: ignore[union-attr]

EXE = Path(sys.argv[1] if len(sys.argv) > 1 else "bin/ai-mouse-overlay.exe").resolve()
OUT = Path(sys.argv[2] if len(sys.argv) > 2 else "artifacts").resolve()
OUT.mkdir(parents=True, exist_ok=True)

u32 = ctypes.windll.user32
HEADER = 41


class Panel:
    def __init__(self, exe: Path):
        self.p = subprocess.Popen(
            [str(exe), "--panel"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            bufsize=1,
        )
        self.seq = 0
        ready = json.loads(self.p.stdout.readline())
        assert ready.get("event") == "ready", ready
        self.pid = self.p.pid

    # NOTE: the first parameter must not be named `self` — `panel_roster` has a
    # payload key called `self`, and Python would bind it to the receiver instead.
    def req(_this, cmd, **kw):
        _this.seq += 1
        msg = {"id": str(_this.seq), "cmd": cmd, **kw}
        _this.p.stdin.write(json.dumps(msg) + "\n")
        _this.p.stdin.flush()
        for _ in range(400):
            line = _this.p.stdout.readline()
            if not line:
                raise RuntimeError("overlay died: " + _this.p.stderr.read())
            try:
                v = json.loads(line)
            except json.JSONDecodeError:
                continue
            if v.get("id") == msg["id"]:
                return v
        raise RuntimeError("no reply for " + cmd)

    def alive(self) -> bool:
        return self.p.poll() is None

    def hits(self):
        return self.req("hits")

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


results = []


def check(name, ok, got=""):
    results.append((name, bool(ok), got))
    print(("[PASS] " if ok else "[FAIL] ") + name + (" -> " + str(got) if got else ""))
    return ok


def hit_by_id(hits, ident):
    for h in hits:
        if h["id"] == ident:
            return h
    return None


def panel_rect(pn: Panel):
    return wheels.panel_rect(pn.pid)


def click_window(pn: Panel, wx: int, wy: int):
    hit = panel_rect(pn)
    assert hit, "panel window not found"
    _, left, top, _w, _h = hit
    wheels.click_at(left + wx, top + wy)


def wheel_over_panel(pn: Panel, notches: int, gap=0.12):
    hit = panel_rect(pn)
    assert hit, "panel window not found"
    _hwnd, left, top, w, h = hit
    u32.SetCursorPos(left + w // 2, top + h // 3)
    time.sleep(0.25)
    for _ in range(abs(notches)):
        wheels.wheel(1 if notches > 0 else -1)
        time.sleep(gap)
    time.sleep(0.35)


def ensure_visible(pn: Panel, ident: str, margin: int = 18):
    """Scroll (state, not input) until the widget is inside the viewport, then
    return its rect. Scrolling here is setup; the click is the real input."""
    for _ in range(8):
        data = pn.hits()
        view, sc = data["view_h"], data["scroll"]
        t = hit_by_id(data["hits"], ident)
        if t is None:
            return None
        if t["y"] >= HEADER + margin and t["y"] + t["h"] <= view - margin:
            return t
        want = max(0, sc + t["y"] + t["h"] // 2 - view // 2)
        pn.req("panel_scroll", y=want)
        time.sleep(0.2)
    return hit_by_id(pn.hits()["hits"], ident)


def click_hit(pn: Panel, ident: str, dx: int = 0, dy: int = 0) -> bool:
    t = ensure_visible(pn, ident)
    if t is None:
        print(f"  (no widget {ident})")
        return False
    click_window(pn, t["x"] + t["w"] // 2 + dx, t["y"] + t["h"] // 2 + dy)
    time.sleep(0.4)
    return True


def click_until(pn: Panel, ident: str, predicate, tries: int = 3, dx: int = 0, dy: int = 0) -> bool:
    """Click a widget and retry until the expected state appears.

    The desktop is shared: another agent or the human can scroll the panel or move a
    window between reading the rect and landing the click. A retry re-reads the live
    rect, so the check converges instead of depending on a quiet machine — and it
    stops the moment the state changed, so a toggle is never double-applied.
    """
    for _ in range(tries):
        if not click_hit(pn, ident, dx=dx, dy=dy):
            return False
        if predicate():
            return True
    return predicate()


def mailbox() -> dict:
    path = EXE.parent / "logs" / "dock-command.json"
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return {}


def dump_panel(pn: Panel, tag: str):
    p = OUT / f"panel-{tag}.bmp"
    r = pn.req("dump", path=str(p), target="panel")
    assert r.get("ok") and not r.get("dump_error"), r
    return p


def main() -> int:
    # A throwaway asset dir keeps the user's real presets out of the test.
    tmp = Path(tempfile.mkdtemp(prefix="panel-assets-"))
    (tmp / "menu-themes.json").write_text(
        json.dumps([{"name": "测试主题", "accent": "#7C5CFF", "zoom": 1.15}]),
        encoding="utf-8",
    )
    (tmp / "cursor-presets.json").write_text(
        json.dumps(
            [{"name": "默认", "theme": "arrow", "color": "#FF0044", "opacity": 1, "scale": 1, "badge": True}]
        ),
        encoding="utf-8",
    )

    pn = Panel(EXE)
    print(f"panel pid={pn.pid} assets={tmp}")
    st = pn.req("panel", visible=True)
    if st.get("panel") is not True:
        pn.req("resume")
        st = pn.req("panel", visible=True)
    pn.req("panel_expanded", expanded=True)
    pn.req("assets", dir=str(tmp))
    time.sleep(1.8)  # the panel re-reads its lists about every 1.5s

    st = pn.req("status")
    content, view = st["panel_content_h"], st["panel_view_h"]
    print(f"content_h={content} view_h={view} scroll={st['panel_scroll']}")
    check("A1 内容高于视口（需要滚动）", content > view, f"{content} > {view}")
    check("A2 视口不越过工作区（1080p 任务栏外）", view <= 1040 - 8, f"view={view}")
    dump_panel(pn, "scroll0")

    # ---- real wheel ----
    # The desktop is shared, so this asserts the DIRECTION and the clamp rather than
    # an exact pixel count: the point is that the real wheel message scrolls.
    before = pn.req("status")["panel_scroll"]
    wheel_over_panel(pn, -1)
    st = pn.req("status")
    check("A3 真滚轮一格 -> 内容向下滚", st["panel_scroll"] > before,
          f"{before} -> {st['panel_scroll']}")

    wheel_over_panel(pn, -12)
    st = pn.req("status")
    check(
        "A4 继续下滚 -> 停在最大值",
        st["panel_scroll"] == content - view,
        f"scroll={st['panel_scroll']} max={content-view}",
    )
    dump_panel(pn, "scrollmax")

    # ---- the section split ----
    data = pn.hits()
    ids = {h["id"] for h in data["hits"]}
    check("B1 「菜单主题」分区存在", "sec_menu_theme" in ids)
    check("B2 「AI 鼠标设置」分区存在", "sec_mouse" in ids)
    check("B3 只有一个预设分区（旧 sec_presets 已移除）",
          "sec_mouse_presets" in ids and "sec_presets" not in ids,
          sorted(i for i in ids if i.startswith("sec_")))
    titles = [h["text"] for h in data["hits"] if h["id"].startswith("sec_")]
    check("B4 「预设」字样只出现一次",
          sum(1 for t in titles if "预设" in t) == 1, titles)

    # ---- clicking a header that only became reachable by scrolling ----
    before_h = pn.req("status")["panel_content_h"]
    expanded = click_until(
        pn,
        "sec_mouse_presets",
        lambda: any(h["id"] == "mp_save" for h in pn.hits()["hits"]),
    )
    after_h = pn.req("status")["panel_content_h"]
    check("A6 点击滚出来的「鼠标预设」表头 -> 展开", expanded and after_h > before_h,
          f"{before_h} -> {after_h}")
    dump_panel(pn, "presets-open")

    # ---- the viewport guard ----
    # Find a SECTION header that is half scrolled under the header strip, then click
    # the hidden half. A section toggle changes the content height, so "nothing
    # happened" is provable — and a section is safe to click (no 退出 / 停止全部).
    target, before_h = None, pn.req("status")["panel_content_h"]
    for y in range(0, 600, 10):
        pn.req("panel_scroll", y=y)
        time.sleep(0.08)
        d = pn.hits()
        for h in d["hits"]:
            if (
                h["kind"] == "Section"
                and h["y"] < HEADER - 4
                and h["y"] + h["h"] > HEADER + 2
            ):
                target = h
                before_h = d["content_h"]
                break
        if target:
            break
    check("A7 存在被表头盖住一半的分区表头", target is not None, target and target["id"])
    if target:
        click_window(pn, target["x"] + 20, 20)  # y=20 在表头条里，命中框却包含它
        time.sleep(0.5)
        alive = pn.alive()
        after_h = pn.req("status")["panel_content_h"] if alive else None
        check("A8 点那不可见的一半 -> 无反应（守卫生效）",
              alive and after_h == before_h,
              f"{target['id']} y={target['y']} clicked y=20 content {before_h}->{after_h}")
    else:
        check("A8 没有半隐藏的可点控件可测", False)

    # ---- 菜单主题 与 AI 鼠标 accent 分离 ----
    visible = click_until(
        pn,
        "sec_menu_theme",
        lambda: any(h["id"] == "mt1" for h in pn.hits()["hits"]),
    )
    check("B5 「菜单主题」可以展开（默认折叠）", visible)
    st = pn.req("status")
    panel_accent0, cursor_color0 = st["panel_accent"], st["color"]
    check("B6 菜单主题列表读到了 menu-themes.json",
          st["menu_themes"] == ["测试主题"], st["menu_themes"])
    check("B7 鼠标预设列表读到了 cursor-presets.json",
          st["mouse_presets"] == ["默认"], st["mouse_presets"])

    changed = click_until(
        pn,
        "mt1",
        lambda: pn.req("status")["panel_accent"].upper() == "#7C5CFF",
    )
    st = pn.req("status")
    check("B8 改菜单强调色 -> panel_accent 变、光标颜色不变",
          changed and st["color"] == cursor_color0,
          f"panel={st['panel_accent']} cursor={st['color']} (was {panel_accent0}/{cursor_color0})")

    changed = click_until(
        pn,
        "sw3",
        lambda: pn.req("status")["color"].upper() == "#34D399",
    )
    st = pn.req("status")
    check("B9 改光标颜色 -> color 变、菜单强调色不变",
          changed and st["panel_accent"].upper() == "#7C5CFF",
          f"panel={st['panel_accent']} cursor={st['color']}")

    # ---- applying a saved menu theme ----
    applied = click_until(
        pn,
        "mt_apply_0",
        lambda: abs(float(pn.req("status")["panel_zoom"]) - 1.15) < 1e-6,
    )
    st = pn.req("status")
    check("B10 点主题列表 -> 应用文件里的 accent/zoom",
          applied and st["panel_accent"].upper() == "#7C5CFF",
          f"panel={st['panel_accent']} zoom={st['panel_zoom']}")

    # ---- the panel only WRITES intents; the host owns the files ----
    saved = click_until(pn, "mt_save", lambda: mailbox().get("action") == "menu_theme_save")
    box = mailbox()
    check("B11 「保存主题」把意图写进信箱（带 epoch/zoom）",
          saved and "epoch" in box and "zoom" in box, box)
    saved = click_until(pn, "mp_save", lambda: mailbox().get("action") == "cursor_preset_save")
    check("B12 「保存当前」把意图写进信箱", saved, mailbox())

    # leave the panel at 100% for whatever runs next
    pn.req("panel_scroll", y=0)
    click_until(pn, "mt_reset", lambda: abs(float(pn.req("status")["panel_zoom"]) - 1.0) < 1e-6)
    pn.req("panel_scroll", y=0)

    # ---- 选中鼠标：roster / ‹ › / 路由（C 阶段） ----
    # A stand-in for the host: the real one publishes exactly this shape from
    # `pushRoster()`. Two cursors, so the arrows and the name rule have work to do.
    roster = {
        "self": "default",
        "selected": 0,
        "cursors": [
            {
                "name": "default",
                "display": "默认",
                "theme": "whale",
                "label": "AI 鼠标",
                "color": "#4FC3F7",
                "opacity": 1,
                "scale": 1,
                "badge": True,
            },
            {
                "name": "agent-b",
                "display": "自定义1",
                "theme": "arrow",
                "label": "AI 鼠标",
                "color": "#FF0044",
                "opacity": 0.8,
                "scale": 1.2,
                "badge": True,
            },
        ],
    }
    pn.req("panel_roster", **roster)
    time.sleep(0.4)
    st = pn.req("status")
    check("C1 roster 到达面板（2 只，选中第 1 只）",
          st["sel_total"] == 2 and st["sel_index"] == 0 and st["sel_name"] == "默认",
          f"{st['sel_name']} {st['sel_index']+1}/{st['sel_total']}")

    moved = click_until(pn, "mouse_next", lambda: pn.req("status")["sel_index"] == 1)
    st = pn.req("status")
    check("C2 点 › -> 选中第二只，名字用 roster 的「自定义1」",
          moved and st["sel_name"] == "自定义1", f"{st['sel_name']} {st['sel_index']+1}/{st['sel_total']}")
    box = mailbox()
    check("C3 切换选中鼠标会写信箱（select_cursor + index）",
          box.get("action") == "select_cursor" and box.get("index") == 1, box)
    dump_panel(pn, "selected-2")

    # the mouse section must show the SELECTED cursor's values, not this process's
    hex_hit = next((h for h in pn.hits()["hits"] if h["id"] == "l_hex"), None)
    check("C4 设置区显示选中那只的颜色（#FF0044）",
          hex_hit is not None and hex_hit["text"].upper() == "#FF0044", hex_hit and hex_hit["text"])

    # An edit while another cursor is selected is FORWARDED, not applied locally.
    # The colour is deliberately different from this cursor's, so "unchanged" means
    # something.
    color_before = pn.req("status")["color"]
    (EXE.parent / "logs" / "dock-command.json").unlink(missing_ok=True)
    forwarded = click_until(pn, "sw6", lambda: mailbox().get("action") == "cursor_patch")
    box = mailbox()
    st = pn.req("status")
    check("C5 编辑别的鼠标 -> 写 cursor_patch 给 agent-b",
          forwarded and box.get("target") == "agent-b"
          and (box.get("patch") or {}).get("color", "").upper() == "#FF4D4F", box)
    check("C6 同时本地光标颜色不变（没有误改自己）", st["color"] == color_before,
          f"local {color_before} -> {st['color']}")

    # switching back to itself must apply locally again
    back = click_until(pn, "mouse_prev", lambda: pn.req("status")["sel_index"] == 0)
    (EXE.parent / "logs" / "dock-command.json").unlink(missing_ok=True)
    local = click_until(
        pn,
        "sw5",
        lambda: pn.req("status")["color"].upper() == "#E2E8F0",
    )
    check("C7 切回本光标后再改颜色 -> 本进程立即生效",
          back and local, pn.req("status")["color"])
    check("C8 本地编辑不再写信箱", not (EXE.parent / "logs" / "dock-command.json").exists())

    # ---- 鼠标预设：应用 / 删除（D 阶段，落盘由宿主完成，单测覆盖） ----
    applied = click_until(
        pn,
        "mp_apply_0",
        lambda: mailbox().get("action") == "cursor_preset_apply",
    )
    box = mailbox()
    check("D1 点预设名 -> 写 cursor_preset_apply（带名字与目标）",
          applied and box.get("name") == "默认" and "target" in box, box)
    (EXE.parent / "logs" / "dock-command.json").unlink(missing_ok=True)
    deleted = click_until(
        pn,
        "mp_delete",
        lambda: mailbox().get("action") == "cursor_preset_delete",
    )
    box = mailbox()
    check("D2 选中后点「删除所选」-> 写 cursor_preset_delete（带名字）",
          deleted and box.get("name") == "默认", box)

    # ---- back to the top ----
    wheel_over_panel(pn, 24)
    st = pn.req("status")
    check("A9 上滚到头 -> clamp 到 0", st["panel_scroll"] == 0, f"scroll={st['panel_scroll']}")
    dump_panel(pn, "scrolltop")

    log = Path(pn.req("status").get("log_path", ""))
    if log.exists():
        raw = log.read_text(encoding="utf-8", errors="replace")
        check("A10 日志里有真实滚轮原始值", "panel wheel raw=" in raw, "panel wheel raw=…")

    pn.close()
    for name in ("scroll0", "scrollmax", "presets-open", "scrolltop", "selected-2"):
        bmp = OUT / f"panel-{name}.bmp"
        check(f"A11 帧已写出 {bmp.name}", bmp.exists() and bmp.stat().st_size > 1000,
              f"{bmp.stat().st_size if bmp.exists() else 0} bytes")

    failed = [r for r in results if not r[1]]
    print(f"\n{len(results) - len(failed)}/{len(results)} passed")
    for name, _, got in failed:
        print("  FAILED:", name, got)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
