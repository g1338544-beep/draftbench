#!/usr/bin/env python3
"""shots.py PORT OUTDIR: screenshots of the running app for review (Home page, the sample design
in the default look, and the ribbon), taken through the control channel."""

import os
import sys
import time

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from enclosure_mcp import control  # noqa: E402


def ui(port, method, params=None):
    r = control(port, method, params or {})
    if not r.get("ok", False):
        raise RuntimeError(f"{method}: {r.get('error')}")
    return r.get("result")


def main():
    port, out = int(sys.argv[1]), os.path.abspath(sys.argv[2])
    os.makedirs(out, exist_ok=True)
    ui(port, "ui.resize", {"width": 1600, "height": 900})
    ui(port, "ui.prefs", {"theme": "light", "button": "apply"})
    time.sleep(1.5)
    ui(port, "ui.home", {"open": True})
    time.sleep(2.5)
    ui(port, "ui.screenshot", {"path": f"{out}/home.png"})
    ui(port, "ui.home", {"open": False})
    time.sleep(1.0)
    ui(port, "ui.screenshot", {"path": f"{out}/model.png"})
    ui(port, "ui.prefs", {"theme": "dark", "button": "apply"})
    time.sleep(1.5)
    ui(port, "ui.screenshot", {"path": f"{out}/model-dark.png"})
    ui(port, "ui.home", {"open": True})
    time.sleep(1.5)
    ui(port, "ui.screenshot", {"path": f"{out}/home-dark.png"})


if __name__ == "__main__":
    main()
