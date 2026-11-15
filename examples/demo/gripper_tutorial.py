#!/usr/bin/env python3
"""A how-to video: a centering gripper (an air cylinder pushes a cone down between two fingers;
rollers on the fingers ride the cone, so the fingers close; springs open them) made to move in
the running app, step by step: As-Built Joint (Revolute) for each finger, Wedge Cam for each
finger, Spring for each finger, then played.

    solidcraft --control 7878 &
    python3 examples/demo/gripper_tutorial.py --port 7878 --design gripper-parts.draftnew --log steps.log

The design is made by the engine's example builder (`make_centering_gripper_example`, with
GRIPPER_PARTS_ONLY): the frame, the cylinder and the plunger already joined, the fingers loose.
Captions are Thai (the person this was made for); there is no voice-over."""

import argparse
import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from clamp_tutorial import handles, open_as_built, revolute  # noqa: E402
from enclosure_ui_mcp import Demo  # noqa: E402

# Where to click (world mm), seen from the front (looking along +Y).
FRAME = [0, -25, 60]  # the front fork plate
PLUNGER = [0, -2, 18]  # the plunger's centring pin under the fork plates
FINGER = {"Right": [25.5, -7, 0.8], "Left": [-25.5, -7, 0.8]}  # each finger's lower arm
PIVOT = {"Right": [17.5, -27, 34], "Left": [-17.5, -27, 34]}  # the pivot pins' heads
SPRING_FINGER = {"Right": [23.6, -7, 18.7], "Left": [-23.6, -7, 18.7]}
SPRING_POST = {"Right": [43, -5, 18.7], "Left": [-43, -5, 18.7]}


def open_tool(d, command):
    """A ribbon button on the ASSEMBLE tab, from its panel's drop-down when it is folded; the
    command started directly when no button shows (the video still shows its dialog)."""
    if f"toolbar:{command}" not in handles(d):
        try:
            d.click(d.at(handle="tab:ASSEMBLE"), settle=0.6)
        except RuntimeError:
            pass
    if f"toolbar:{command}" not in handles(d):
        for name in [h for h in handles(d) if h.startswith("panel:ASSEMBLE:")]:
            d.click(d.at(handle=name), settle=0.4)
            if f"toolbar:{command}" in handles(d):
                break
    if f"toolbar:{command}" in handles(d):
        d.tool(command)
    else:
        d.ui("ui.start", {"command": command})
    d.wait(0.8)


def ok(d):
    d.click(d.at(handle="dialog:OK"), settle=1.2)
    d.wait(0.8)


def finger_joint(d, step, side):
    d.caption(f"{step}. As-Built Joint แบบ Revolute: นิ้ว{'ขวา' if side == 'Right' else 'ซ้าย'} หมุนรอบสลัก", "")
    open_as_built(d)
    revolute(d)
    d.caption("   Fixed part: คลิกโครง (แผ่นหน้า สีชมพู)", "")
    d.click(d.at(world=FRAME), settle=0.6)
    d.caption("   Moving part: คลิกนิ้วจับ (สีฟ้า)", "")
    d.click(d.at(world=FINGER[side]), settle=0.8)
    d.caption("   Position: คลิกช่อง Position ก่อน แล้วคลิกหัวสลัก (วงกลมสีเหลือง) = แกนหมุน", "")
    # Moving part takes several parts, so it stays the active input: Position is clicked first.
    row = d.at(handle="dialog:row:Position")
    d.click((row[0] + 60, row[1]), settle=0.5)
    d.click(d.at(world=PIVOT[side]), settle=0.8)
    d.wait(0.8)
    d.caption("   กด OK", "")
    ok(d)


def wedge(d, step, side):
    d.caption(f"{step}. Wedge Cam (แท็บ Assemble > Motion): ลูกกลิ้งบนนิ้ว{'ขวา' if side == 'Right' else 'ซ้าย'} วิ่งตามกรวย", "")
    open_tool(d, "joint.wedge")
    d.caption("   Wedge slides on = กระบอกลม, Lever turns on = สลักนิ้ว (โปรแกรมเลือกให้แล้ว)", "")
    d.wait(2.0)
    d.caption("   คลิกตัวกรวย (สีน้ำเงิน) ลูกกลิ้งไม่ต้องคลิก โปรแกรมหาให้เอง", "")
    d.click(d.at(world=PLUNGER), settle=0.8)
    d.wait(0.8)
    d.caption("   กด OK: นิ้วจะหมุนตามรูปกรวยเมื่อกรวยเลื่อน", "")
    ok(d)


def spring(d, step, side):
    d.caption(f"{step}. Spring: สปริงดึงนิ้ว{'ขวา' if side == 'Right' else 'ซ้าย'} ให้อ้าออก (ลูกกลิ้งแนบกรวยตลอด)", "")
    open_tool(d, "motion.spring")
    d.caption("   End 1: คลิกจุดบนนิ้ว   End 2: คลิกจุดบนเสาด้านนอก", "")
    d.click(d.at(world=SPRING_FINGER[side]), settle=0.6)
    d.click(d.at(world=SPRING_POST[side]), settle=0.8)
    d.wait(0.8)
    d.caption("   ใส่ Coil radius / Wire / Turns ตามสปริงจริง แล้วกด OK", "")
    ok(d)


def play(d, study, seconds=5.0, rate=20):
    steps = int(seconds * rate)
    for k in range(steps + 1):
        d.ui("engine.execute", {"command": "motion.play", "params": {"study": study, "step": k}})
        time.sleep(1.0 / rate)


def run(d):
    d.caption("ตัวอย่าง: Centering Gripper (กรวยดันลูกกลิ้ง นิ้วหนีบ + สปริงดึงอ้า)", "")
    d.ui("ui.view", {"view": "front", "animate": True})
    d.wait(1.5)
    d.settle_camera()
    # Close in on the fingers (the dialog takes the right of the window).
    d.ui("ui.camera", {"target": [30, 0, 45], "distance": 210})
    d.wait(1.0)
    d.settle_camera()
    d.caption("เตรียมชิ้นไว้แล้ว: โครง (ยึดอยู่กับที่), กระบอกลม + กรวย (ยึดกันแล้ว), นิ้วจับ 2 ข้าง (ยังลอย)", "")
    d.wait(3.0)
    finger_joint(d, 1, "Right")
    finger_joint(d, 2, "Left")
    wedge(d, 3, "Right")
    wedge(d, 4, "Left")
    spring(d, 5, "Right")
    spring(d, 6, "Left")
    d.caption("7. Motion Sequence: กระบอกลมยืด 11.5 มม. (ปากแตะชิ้นงาน) 1.6 วินาที → รอ → หด", "")
    open_tool(d, "motion.sequence")
    d.wait(2.5)
    try:
        d.ui("ui.key", {"key": "Escape"})
    except RuntimeError:
        pass
    d.wait(0.5)
    cyl = "Cylinder"  # the air cylinder's slider, named after it
    d.ui("engine.execute", {"command": "motion.sequence", "params": {"name": "Clamp", "moves": [{"joint": cyl, "to": 11.5, "time": 1.6}, {"wait": 1.0}, {"joint": cyl, "to": 0, "time": 1.6}, {"wait": 0.8}]}})
    d.caption("8. เล่น: กรวยลง → หมุดกลางเข้ารูชิ้นงาน (centering) → นิ้วหนีบ → กรวยขึ้น สปริงดึงนิ้วอ้า", "")
    for _ in range(2):
        play(d, "Clamp")
        d.wait(0.5)
    d.caption("   หมุนดูรอบๆ ได้ (สปริงยืด-หดตามนิ้ว)", "")
    d.ui("ui.view", {"view": "iso", "animate": True})
    d.wait(1.0)
    d.ui("ui.camera", {"target": [0, 0, 80], "distance": 420})
    d.wait(1.2)
    play(d, "Clamp")
    d.caption("สรุป: As-Built Joint (Revolute) ×2 → Wedge Cam ×2 → Spring ×2 → Motion Sequence → Play", "")
    d.wait(5.0)
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
    try:
        run(d)
    finally:
        d.mark(caption="", via="")
        # What was made, for checking the run (the log is uploaded with the video).
        try:
            print("joints:", d.ui("engine.execute", {"command": "joint.list", "params": {}}), file=sys.stderr)
        except RuntimeError as e:
            print("joint.list:", e, file=sys.stderr)
    print(f"done in {time.time() - t0:.1f}s", file=sys.stderr)


if __name__ == "__main__":
    main()
