#!/usr/bin/env python3
"""Compare measured genuine upstream DClockFace with native clock evidence.
No source fixtures are generated or altered. Dates intentionally differ at the
accessibility minimum, so pixel bounds compare primary-coloured digit ink only.
"""
import json
from pathlib import Path
import sys
from PIL import Image


def bounds(path):
    image = Image.open(path).convert("RGBA")
    points = [(x, y) for y in range(image.height) for x in range(image.width)
              if (lambda c: c[3] >= 128 and c[2] - c[1] >= 30 and c[0] > 60)(image.getpixel((x, y)))]
    assert points, path
    return [min(x for x, _ in points), min(y for _, y in points), max(x for x, _ in points), max(y for _, y in points)]


def main():
    native, source, output = map(Path, sys.argv[1:4])
    actual = {row["case"]: row["metrics"] for row in json.loads((native / "native-metrics.json").read_text())}
    reference = json.loads((source / "source-metrics.json").read_text())
    rows = []
    for mode in ("dark", "light"):
        for scale in (100, 200):
            for width, height in ((96,168),(192,336),(368,336),(256,168),(208,336)):
                name = f"clock-{width}x{height}-{mode}-{scale}"
                expected = next(r for r in reference[f"{mode}-{scale}"] if r["width"] == width)
                measured = actual[name]
                differences = {key: measured[key] - expected[other] for key, other in
                               (("row_advance", "rowAdvance"), ("digit_size", "digitSize"), ("block_top", "blockTop"), ("support_size", "supportSize"), ("ink_ratio", "inkRatio"))}
                assert abs(differences["row_advance"]) < .0005
                # Qt and the native outline parser quantize reference ink
                # differently at opsz84 (76px vs75px at reference size100).
                # Bound/report that divergence and independently check actual
                # painted ink, never hardcode a size-specific correction.
                assert abs(differences["ink_ratio"]) <= .010001
                assert abs(differences["digit_size"]) < 1.2
                assert abs(differences["block_top"]) < .9
                a = bounds(native / f"{name}.png")
                b = bounds(source / f"clock-{width}x{height}-{mode}-1920x1080-{scale}.png")
                assert max(abs(x-y) for x, y in zip(a, b)) <= 2, (name, a, b)
                rows.append({"case": name, "native_minus_source": differences, "native_digit_bounds": a, "source_digit_bounds": b})
    output.write_text(json.dumps(rows, indent=2))
    print(f"PASS 20 actual-source/native clock comparisons: {output}")


if __name__ == "__main__":
    main()
