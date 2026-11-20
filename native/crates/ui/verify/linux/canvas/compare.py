"""The browser canvas against the native redraw: authored pictures, each sent
through a Chromium headless shell (createImageBitmap, paintUpload on white,
toDataURL image/jpeg 0.88, driven over the DevTools protocol by cdp.mjs on
port 9395) and through `mcbench-ui-dev --redraw`; decoded and compared pixel
by pixel. Writes pictures, the page, the shell's profile and the outputs
beside itself: run it from a copy in a scratch folder.

    python3 compare.py <mcbench-ui-dev> <chrome-headless-shell>"""
import base64, io, json, os, subprocess, sys
from PIL import Image
here = os.path.dirname(os.path.abspath(__file__))
pics = os.path.join(here, "pics"); os.makedirs(pics, exist_ok=True)
dev, shell = sys.argv[1], sys.argv[2]
def save(name, img, **kw):
    img.save(os.path.join(pics, name), **kw)
w, h = 4032, 3024
g = Image.new("RGB", (w, h))
px = g.load()
for y in range(h):
    for x in range(w):
        px[x, y] = (x * 255 // (w - 1), y * 255 // (h - 1), (x ^ y) & 255)
save("photo.jpg", g, quality=95)
t = Image.new("RGBA", (600, 400), (0, 0, 0, 0))
tp = t.load()
for y in range(400):
    for x in range(300, 600):
        tp[x, y] = (255, 0, 0, (y * 255) // 399)
save("clear.png", t)
save("small.png", Image.new("RGB", (64, 48), (10, 120, 200)))
r = Image.new("RGB", (400, 200), (200, 30, 30))
rp = r.load()
for y in range(200):
    for x in range(200):
        rp[x, y] = (30, 30, 200)
exif = Image.Exif(); exif[0x0112] = 6
save("turned.jpg", r, quality=95, exif=exif.tobytes())
names = ["photo.jpg", "clear.png", "small.png", "turned.jpg"]
page = os.path.join(here, "page.html")
open(page, "w").write("""<!doctype html><body><pre id=out></pre><script>
const MAX=1568;
function paint(c,b,m){const s=Math.min(1,m/Math.max(b.width,b.height));c.width=Math.round(b.width*s);c.height=Math.round(b.height*s);const x=c.getContext("2d");x.fillStyle="#ffffff";x.fillRect(0,0,c.width,c.height);x.drawImage(b,0,0,c.width,c.height);}
window.done=(async()=>{const out={};for(const n of %s){const blob=await (await fetch("pics/"+n)).blob();const b=await createImageBitmap(blob);const c=document.createElement("canvas");paint(c,b,MAX);out[n]=c.toDataURL("image/jpeg",0.88);}
return JSON.stringify(out);})();
</script></body>""" % json.dumps(names))
env = {"HOME": os.path.join(here, "home"), "XDG_RUNTIME_DIR": os.path.join(here, "rt"), "PATH": "/usr/bin:/bin"}
os.makedirs(env["HOME"], exist_ok=True); os.makedirs(env["XDG_RUNTIME_DIR"], mode=0o700, exist_ok=True)
raw = subprocess.run(["node", os.path.join(here, "cdp.mjs"), shell, page, "9395"], capture_output=True, text=True, timeout=300).stdout
browser = json.loads(json.loads(raw))
rows = []
for n in names:
    b = Image.open(io.BytesIO(base64.b64decode(browser[n].split(",", 1)[1]))).convert("RGB")
    out = os.path.join(here, "native-" + n + ".jpg")
    subprocess.run([dev, "--redraw", os.path.join(pics, n), out], check=True)
    m = Image.open(out).convert("RGB")
    row = {"picture": n, "browser": b.size, "native": m.size}
    if b.size == m.size:
        bd, md = b.tobytes(), m.tobytes()
        diffs = [abs(x - y) for x, y in zip(bd, md)]
        mse = sum(d * d for d in diffs) / len(diffs)
        import math
        row.update(mean_abs=round(sum(diffs) / len(diffs), 3), max_abs=max(diffs), psnr_db=round(10 * math.log10(255 * 255 / mse), 2) if mse else None,
                   share_over_8=round(sum(d > 8 for d in diffs) / len(diffs), 4))
    row["browser_bytes"] = len(base64.b64decode(browser[n].split(",", 1)[1])); row["native_bytes"] = os.path.getsize(out)
    rows.append(row)
    print(json.dumps(row))
