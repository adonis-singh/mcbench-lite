# App icon

The icon is the app's own mark, the wire cube with a lit green block that the page's tab and the native window draw, placed on a rounded dark square. It is a placeholder until the owner approves the finished icon: the background's shape on each platform and whether the mark itself changes are both still open.

`render.py` reads the mark from `web/icons/block.svg` (the native window's `native/crates/ui/assets/mark-wire.svg` and `mark-block.svg` hold the same paths). It draws the wire in the file's dark-scheme colour, the lit block with its fills, outline and glow, and the background from `BACKGROUND` and `MARK_BOX` at the top of the script. A line drawn without a fill, which is the wire, is never narrower than one output pixel, so the 16 and 24 pixel icons keep a visible cube. The script uses the Python 3 standard library only, and two runs give identical bytes.

| File | Use |
|---|---|
| `render.py` | Reads the mark and writes every other file in this folder. |
| `mcbench-lite.svg` | The composed icon as vector art: the background, then the mark's own elements (Linux `hicolor/scalable`, the site's favicon). |
| `mcbench-lite-<size>.png` | The icon at 16, 24, 32, 48, 64, 128, 256, 512 and 1024 pixels (Linux `hicolor` sizes, the site). |
| `mcbench-lite.ico` | Windows icon: PNG entries at 16, 24, 32, 48, 64 and 256. |
| `mcbench-lite.icns` | macOS icon: PNG entries icp4 (16), icp5 and ic11 (32), ic12 (64), ic07 (128), ic08 and ic13 (256), ic09 and ic14 (512), ic10 (1024). |

To change the icon, edit `web/icons/block.svg` (the mark), or `BACKGROUND` and `MARK_BOX` in `render.py` (the square behind it and the mark's size on it), then run `python3 native/packaging/icon/render.py` and commit the script and every file it writes. The script reads paths made of absolute M, L, H, V and Z commands, round line caps and joins, `opacity`, and one Gaussian blur filter; it stops with a sentence naming anything else in the file. To use finished artwork from the owner instead, replace `draw` with a function that returns that artwork's RGBA rows at each size and `svg` with one that returns the owner's SVG, then run the script.
