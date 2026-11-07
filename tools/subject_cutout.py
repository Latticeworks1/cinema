#!/usr/bin/env python3
"""Lift the foreground subject out of images with the macOS Vision subject-lift, the engine behind
"Copy Subject" in Preview and Photos, and write each result as a transparent PNG.

    tools/subject_cutout.py [--media media] [--prefix cut_] [--min-share 0.03] [--max-share 0.85] PATTERN...

Every file in the media folder matching a pattern gets `<prefix><name>` beside it, cropped to the
subject. The share of the image the subject covers is reported; a result below `--min-share` (no
clear subject) or above `--max-share` (the subject is the whole picture, so cutting it out gains
nothing) is not kept. Images whose cutout exists and is newer than the source are skipped.
Requires macOS 14 or later and PyObjC (`pyobjc-framework-Vision`, `pyobjc-framework-Quartz`).
"""

import argparse
import fnmatch
import sys
import tempfile
from pathlib import Path

import numpy as np
from PIL import Image


def lift(src: Path, out: Path) -> None:
    """Write the subject of `src` to `out` as an RGBA PNG at the size of the source. Raises if none is found."""
    import Quartz
    import Vision
    from Foundation import NSURL

    handler = Vision.VNImageRequestHandler.alloc().initWithURL_options_(NSURL.fileURLWithPath_(str(src)), {})
    request = Vision.VNGenerateForegroundInstanceMaskRequest.alloc().init()
    ok, err = handler.performRequests_error_([request], None)
    if not ok:
        raise RuntimeError(f"vision request failed: {err}")
    results = request.results()
    if not results:
        raise RuntimeError("no subject found")
    obs = results[0]
    buf, err = obs.generateMaskedImageOfInstances_fromRequestHandler_croppedToInstancesExtent_error_(obs.allInstances(), handler, False, None)
    if buf is None:
        raise RuntimeError(f"no masked image: {err}")
    image = Quartz.CIImage.imageWithCVPixelBuffer_(buf)
    cg = Quartz.CIContext.contextWithOptions_(None).createCGImage_fromRect_(image, image.extent())
    dest = Quartz.CGImageDestinationCreateWithURL(NSURL.fileURLWithPath_(str(out)), "public.png", 1, None)
    Quartz.CGImageDestinationAddImage(dest, cg, None)
    if not Quartz.CGImageDestinationFinalize(dest):
        raise RuntimeError("could not write the PNG")


def cut(src: Path, out: Path, min_share: float, max_share: float) -> str:
    with tempfile.TemporaryDirectory() as d:
        tmp = Path(d) / "lift.png"
        try:
            lift(src, tmp)
        except RuntimeError as e:
            return f"none ({e})"
        im = Image.open(tmp).convert("RGBA")
    a = np.asarray(im)[..., 3] > 16
    share = float(a.mean())
    if share < min_share:
        return f"skipped, subject covers {share:.0%}"
    if share > max_share:
        return f"skipped, subject covers {share:.0%} (the whole picture)"
    ys, xs = np.where(a)
    im = im.crop((xs.min(), ys.min(), xs.max() + 1, ys.max() + 1))
    im.save(out)
    return f"{share:.0%} of the picture -> {out.name} {im.size[0]}x{im.size[1]}"


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("patterns", nargs="+")
    ap.add_argument("--media", default="media")
    ap.add_argument("--prefix", default="cut_")
    ap.add_argument("--min-share", type=float, default=0.03)
    ap.add_argument("--max-share", type=float, default=0.85)
    ap.add_argument("--force", action="store_true")
    a = ap.parse_args()
    media = Path(a.media)
    files = [p for p in sorted(media.iterdir()) if not p.name.startswith(a.prefix) and any(fnmatch.fnmatch(p.name, g) for g in a.patterns)]
    if not files:
        sys.exit(f"[!] Error: no file in {media} matches {a.patterns}")
    for p in files:
        out = media / f"{a.prefix}{p.name}"
        if out.exists() and out.stat().st_mtime > p.stat().st_mtime and not a.force:
            print(f"{p.name}: kept {out.name}")
            continue
        out.unlink(missing_ok=True)
        print(f"{p.name}: {cut(p, out, a.min_share, a.max_share)}")


if __name__ == "__main__":
    main()
