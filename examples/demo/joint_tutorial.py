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


# What the voice-over says for each caption (voice.py speaks these; English names are written the
# way a Thai speaker says them, so the Thai voice reads them naturally).
NARRATION = {
    "ตัวอย่าง: ทำกระบอกลมด้วย Joint แบบ Slider": "สวัสดีครับ วันนี้เรามาลองทำกระบอกลม ด้วยคำสั่ง จอยท์ แบบสไลเดอร์ กันนะครับ",
    "1. ไปที่แท็บ Assemble แล้วกด Joint": "เริ่มจากไปที่แท็บ แอสเซมบลี้ แล้วกดปุ่ม จอยท์ ครับ",
    "2. Fixed part: คลิกหน้าปลายกระบอก (ชิ้นที่อยู่กับที่)": "ช่องแรก ฟิกซ์ พาร์ท คือชิ้นที่อยู่กับที่ เราคลิกที่หน้าปลายกระบอกเลยครับ",
    "3. Moving part: คลิกหน้าปลายก้านสูบ (ชิ้นที่จะขยับ) ก้านจะกระโดดเข้ามาประกบ": "ต่อไปช่อง มูฟวิ่ง พาร์ท คือชิ้นที่จะขยับ คลิกที่หน้าปลายก้านสูบ แล้วก้านจะเข้ามาประกบให้เองครับ",
    "4. Type: เลือก Slider (เลื่อนตามแกน)": "ตรงช่อง ไทป์ เลือกเป็น สไลเดอร์ ก็คือให้เลื่อนไปตามแกนได้อย่างเดียว",
    "5. Range: ติ๊ก Keep within ใส่ระยะชัก From -80 mm To 0 mm": "แล้วมากำหนดระยะชักกันครับ ติ๊ก คีพ วิธอิน แล้วใส่ ลบแปดสิบ มิล ถึง ศูนย์ มิล",
    "6. กด Play ดูก้านเลื่อนเข้า-ออก ได้แค่ในช่วง Range ที่ตั้ง": "ลองกด เพลย์ ดูครับ ก้านจะเลื่อนเข้าออก อยู่แค่ในช่วงที่เราตั้งไว้ ไม่หลุดออกจากกระบอก",
    "7. กด OK เสร็จ ข้อต่อจะอยู่ใน Browser หัวข้อ Joints": "เรียบร้อยแล้วก็กด โอเค ได้เลยครับ ข้อต่อที่เราทำจะไปอยู่ในหัวข้อ จอยท์ ทางซ้ายมือ ง่ายๆ แค่นี้เองครับ",
}


def handle_like(d, *words):
    """The click point of the first dialog handle whose name holds all `words`, scrolled into
    view: a row below the dialog's fold still has a handle (outside the visible part), so the
    dialog is scrolled until the row sits above the OK button."""
    for attempt in range(10):
        handles = d.ui("ui.handles")
        ok = next((h for h in handles if h["name"] == "dialog:OK"), None)
        card = next((h for h in handles if h["name"] in ("dialog:card", "dialog:Type")), None)
        for h in handles:
            if all(w in h["name"] for w in words):
                at = d.at(handle=h["name"])
                if ok is None or at[1] < ok["y"] - 30:
                    return at
                break
        if card is None:
            break
        d.ui("ui.scroll", {"x": card["x"], "y": card["y"] + 60, "delta": -120})
        d.wait(0.4)
        d.settle_camera()
    raise RuntimeError(f"no visible handle with {words}: {[(h['name'], round(h['y'])) for h in d.ui('ui.handles') if h['name'].startswith('dialog')]}")


def has(d, word):
    return any(word in h["name"] for h in d.ui("ui.handles") if h["name"].startswith("dialog"))


def tick_range(d):
    """Tick the Range check box: its handle first, then along its row, until From shows."""
    # The Range section open first (it remembers being folded).
    if not has(d, "limits"):
        d.click(d.at(handle="dialog:section:Range"), settle=0.5)
        d.settle_camera()
    box = handle_like(d, "dialog:", "limits")
    row = handle_like(d, "dialog:row:", "limits")
    for at in [box, (row[0] + 60, row[1]), (row[0] + 110, row[1]), (row[0] - 40, row[1])]:
        d.click(at, settle=0.5)
        d.settle_camera()
        # The From row may open below the dialog's fold: scroll down a little before looking.
        for _ in range(3):
            if has(d, "From"):
                return
            card = next((h for h in d.ui("ui.handles") if h["name"] in ("dialog:card", "dialog:Type")), None)
            if card is None:
                break
            d.ui("ui.scroll", {"x": card["x"], "y": card["y"] + 120, "delta": -200})
            d.wait(0.3)
            d.settle_camera()
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
    d.click(d.at(world=[110, 0, 15]), settle=1.0)
    d.wait(0.8)
    d.caption("3. Moving part: คลิกหน้าปลายก้านสูบ (ชิ้นที่จะขยับ) ก้านจะกระโดดเข้ามาประกบ", "")
    d.click(d.at(world=[120, 55, 15]), settle=1.5)
    d.wait(1.2)
    d.caption("4. Type: เลือก Slider (เลื่อนตามแกน)", "")
    d.click(d.at(handle="dialog:Type"), settle=0.4)
    d.click(d.at(handle="dialog:Type:Slider"), settle=0.8)
    d.wait(0.8)
    # Fold Options away so the Range rows fit on screen.
    if has(d, "Flip"):
        d.click(d.at(handle="dialog:section:Options"), settle=0.4)
    d.caption("5. Range: ติ๊ก Keep within ใส่ระยะชัก From -80 mm To 0 mm", "")
    tick_range(d)
    fill(d, handle_like(d, "dialog:", "From"), "-80 mm")
    fill(d, handle_like(d, "dialog:", "To (no"), "0 mm")
    d.ui("ui.key", {"key": "Tab"})
    d.wait(0.8)
    d.caption("6. กด Play ดูก้านเลื่อนเข้า-ออก ได้แค่ในช่วง Range ที่ตั้ง", "")
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
        d.wait(7.0)
        tap(play)
        d.wait(0.5)
    else:
        d.wait(3.0)
    d.caption("7. กด OK เสร็จ ข้อต่อจะอยู่ใน Browser หัวข้อ Joints", "")
    d.click(d.at(handle="dialog:OK"), settle=1.0)
    d.wait(2.5)
    d.caption("", "")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--port", type=int, default=7878)
    ap.add_argument("--design", required=True)
    ap.add_argument("--log")
    ap.add_argument("--speed", type=float, default=1.0)
    ap.add_argument("--voice", help="voice.py's index.json: each caption waits for its narration")
    a = ap.parse_args()
    d = Demo(a.port, None, open(a.log, "w") if a.log else None, a.speed, 2.2)
    if a.voice:
        import json

        d.voice = {k: v["seconds"] for k, v in json.load(open(a.voice)).items()}
    d.ui("ui.prefs", {"language": "en", "theme": "light", "ui_scale": 1, "button": "apply"})
    d.ui("ui.resize", {"width": 1920, "height": 1080})
    # The whole screen, and a larger interface so the video reads well.
    try:
        d.ui("ui.window", {"action": "maximize"})
    except RuntimeError:
        pass
    d.ui("ui.prefs", {"ui_scale": 1.15, "button": "apply"})
    # The window's size is in interface points: fill the 1920 x 1080 screen at this scale.
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
    _ = control


if __name__ == "__main__":
    main()
