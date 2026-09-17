#!/usr/bin/env python3
"""Derives every image the project ships from the one logo it has.

Run it after `docs/logo.jpg` changes:

    python3 packaging/make-icons.py

It exists so that the numbers below survive. The app icon is a crop of
the logo, not the logo itself: at the sizes an icon is actually seen —
48 px in a task bar, 32 px in a window list — the wordmark "SM2
MODLAUNCHER" is a grey smudge, while the helmet still reads as a shape.
Which part of the logo that crop takes is a decision nobody could
reconstruct from the PNGs afterwards, so it lives here as CROP.

Needs Pillow (`pip install pillow`). Deliberately not wired into
`build.rs` or CI: the inputs change once a year at most, and a build that
silently regenerates committed assets hides the moment they change.
"""

import pathlib

from PIL import Image, ImageDraw, ImageFilter

ROOT = pathlib.Path(__file__).resolve().parent.parent
LOGO = ROOT / "docs" / "logo.jpg"

# The helmet, with the surrounding texture it sits on. Taken as one piece
# out of the original rather than pasted onto a background patch: two
# crops of the same photograph still differ in brightness, and the seam
# between them is visible at 256 px.
CROP = (288, 238, 480, 430)

# The corner radius, as a share of the edge. 22 % is what the icon this
# one replaces used (`rx="14"` on a 64 px tile), so the silhouette in the
# task bar stays the one people already have.
RADIUS_SHARE = 0.22

# 48 upwards is what a desktop actually looks for; everything smaller a
# toolkit scales down from 48 itself, and scaling down a photograph is
# the one thing it does well.
ICON_SIZES = (48, 64, 128, 256)

# What Windows picks from: 16 and 32 for lists, 48 for the desktop, 256
# for the large view in Explorer.
ICO_SIZES = ((16, 16), (32, 32), (48, 48), (256, 256))

# GitHub renders the social preview at 1280x640 and crops anything else.
SOCIAL_SIZE = (1280, 640)


def rounded(image):
    """Cuts the corners off a square image, alpha channel and all."""
    mask = Image.new("L", image.size, 0)
    radius = round(image.size[0] * RADIUS_SHARE)
    ImageDraw.Draw(mask).rounded_rectangle(
        (0, 0, image.size[0] - 1, image.size[1] - 1), radius=radius, fill=255
    )
    out = image.convert("RGBA")
    out.putalpha(mask)
    return out


def write_icons(logo):
    """The app icon, in the sizes `install.sh` puts into hicolor."""
    icons = ROOT / "packaging" / "icons"
    icons.mkdir(exist_ok=True)
    source = logo.crop(CROP)
    for size in ICON_SIZES:
        icon = rounded(source.resize((size, size), Image.LANCZOS))
        icon.save(icons / f"lina-sm2-{size}.png")
    # The window icon is read back at runtime by `gui::run` through
    # `eframe::icon_data::from_png_bytes`, so it is one file, not a set.
    rounded(source.resize((256, 256), Image.LANCZOS)).save(
        ROOT / "packaging" / "lina-sm2-window.png"
    )
    # A single .ico holding every size Windows asks for. Written from the
    # largest one; Pillow downsamples the rest itself.
    rounded(source.resize((256, 256), Image.LANCZOS)).save(
        ROOT / "packaging" / "lina-sm2.ico", sizes=ICO_SIZES
    )


def write_social_preview(logo):
    """The image GitHub shows when someone shares a link to the repo.

    The logo is square and the preview is not, so the empty half has to
    be filled with something. A blurred, darkened copy of the logo itself
    is the one filling that cannot clash with it.
    """
    width, height = SOCIAL_SIZE
    scale = width / logo.size[0]
    backdrop = logo.resize((width, round(logo.size[1] * scale)), Image.LANCZOS)
    top = (backdrop.size[1] - height) // 2
    backdrop = backdrop.crop((0, top, width, top + height))
    backdrop = backdrop.filter(ImageFilter.GaussianBlur(24))
    backdrop = Image.blend(backdrop, Image.new("RGB", SOCIAL_SIZE, (0, 0, 0)), 0.55)

    edge = round(height * 0.86)
    front = rounded(logo.resize((edge, edge), Image.LANCZOS))
    backdrop.paste(front, ((width - edge) // 2, (height - edge) // 2), front)
    backdrop.save(ROOT / "docs" / "social-preview.png")


def main():
    logo = Image.open(LOGO).convert("RGB")
    write_icons(logo)
    write_social_preview(logo)
    print(f"written from {LOGO.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
