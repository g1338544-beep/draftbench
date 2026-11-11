#!/usr/bin/env python3
"""A how-to video of the Joint dialog: a slider for an air cylinder, clicked step by step in the
running app (the real dialog, real picks), then the rod dragged with the mouse.

    solidcraft --control 7878 &
    python3 examples/demo/joint_tutorial.py --port 7878 --design cylinder-parts.draftnew --log steps.log

The design is examples/motion/cylinder-parts.json built with `solidcraft-cli run`: a grounded
barrel and a loose rod beside it. Captions are Thai (the person this was made for)."""

import argparse
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from enclosure_mcp import control  # noqa: E402
from enclosure_ui_mcp import Demo  # noqa: E402


def handle_like(d, *words):
    """The first handle drawn whose name holds all `words`; the dialog is scrolled down (a few
    times) to bring a row below its fold into view."""
    for attempt in range(6):
        handles = d.ui("ui.handles")
        for h in handles:
            if all(w in h["name"] for w in words):
                return (h["x"], h["y"])
        card = next((h for h in handles if h["name"] == "dialog:card"), None) or next((h for h in handles if h["name"] == "dialog:Type"), None)
        if card is None:
            break
        d.ui("ui.scroll", {"x": card["x"], "y": card["y"] + 120, "delta": -200})
        d.wait(0.3)
        d.settle_camera()
    raise RuntimeError(f"no handle with {words}: {[h['name'] for h in d.ui('ui.handles') if h['name'].startswith('dialog')]}")


def has(d, word):
    return any(word in h["name"] for h in d.ui("ui.handles") if h["name"].startswith("dialog"))


def tick_range(d):
    """Tick the Range check box: its handle first, then along its row, until From shows."""
    box = handle_like(d, "dialog:", "limits")
    row = handle_like(d, "dialog:row:", "limits")
    for at in [box, (row[0] + 60, row[1]), (row[0] + 110, row[1]), (row[0] - 40, row[1])]:
        d.click(at, settle=0.5)
        d.settle_camera()
        if has(d, "From"):
            return
    raise RuntimeError("the Range check box did not tick")


def fill(d, at, text):
    d.click(at, settle=0.2)
    d.ui("ui.key", {"key": "A", "cmd": True})
    d.type(text, enter=False)


def run(d):
    d.caption("ตัวอย่าง: ทำกระบอกลมด้วย Joint แบบ Slider", "")
    d.view("home", 1.0)
    d.wait(1.0)
    d.caption("1. ไปที่แท็บ Assemble แล้วกด Joint", "")
    d.click(d.at(handle="tab:ASSEMBLE"), settle=0.6)
    d.tool("joint.create")
    d.wait(0.8)
    d.caption("2. Fixed part: คลิกหน้าปลายกระบอก (ชิ้นที่อยู่กับที่)", "")
    d.click(d.at(world=[110, 7, 22]), settle=1.0)
    d.wait(0.8)
    d.caption("3. Moving part: คลิกหน้าปลายก้านสูบ (ชิ้นที่จะขยับ) ก้านจะกระโดดเข้ามาประกบ", "")
    d.click(d.at(world=[120, 57, 17]), settle=1.5)
    d.wait(1.2)
    d.caption("4. Type: เลือก Slider (เลื่อนตามแกน)", "")
    d.click(d.at(handle="dialog:Type"), settle=0.4)
    d.click(d.at(handle="dialog:Type:Slider"), settle=0.8)
    d.wait(0.8)
    d.caption("5. Range: ติ๊ก Keep within ใส่ระยะชัก From -80 mm To 0 mm", "")
    tick_range(d)
    fill(d, handle_like(d, "dialog:", "From"), "-80 mm")
    fill(d, handle_like(d, "dialog:", "To"), "0 mm")
    d.wait(0.8)
    d.caption("6. กด OK", "")
    d.click(d.at(handle="dialog:OK"), settle=1.0)
    d.wait(0.8)
    d.caption("7. ปิดหน้าต่างทั้งหมดแล้วคลิกค้างที่ก้านสูบ ลากเข้า-ออกด้วยเมาส์", "")
    frm = d.at(world=[200, 4, 21])
    back = d.at(world=[130, 4, 21])
    d.drag(frm, back, steps=60)
    d.wait(0.6)
    d.drag(back, frm, steps=60)
    d.wait(0.8)
    d.caption("ก้านเลื่อนได้แค่ในช่วง Range ที่ตั้ง ไม่หลุดออกจากกระบอก", "")
    d.drag(frm, d.at(world=[60, 4, 21]), steps=60)
    d.wait(1.5)
    d.caption("", "")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--port", type=int, default=7878)
    ap.add_argument("--design", required=True)
    ap.add_argument("--log")
    ap.add_argument("--speed", type=float, default=1.0)
    a = ap.parse_args()
    d = Demo(a.port, None, open(a.log, "w") if a.log else None, a.speed, 2.2)
    d.ui("ui.prefs", {"language": "en", "theme": "light", "ui_scale": 1, "button": "apply"})
    d.ui("ui.resize", {"width": 1920, "height": 1080})
    d.ui("ui.home", {"open": False})
    d.ui("ui.file", {"path": os.path.abspath(a.design)})
    for _ in range(1200):
        if not d.ui("ui.inspect").get("file_busy", False):
            break
        time.sleep(0.1)
    d.ui("ui.home", {"open": False})
    d.settle_camera()
    t0 = time.time()
    run(d)
    d.mark(caption="", via="")
    print(f"done in {time.time() - t0:.1f}s", file=sys.stderr)
    _ = control


if __name__ == "__main__":
    main()
