"""Largest empty horizontal band (px) in each screenshot: a displaced row of cards leaves a gap of ~150-200 px, normal gaps stay under ~45.
usage: python3 blank_bands.py shot1.png shot2.png ..."""
import sys
import numpy as np
from PIL import Image

for path in sys.argv[1:]:
    im = np.asarray(Image.open(path).convert("L")).astype(int)
    h, w = im.shape
    empty = im[:, int(w * 0.04):int(w * 0.97)].std(axis=1) < 2.5
    best = run = 0
    for y in range(int(h * 0.08), h):
        run = run + 1 if empty[y] else 0
        best = max(best, run)
    print(path, best)
