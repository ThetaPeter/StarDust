"""Derive stardust.ico, stardust-caption.ico, and web/luna-icon.png from luna-silhouette.png.

Source PNG pixels are not recolored. Near-white / paper is knockout
(transparent). Taskbar/PE ink is white (#FFFFFF). Windows UI caption ink stays
black. No plate, no fill, no rounded square.
"""
import struct
from io import BytesIO
from pathlib import Path

from PIL import Image

NEAR_WHITE = 240
SIZES = (16, 24, 32, 48, 64, 256)

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
PNG_PATH = ROOT / "images" / "luna-silhouette.png"
ICO_PATH = HERE / "stardust.ico"
CAPTION_ICO_PATH = HERE / "stardust-caption.ico"
FAVICON_PATH = ROOT / "crates" / "fs-viz" / "web" / "luna-icon.png"


def knockout_and_whiten(im: Image.Image) -> Image.Image:
    """Paper → transparent; remaining darkness becomes white coverage."""
    im = im.convert("RGBA")
    try:
        import numpy as np

        arr = np.array(im)
        r, g, b, a = arr[:, :, 0], arr[:, :, 1], arr[:, :, 2], arr[:, :, 3]
        paper = (r > NEAR_WHITE) & (g > NEAR_WHITE) & (b > NEAR_WHITE)
        ink = 255 - ((r.astype(np.uint16) + g + b) // 3)
        na = (ink.astype(np.uint16) * a) // 255
        na = np.where(paper, 0, na).astype(np.uint8)
        out = np.empty_like(arr)
        out[:, :, 0] = 255
        out[:, :, 1] = 255
        out[:, :, 2] = 255
        out[:, :, 3] = na
        return Image.fromarray(out)
    except ImportError:
        px = im.load()
        w, h = im.size
        for y in range(h):
            for x in range(w):
                r, g, b, a = px[x, y]
                if r > NEAR_WHITE and g > NEAR_WHITE and b > NEAR_WHITE:
                    px[x, y] = (255, 255, 255, 0)
                else:
                    ink = 255 - ((r + g + b) // 3)
                    px[x, y] = (255, 255, 255, (ink * a) // 255)
        return im


def knockout_keep_black(im: Image.Image) -> Image.Image:
    """Paper → transparent; remaining darkness stays black coverage."""
    im = im.convert("RGBA")
    try:
        import numpy as np

        arr = np.array(im)
        r, g, b, a = arr[:, :, 0], arr[:, :, 1], arr[:, :, 2], arr[:, :, 3]
        paper = (r > NEAR_WHITE) & (g > NEAR_WHITE) & (b > NEAR_WHITE)
        ink = 255 - ((r.astype(np.uint16) + g + b) // 3)
        na = (ink.astype(np.uint16) * a) // 255
        na = np.where(paper, 0, na).astype(np.uint8)
        out = np.empty_like(arr)
        out[:, :, 0] = 0
        out[:, :, 1] = 0
        out[:, :, 2] = 0
        out[:, :, 3] = na
        return Image.fromarray(out)
    except ImportError:
        px = im.load()
        w, h = im.size
        for y in range(h):
            for x in range(w):
                r, g, b, a = px[x, y]
                if r > NEAR_WHITE and g > NEAR_WHITE and b > NEAR_WHITE:
                    px[x, y] = (0, 0, 0, 0)
                else:
                    ink = 255 - ((r + g + b) // 3)
                    px[x, y] = (0, 0, 0, (ink * a) // 255)
        return im


def force_black_rgb(im: Image.Image) -> Image.Image:
    """After LANCZOS, keep alpha; RGB stays #000000."""
    im = im.convert("RGBA")
    try:
        import numpy as np

        arr = np.array(im)
        arr[:, :, 0] = 0
        arr[:, :, 1] = 0
        arr[:, :, 2] = 0
        return Image.fromarray(arr)
    except ImportError:
        px = im.load()
        w, h = im.size
        for y in range(h):
            for x in range(w):
                _, _, _, a = px[x, y]
                px[x, y] = (0, 0, 0, a)
        return im


def force_white_rgb(im: Image.Image) -> Image.Image:
    """After LANCZOS, keep alpha; RGB stays #FFFFFF (no gray fringe fill)."""
    im = im.convert("RGBA")
    try:
        import numpy as np

        arr = np.array(im)
        arr[:, :, 0] = 255
        arr[:, :, 1] = 255
        arr[:, :, 2] = 255
        return Image.fromarray(arr)
    except ImportError:
        px = im.load()
        w, h = im.size
        for y in range(h):
            for x in range(w):
                _, _, _, a = px[x, y]
                px[x, y] = (255, 255, 255, a)
        return im


def sized(art: Image.Image, size: int, black: bool = False) -> Image.Image:
    resized = art.resize((size, size), Image.Resampling.LANCZOS)
    return force_black_rgb(resized) if black else force_white_rgb(resized)


def png_payload(im: Image.Image) -> bytes:
    buf = BytesIO()
    im.save(buf, format="PNG")
    return buf.getvalue()


def write_png_ico(path: Path, images: list) -> None:
    payloads = [png_payload(im) for im in images]
    offset = 6 + 16 * len(images)
    buf = BytesIO()
    buf.write(struct.pack("<HHH", 0, 1, len(images)))
    for im, payload in zip(images, payloads):
        w, h = im.size
        buf.write(
            struct.pack(
                "<BBBBHHII",
                0 if w >= 256 else w,
                0 if h >= 256 else h,
                0,
                0,
                1,
                32,
                len(payload),
                offset,
            )
        )
        offset += len(payload)
    for payload in payloads:
        buf.write(payload)
    path.write_bytes(buf.getvalue())


def buckets(im: Image.Image) -> dict:
    cream = black = white = trans = other = 0
    for r, g, b, a in im.convert("RGBA").getdata():
        if a < 16:
            trans += 1
        elif r < 40 and g < 40 and b < 40:
            black += 1
        elif abs(r - 0xED) < 20 and abs(g - 0xE4) < 20 and abs(b - 0xD4) < 20:
            cream += 1
        elif r > 240 and g > 240 and b > 240:
            white += 1
        else:
            other += 1
    return {
        "cream": cream,
        "black": black,
        "white": white,
        "trans": trans,
        "other": other,
    }


def main() -> None:
    src = knockout_and_whiten(Image.open(PNG_PATH))
    src.save(FAVICON_PATH, format="PNG")
    frames = [sized(src, s) for s in SIZES]
    write_png_ico(ICO_PATH, frames)
    print(f"wrote {ICO_PATH} frames={len(frames)} sizes={list(SIZES)} bytes={ICO_PATH.stat().st_size}")
    print(f"favicon {FAVICON_PATH} bytes={FAVICON_PATH.stat().st_size}")
    for im in frames:
        b = buckets(im)
        print(
            f"{im.size[0]}px buckets cream={b['cream']} black={b['black']} "
            f"white={b['white']} trans={b['trans']} other={b['other']}"
        )
    if frames[-1].size != (256, 256):
        raise SystemExit("expected last frame 256px")
    b256 = buckets(frames[-1])
    if b256["cream"] or b256["black"]:
        raise SystemExit(f"256px still has plate/ink: {b256}")
    if b256["white"] < 1000 or b256["trans"] < 1000:
        raise SystemExit(f"256px not white-on-transparent: {b256}")

    cap_src = knockout_keep_black(Image.open(PNG_PATH))
    cap_frames = [sized(cap_src, s, black=True) for s in SIZES]
    write_png_ico(CAPTION_ICO_PATH, cap_frames)
    print(
        f"wrote {CAPTION_ICO_PATH} frames={len(cap_frames)} "
        f"sizes={list(SIZES)} bytes={CAPTION_ICO_PATH.stat().st_size}"
    )
    cap256 = buckets(cap_frames[-1])
    if cap256["cream"] or cap256["white"]:
        raise SystemExit(f"caption 256px still has plate/white: {cap256}")
    if cap256["black"] < 1000 or cap256["trans"] < 1000:
        raise SystemExit(f"caption 256px not black-on-transparent: {cap256}")


if __name__ == "__main__":
    main()
