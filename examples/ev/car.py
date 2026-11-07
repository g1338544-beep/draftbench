#!/usr/bin/env python3
"""Generates the command script of the electric-car example (dimensions in mm, X forward, Z up)."""
import json, math, sys

C = []
def cmd(_command, **params):
    C.append({"command": _command, "params": params})

def line(pts, closed=False, ids=None):
    p = {"points": pts}
    if closed:
        p["closed"] = True
    cmd("sketch.line", **p)

def sketch(plane, name, offset=None):
    p = {"plane": plane, "name": name}
    if offset is not None:
        p["offset"] = offset
    cmd("sketch.create", **p)

def finish():
    cmd("sketch.finish")

# --- Body: the side profile and the top view, each extruded, then intersected -------------------
sketch("XZ", "Side profile")
spline = lambda pts: cmd("sketch.spline.fit_point", points=pts)
spline([[2250, 520], [1900, 640], [1500, 770], [1100, 900]])            # bonnet
line([[1100, 900], [420, 1400]])                                         # windscreen
spline([[420, 1400], [0, 1455], [-700, 1425], [-1050, 1330]])            # roof
line([[-1050, 1330], [-1850, 1060]])                                     # rear window
spline([[-1850, 1060], [-2100, 985], [-2250, 860]])                      # tail
line([[-2250, 860], [-2250, 260]])
line([[-2250, 260], [2250, 230]])                                        # floor
line([[2250, 230], [2250, 520]])                                         # nose
finish()
cmd("solid.extrude", distance=2000, direction="symmetric", operation="new_body", body_name="Side")

sketch("XY", "Top view")
spline([[2260, -300], [1800, -780], [1200, -915], [0, -930], [-1200, -915], [-1900, -850], [-2260, -700]])
line([[-2260, -700], [-2260, 700]])
spline([[-2260, 700], [-1900, 850], [-1200, 915], [0, 930], [1200, 915], [1800, 780], [2260, 300]])
line([[2260, 300], [2260, -300]])
finish()
cmd("solid.extrude", distance=1600, operation="new_body", body_name="Top")
cmd("solid.combine", target="Side", tools=["Top"], operation="intersect")

# --- Wheel arches --------------------------------------------------------------------------------
for tag, x in (("FL", 1380), ("RL", -1420)):
    cmd("solid.cylinder", base=[x, -1000, 345], axis="Y", radius=395, height=500, operation="new_body", body_name="Arch " + tag)
    cmd("solid.cylinder", base=[x, 500, 345], axis="Y", radius=395, height=500, operation="new_body", body_name="Arch " + tag.replace("L", "R"))
cmd("solid.combine", target="Side", tools=["Arch FL", "Arch RL", "Arch FR", "Arch RR"], operation="cut")

# --- Wheel at the origin, axis along Y, then copied into place --------------------------------
sketch("XY", "Tyre section")
tyre = [[232, -105], [300, -118], [325, -100], [340, -70], [342, 0], [340, 70], [325, 100], [300, 118], [232, 105]]
line(tyre + [tyre[0]])
finish()
cmd("solid.revolve", axis="y", operation="new_body", body_name="Tyre")

sketch("XZ", "Rim face")
cmd("sketch.circle.center", center=[0, 0], radius=235)
for k in range(5):
    a = math.radians(72 * k + 36)
    cmd("sketch.circle.center", center=[round(140 * math.cos(a), 2), round(140 * math.sin(a), 2)], radius=52)
cmd("sketch.circle.center", center=[0, 0], radius=40)
finish()
cmd("solid.extrude", distance=24, direction="symmetric", operation="new_body", body_name="Rim")

cmd("solid.cylinder", base=[0, -60, 0], axis="Y", radius=34, height=120, operation="new_body", body_name="Hub cap")

json.dump({"commands": C}, open(sys.argv[1], "w"), indent=1)
