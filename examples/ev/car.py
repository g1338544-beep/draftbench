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
# (the XZ sketch plane's own y axis points down the world Z axis, so heights are drawn negative)
def sz(pts):
    return [[x, -z] for x, z in pts]

sketch("XZ", "Side profile")
spline = lambda pts: cmd("sketch.spline.fit_point", points=pts)
spline(sz([[2250, 520], [1900, 640], [1500, 770], [1100, 900]]))            # bonnet
line(sz([[1100, 900], [420, 1400]]))                                         # windscreen
spline(sz([[420, 1400], [0, 1455], [-700, 1425], [-1050, 1330]]))            # roof
line(sz([[-1050, 1330], [-1850, 1060]]))                                     # rear window
spline(sz([[-1850, 1060], [-2100, 985], [-2250, 860]]))                      # tail
line(sz([[-2250, 860], [-2250, 260]]))
line(sz([[-2250, 260], [2250, 230]]))                                        # floor
line(sz([[2250, 230], [2250, 520]]))                                         # nose
finish()
cmd("solid.extrude", distance=2000, direction="symmetric", operation="new_body", body_name="Body")

sketch("XY", "Top view")
spline([[2260, -300], [1800, -780], [1200, -915], [0, -930], [-1200, -915], [-1900, -850], [-2260, -700]])
line([[-2260, -700], [-2260, 700]])
spline([[-2260, 700], [-1900, 850], [-1200, 915], [0, 930], [1200, 915], [1800, 780], [2260, 300]])
line([[2260, 300], [2260, -300]])
finish()
cmd("solid.extrude", distance=1600, operation="new_body", body_name="Top")
cmd("solid.combine", target="Body", tools=["Top"], operation="intersect")

# --- Wheel arches --------------------------------------------------------------------------------
WHEELS = {"FL": (1400, -800), "FR": (1400, 800), "RL": (-1420, -800), "RR": (-1420, 800)}
for tag, (x, y) in WHEELS.items():
    y0 = -1000 if y < 0 else 500
    cmd("solid.cylinder", base=[x, y0, 345], axis="Y", radius=395, height=500, operation="new_body", body_name="Arch " + tag)
cmd("solid.combine", target="Body", tools=["Arch " + t for t in WHEELS], operation="cut")

# --- Four wheels: tyre (revolved), rim face with five windows, hub cap -------------------------
tyre = [[232, -105], [300, -118], [325, -100], [340, -70], [342, 0], [340, 70], [325, 100], [300, 118], [232, 105]]
for tag, (x, y) in WHEELS.items():
    at = {"origin": [x, y, 345]}
    sketch({"origin": [x, y, 345], "x_dir": [1, 0, 0], "y_dir": [0, 1, 0]}, "Tyre section " + tag)
    line(tyre + [tyre[0]])
    finish()
    cmd("solid.revolve", axis="y", operation="new_body", body_name="Tyre " + tag)
    sketch({"origin": [x, y, 345], "x_dir": [1, 0, 0], "y_dir": [0, 0, 1]}, "Rim face " + tag)
    cmd("sketch.circle.center", center=[0, 0], radius=235)
    for k in range(5):
        a = math.radians(72 * k + 36)
        cmd("sketch.circle.center", center=[round(140 * math.cos(a), 2), round(140 * math.sin(a), 2)], radius=52)
    cmd("sketch.circle.center", center=[0, 0], radius=40)
    finish()
    cmd("solid.extrude", distance=24, direction="symmetric", operation="new_body", body_name="Rim " + tag)
    cmd("solid.cylinder", base=[x, y - 60, 345], axis="Y", radius=34, height=120, operation="new_body", body_name="Hub cap " + tag)

# --- Colours ----------------------------------------------------------------------------------------
cmd("appearance.assign", bodies=["Body"], color="#1c4fa3", pbr={"metallic": 0.6, "roughness": 0.22, "dielectric_ior": 1.5})
cmd("appearance.assign", faces=[[760, 0, 1150], [-1450, 0, 1195]], body="Body", color="#0d1117", pbr={"metallic": 0.1, "roughness": 0.05, "dielectric_ior": 1.5})
for tag in WHEELS:
    cmd("appearance.assign", bodies=["Tyre " + tag], color="#161616", pbr={"metallic": 0.0, "roughness": 0.85, "dielectric_ior": 1.5})
    cmd("appearance.assign", bodies=["Rim " + tag], color="#c9ccd1", pbr={"metallic": 0.9, "roughness": 0.2, "dielectric_ior": 1.5})
    cmd("appearance.assign", bodies=["Hub cap " + tag], color="#8a8d92", pbr={"metallic": 0.8, "roughness": 0.3, "dielectric_ior": 1.5})

import os
limit = int(os.environ.get("LIMIT", "0")) or len(C)
json.dump({"commands": C[:limit]}, open(sys.argv[1], "w"), indent=1)
