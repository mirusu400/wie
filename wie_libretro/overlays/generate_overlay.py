"""Generate placeholder feature-phone overlay PNG + .cfg.

Portrait layout (480x800):
  - Top  0–360  (45%): transparent game area
  - Bot  360–800 (55%): dark keypad with labelled rectangles

Run: python generate_overlay.py
Outputs: wie_feature_phone.png, wie_feature_phone.cfg

Re-run any time the key layout changes — both files stay in sync via the
KEYS table below (single source of truth for both pixels and RA hitboxes).
"""

from PIL import Image, ImageDraw, ImageFont

IMG_W, IMG_H = 480, 800
BG = (24, 24, 28, 230)          # dark translucent
KEY_BG = (50, 50, 58, 255)
KEY_OUT = (200, 200, 210, 255)
TXT = (235, 235, 240, 255)
RADIUS = 10

# (RA button id, label, pixel-rect x, y, w, h)
KEYS = [
    # nav cluster
    ("up",     "Up",     210,  370, 60, 50),
    ("left",   "Left",   140,  430, 60, 50),
    ("a",      "OK / 5", 210,  430, 60, 50),
    ("right",  "Right",  280,  430, 60, 50),
    ("down",   "Down",   210,  490, 60, 50),
    # soft + call/end row
    ("l",      "SoftL",   20,  370, 70, 50),
    ("start",  "Call",   100,  490, 70, 50),
    ("select", "End",    310,  490, 70, 50),
    ("r",      "SoftR",  390,  370, 70, 50),
    # digit grid
    ("l2",     "1",       60,  560, 70, 50),
    ("y",      "0",      205,  740, 70, 50),  # reuse Y=0 (per MAPPING)
    ("r2",     "3",      350,  560, 70, 50),
    # 2/4/5/6/8 are dual-bound to D-pad/A; show them but route to same buttons:
    ("up",     "2",      205,  560, 70, 50),
    ("left",   "4",       60,  620, 70, 50),
    ("a",      "5",      205,  620, 70, 50),
    ("right",  "6",      350,  620, 70, 50),
    ("l3",     "7",       60,  680, 70, 50),
    ("down",   "8",      205,  680, 70, 50),
    ("r3",     "9",      350,  680, 70, 50),
    ("b",      "* / Clr", 60,  740, 70, 50),
    ("x",      "#",      350,  740, 70, 50),
]


def draw_png():
    img = Image.new("RGBA", (IMG_W, IMG_H), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # keypad backdrop
    d.rectangle([0, 360, IMG_W, IMG_H], fill=BG)
    try:
        font = ImageFont.truetype("arial.ttf", 18)
    except OSError:
        font = ImageFont.load_default()
    for _, label, x, y, w, h in KEYS:
        d.rounded_rectangle([x, y, x + w, y + h], radius=RADIUS, fill=KEY_BG, outline=KEY_OUT, width=2)
        tb = d.textbbox((0, 0), label, font=font)
        tw, th = tb[2] - tb[0], tb[3] - tb[1]
        d.text((x + (w - tw) / 2, y + (h - th) / 2 - 2), label, fill=TXT, font=font)
    img.save("wie_feature_phone.png")


def write_cfg():
    lines = [
        "# WIE feature-phone keypad overlay (portrait).",
        "# Install to <RetroArch>/overlays/wie/, then enable via",
        "# Settings → On-Screen Overlay → Overlay Preset.",
        "",
        "overlays = 1",
        "",
        "overlay0_overlay = wie_feature_phone.png",
        "overlay0_full_screen = true",
        "overlay0_normalized = true",
        "overlay0_range_mod = 1.5",
        "overlay0_alpha_mod = 1.0",
        "",
        f"overlay0_descs = {len(KEYS)}",
        "",
    ]
    for i, (btn, _label, x, y, w, h) in enumerate(KEYS):
        cx = (x + w / 2) / IMG_W
        cy = (y + h / 2) / IMG_H
        rx = (w / 2) / IMG_W
        ry = (h / 2) / IMG_H
        lines.append(f'overlay0_desc{i} = "{btn},{cx:.4f},{cy:.4f},rect,{rx:.4f},{ry:.4f}"')
    with open("wie_feature_phone.cfg", "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    draw_png()
    write_cfg()
    print(f"wrote wie_feature_phone.png ({IMG_W}x{IMG_H}) + .cfg with {len(KEYS)} hitboxes")
