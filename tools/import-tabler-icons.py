#!/usr/bin/env python3
"""Convert Tabler outline icons into GTK 4.20+ stroked symbolic icons.

GTK's classic symbolic loader fills every shape, which turns outline icons
into blobs. GTK's newer SVG renderer understands `gpa:stroke="foreground"`,
so strokes follow the CSS `color` (themes, accent, hover) like any symbolic icon.

usage: import-tabler-icons.py TABLER_OUTLINE_DIR OUT_DIR NAME...
"""
import re
import sys
from pathlib import Path

SHAPES = ("path", "circle", "rect", "line", "polyline", "polygon", "ellipse")
ATTRS = ('class="foreground-stroke transparent-fill" gpa:stroke="foreground" gpa:fill="none" '
         'stroke="rgb(0,0,0)" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"')


def convert(src: str) -> str:
    body = re.search(r"<svg[^>]*>(.*)</svg>", src, re.S).group(1)
    body = re.sub(r'\s*<path stroke="none" d="M0 0h24v24H0z" fill="none"\s*/>', "", body)
    for tag in SHAPES:
        body = re.sub(rf"<{tag}\b", f"<{tag} {ATTRS}", body)
    return ('<svg xmlns="http://www.w3.org/2000/svg" xmlns:gpa="https://www.gtk.org/grappa" '
            'width="24" height="24" viewBox="0 0 24 24" gpa:version="1">'
            + " ".join(body.split()) + "</svg>\n")


def main() -> None:
    src_dir, out_dir, names = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3:]
    out_dir.mkdir(parents=True, exist_ok=True)
    for name in names:
        (out_dir / f"tabler-{name}-symbolic.svg").write_text(convert((src_dir / f"{name}.svg").read_text()))


if __name__ == "__main__":
    main()
