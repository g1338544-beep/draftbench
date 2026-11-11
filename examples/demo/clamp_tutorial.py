#!/usr/bin/env python3
"""A how-to video: a toggle clamp (a four-bar linkage) made to move with As-Built Joints, clicked
step by step in the running app, then played.

    solidcraft --control 7878 &
    python3 examples/demo/clamp_tutorial.py --port 7878 --design toggle-clamp-parts.draftnew --log steps.log

The design is examples/motion/toggle-clamp-parts.json built with `solidcraft-cli run`: a grounded
base with two pins, an arm with its spindle, a handle and a link, already put together as a clamp.
Captions are Thai (the person this was made for); there is no voice-over."""

import argparse
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from enclosure_ui_mcp import Demo  # noqa: E402
from joint_tutorial import fill, handle_like, has, tick_range  # noqa: E402

# The pins: where each joint turns (top rims of the pins, as the view sees them from above).
PIN_A = [2.5, 0, 14]  # base - arm
PIN_B = [2.5, 40, 20]  # arm - handle
PIN_C = [32.5, 40, 20]  # handle - link
PIN_D = [42.5, 0, 14]  # base - link

# A point on each part that nothing covers in the top view.
BASE = [-40, -15, 8]
ARM = [-40, 40, 14]
HANDLE = [65, 40, 20]
LINK = [36, 16, 14]


def handles(d):
    return [h["name"] for h in d.ui("ui.handles")]


def open_as_built(d):
    """The As-Built Joint button: on the ASSEMBLE tab, in its panel's drop-down when it is folded."""
    d.click(d.at(handle="tab:ASSEMBLE"), settle=0.6)
    if "toolbar:joint.as_built" not in handles(d):
        for name in [h for h in handles(d) if h.startswith("panel:ASSEMBLE:")]:
            d.click(d.at(handle=name), settle=0.4)
            if "toolbar:joint.as_built" in handles(d):
                break
    d.tool("joint.as_built")
    d.wait(0.8)


def joint(d, step, a, a_name, b, b_name, pin, pin_name):
    d.caption(f"{step}. Components: คลิก {a_name} แล้วคลิก {b_name} (สองชิ้นที่ต่อกัน)", "")
    d.click(d.at(world=a), settle=0.6)
    d.click(d.at(world=b), settle=0.8)
    d.wait(0.6)
    d.caption(f"   Position: คลิกขอบสลักที่จุด {pin_name} (แกนหมุนอยู่ตรงกลางสลัก)", "")
    d.click(d.at(world=pin), settle=0.8)
    d.wait(0.8)


def apply(d):
    d.caption("   กด Apply: ได้ข้อต่อแล้ว หน้าคำสั่งยังเปิดอยู่ ทำข้อต่อถัดไปต่อได้เลย", "")
    d.click(d.at(handle="dialog:Apply"), settle=1.2)
    d.wait(1.0)


def run(d):
    d.caption("ตัวอย่าง: ทำ Toggle Clamp ให้ขยับได้ (กลไก 4 ข้อต่อ / Four-bar)", "")
    d.view("home", 1.0)
    d.wait(2.0)
    d.caption("ชิ้นงาน 4 ชิ้น: Base (ยึดอยู่กับที่ สีเทา), Arm (สีฟ้า), Handle (สีส้ม), Link (สีเขียว)", "")
    d.wait(3.0)
    d.caption("วางชิ้นงานไว้ตามตำแหน่งจริงแล้ว ใช้ As-Built Joint ชิ้นจะไม่ขยับตอนทำข้อต่อ", "")
    d.ui("ui.view", {"view": "top", "animate": True})
    d.wait(1.5)
    d.settle_camera()
    d.caption("1. แท็บ Assemble แล้วกด As-Built Joint", "")
    open_as_built(d)
    d.caption("2. Type: เลือก Revolute (หมุนรอบแกนสลัก)", "")
    d.click(d.at(handle="dialog:Type"), settle=0.4)
    d.click(d.at(handle="dialog:Type:Revolute"), settle=0.8)
    d.wait(0.8)
    joint(d, 3, BASE, "Base", ARM, "Arm", PIN_A, "A")
    apply(d)
    joint(d, 4, ARM, "Arm", HANDLE, "Handle", PIN_B, "B")
    apply(d)
    joint(d, 5, HANDLE, "Handle", LINK, "Link", PIN_C, "C")
    apply(d)
    joint(d, 6, BASE, "Base", LINK, "Link", PIN_D, "D")
    d.caption("ครบ 4 ข้อต่อ: Base-Arm-Handle-Link ต่อกันเป็นวง (Four-bar)", "")
    d.wait(1.5)
    # Fold Options away so the Range rows fit on screen.
    if has(d, "Flip"):
        d.click(d.at(handle="dialog:section:Options"), settle=0.4)
    d.caption("7. Range: ติ๊ก Keep within ให้ Link หมุนได้ -20° ถึง 20°", "")
    tick_range(d)
    fill(d, handle_like(d, "dialog:", "From"), "-20 deg")
    fill(d, handle_like(d, "dialog:", "To (no"), "20 deg")
    d.ui("ui.key", {"key": "Tab"})
    d.wait(0.8)
    d.caption("8. กด Play: หมุนข้อต่อเดียว ชิ้นอื่นขยับตามทั้งวง ด้ามกด แขนยกขึ้น-ลง", "")
    d.ui("ui.view", {"view": "iso", "animate": True})
    d.wait(1.2)
    try:
        play = handle_like(d, "Play")
    except RuntimeError:
        play = None
    if play:
        # A playing joint keeps the view moving, so these clicks do not wait for it to settle.
        def tap(at):
            d.glide(at, 0.3)
            d.ui("ui.click", {"x": at[0], "y": at[1]})
            d.mark(cursor=list(at), click=True)

        tap(play)
        d.wait(9.0)
        tap(play)
        d.wait(0.5)
    else:
        d.wait(3.0)
    d.caption("9. กด OK จบคำสั่ง ข้อต่อทั้ง 4 อยู่ใน Browser หัวข้อ Joints", "")
    d.click(d.at(handle="dialog:OK"), settle=1.0)
    d.wait(3.0)
    d.caption("", "")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--port", type=int, default=7878)
    ap.add_argument("--design", required=True)
    ap.add_argument("--log")
    ap.add_argument("--speed", type=float, default=1.0)
    ap.add_argument("--voice", help="unused: this video has captions only")
    a = ap.parse_args()
    d = Demo(a.port, None, open(a.log, "w") if a.log else None, a.speed, 3.0)
    d.ui("ui.prefs", {"language": "en", "theme": "light", "ui_scale": 1, "button": "apply"})
    d.ui("ui.resize", {"width": 1920, "height": 1080})
    try:
        d.ui("ui.window", {"action": "maximize"})
    except RuntimeError:
        pass
    d.ui("ui.prefs", {"ui_scale": 1.15, "button": "apply"})
    d.ui("ui.resize", {"width": round(1920 / 1.15), "height": round(1080 / 1.15)})
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


if __name__ == "__main__":
    main()
