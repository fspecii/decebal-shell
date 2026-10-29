#!/usr/bin/env python3
"""Sample the glass client's actual pixels while moving an underlying window.

Invoked by glass.sh on its private display. No synthetic pointer movement.
"""
import ctypes as C
import hashlib
import math
import sys
import time

x = C.CDLL('libX11.so.6')
P, U = C.c_void_p, C.c_ulong
x.XOpenDisplay.argtypes = [C.c_char_p]
x.XOpenDisplay.restype = P
x.XMoveWindow.argtypes = [P, U, C.c_int, C.c_int]
x.XFlush.argtypes = [P]
x.XSetWindowBackground.argtypes = [P, U, U]
x.XClearWindow.argtypes = [P, U]
x.XCloseDisplay.argtypes = [P]
class Image(C.Structure):
    _fields_ = [('width', C.c_int), ('height', C.c_int), ('xoffset', C.c_int),
                ('format', C.c_int), ('data', P), ('byte_order', C.c_int),
                ('bitmap_unit', C.c_int), ('bitmap_bit_order', C.c_int),
                ('bitmap_pad', C.c_int), ('depth', C.c_int),
                ('bytes_per_line', C.c_int), ('bits_per_pixel', C.c_int)]
x.XGetImage.argtypes = [P, U, C.c_int, C.c_int, C.c_uint, C.c_uint, U, C.c_int]
x.XGetImage.restype = C.POINTER(Image)
x.XDestroyImage.argtypes = [C.POINTER(Image)]
d = x.XOpenDisplay(None)
bar, window = map(lambda n: int(n, 0), sys.argv[1:3])
changes = []
last = None
start = time.monotonic()
try:
    while time.monotonic() - start < 2:
        now = time.monotonic() - start
        x.XMoveWindow(d, window, 400 + int(300 * math.sin(now * 3)), 0)
        x.XFlush(d)
        image = x.XGetImage(d, bar, 0, 0, 1280, 28, U(-1).value, 2)
        assert image, 'glass client readback failed'
        digest = hashlib.sha256(C.string_at(image.contents.data, image.contents.bytes_per_line * 28)).digest()
        x.XDestroyImage(image)
        if digest != last:
            changes.append(time.monotonic() - start)
            last = digest
        time.sleep(.01)
    # Repainting a stationary window must also refresh glass (XDamage), without
    # relying on geometry events or the GUI's periodic fallback timer.
    time.sleep(.1)
    content = set()
    for i in range(12):
        x.XSetWindowBackground(d, window, (i * 19 << 16) | ((255-i*17) << 8) | (i*13))
        x.XClearWindow(d, window)
        x.XFlush(d)
        time.sleep(.05)
        image = x.XGetImage(d, bar, 0, 0, 1280, 28, U(-1).value, 2)
        assert image
        content.add(hashlib.sha256(C.string_at(image.contents.data, image.contents.bytes_per_line * 28)).digest())
        x.XDestroyImage(image)
    print(f'  stationary backdrop repaints: {len(content)} distinct glass frames')
    assert len(content) >= 8, 'stationary window content changes did not wake native glass'
finally:
    x.XCloseDisplay(d)
active = [t for t in changes if .3 <= t <= 1.8]
print(f'  moving backdrop: {len(active)} distinct native glass frames in 1.5s')
assert len(active) >= 15, f'glass still updates at the old slow cadence: {len(active)} frames'
assert max((b-a for a, b in zip(active, active[1:])), default=1) < .2, 'glass stalled during backdrop motion'
