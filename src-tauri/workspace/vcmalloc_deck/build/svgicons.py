"""Convert the icon library into reusable inline SVG.

Rather than importing icons.py (which pulls in python-pptx drawing helpers),
this module re-declares the pure geometry so it can run anywhere.
Geometry is identical to icons.py by construction.
"""
import math

LINE = {}
SOLID = {}
COMPOUND = {}


def _box(x, y, w, h):
    return [(x, y), (x + w, y), (x + w, y + h), (x, y + h)]


def _circle(cx, cy, r, n=44):
    return [(cx + r * math.cos(2 * math.pi * i / n),
             cy + r * math.sin(2 * math.pi * i / n)) for i in range(n)]


def _ring(cx, cy, r_out, r_in, n=48):
    out = []
    for i in range(n):
        a = 2 * math.pi * i / n
        out.append((cx + r_out * math.cos(a), cy + r_out * math.sin(a)))
    for i in range(n - 1, -1, -1):
        a = 2 * math.pi * i / n
        out.append((cx + r_in * math.cos(a), cy + r_in * math.sin(a)))
    return out


def _arc_ring(cx, cy, r_out, r_in, a0, a1, n=22):
    pts = []
    for i in range(n + 1):
        a = math.radians(a0 + (a1 - a0) * i / n)
        pts.append((cx + r_out * math.cos(a), cy + r_out * math.sin(a)))
    for i in range(n, -1, -1):
        a = math.radians(a0 + (a1 - a0) * i / n)
        pts.append((cx + r_in * math.cos(a), cy + r_in * math.sin(a)))
    return pts


def _eye_lens(cx, cy, rx, ry, n=30):
    top = [(cx + rx * math.cos(math.radians(200 + 140 * i / n)),
            cy + ry * math.sin(math.radians(200 + 140 * i / n)))
           for i in range(n + 1)]
    bot = [(cx + rx * math.cos(math.radians(20 - 140 * i / n)),
            cy + ry * math.sin(math.radians(20 - 140 * i / n)))
           for i in range(n + 1)]
    return top + bot


def _gear(cx, cy, r_out, r_in, teeth=8):
    pts = []
    step = 2 * math.pi / teeth
    for t in range(teeth):
        a0 = t * step
        pts += [
            (cx + r_out * math.cos(a0 + step * 0.10),
             cy + r_out * math.sin(a0 + step * 0.10)),
            (cx + r_out * math.cos(a0 + step * 0.34),
             cy + r_out * math.sin(a0 + step * 0.34)),
            (cx + r_in * math.cos(a0 + step * 0.40),
             cy + r_in * math.sin(a0 + step * 0.40)),
            (cx + r_in * math.cos(a0 + step * 0.60),
             cy + r_in * math.sin(a0 + step * 0.60)),
        ]
    return pts


def _curve_points(x0, x1, ytop, ybot, power=0.45, n=30):
    return [(x0 + (x1 - x0) * (i / n),
             ytop + (ybot - ytop) * ((i / n) ** power)) for i in range(n + 1)]


def _arc_stroke(cx, cy, r, a0, a1, w, n=18):
    outer, inner = [], []
    for i in range(n + 1):
        a = math.radians(a0 + (a1 - a0) * i / n)
        outer.append((cx + (r + w / 2) * math.cos(a), cy + (r + w / 2) * math.sin(a)))
    for i in range(n, -1, -1):
        a = math.radians(a0 + (a1 - a0) * i / n)
        inner.append((cx + (r - w / 2) * math.cos(a), cy + (r - w / 2) * math.sin(a)))
    return outer + inner


def _heat_grid(n, m, cell, x0, y0):
    out = []
    for r in range(m):
        for c in range(n):
            x, y = x0 + c * cell, y0 + r * cell
            out.append([(x, y), (x + cell * 0.88, y),
                        (x + cell * 0.88, y + cell * 0.88),
                        (x, y + cell * 0.88)])
    return out


def _scatter_holes(seed=7):
    import random
    rnd = random.Random(seed)
    return [_box(5 + c * 15, 8 + r * 17, 12, 14)
            for r in range(5) for c in range(6) if rnd.random() < 0.52]


# =====================================================================
#  ICON TABLE
# =====================================================================
SOLID.update({
    'database': [_box(20, 14, 60, 12), _box(20, 38, 60, 12),
                 _box(20, 62, 60, 12), _ring(50, 26, 30, 20)],
    'layers': [[(50, 8), (94, 32), (50, 56), (6, 32)],
               [(6, 50), (50, 74), (94, 50), (94, 62), (50, 86), (6, 62)]],
    'blocks': [_box(6, 6, 40, 40), _box(54, 6, 40, 26), _box(54, 40, 40, 12),
               _box(6, 54, 24, 40), _box(38, 60, 26, 34), _box(70, 60, 24, 34)],
    'cpu': [_box(24, 24, 52, 52), _box(36, 36, 28, 28)],
    'bolt': [(56, 4), (22, 56), (46, 56), (38, 96), (78, 42), (52, 42)],
    'shield': [(50, 6), (88, 22), (88, 52), (50, 94), (12, 52), (12, 22)],
    'shield_check': [(50, 6), (88, 22), (88, 52), (50, 94), (12, 52), (12, 22),
                     (32, 48), (44, 62), (70, 34), (78, 42), (44, 78), (24, 56)],
    'target': [_ring(50, 50, 44, 30), _ring(50, 50, 22, 12), _circle(50, 50, 9)],
    'trophy': [(28, 8), (72, 8), (70, 40), (58, 52), (58, 72), (66, 84), (34, 84),
               (42, 72), (42, 52), (30, 40), (18, 14), (28, 14), (28, 30),
               (22, 38), (18, 30), (72, 14), (82, 14), (82, 30), (78, 38), (72, 30)],
    'scale': [_box(46, 6, 8, 82), _box(16, 22, 68, 8), _box(28, 82, 44, 10),
              _box(6, 46, 20, 8), _box(74, 46, 20, 8)],
    'filter_funnel': [(8, 12), (92, 12), (60, 50), (60, 90), (40, 78), (40, 50)],
    'clock': [_ring(50, 50, 44, 34), _circle(50, 50, 6)],
    'flag': [_box(14, 8, 10, 84), (24, 14), (88, 14), (74, 32), (88, 50), (24, 50)],
    'book': [(10, 12), (48, 12), (50, 22), (50, 90), (48, 82), (10, 82),
             (90, 12), (52, 12), (50, 22), (50, 90), (52, 82), (90, 82)],
    'code': [(30, 18), (6, 50), (30, 82), (42, 70), (26, 50), (42, 30),
             (70, 18), (94, 50), (70, 82), (58, 70), (74, 50), (58, 30)],
    'terminal': [_box(8, 16, 84, 68)],
    'gear': [_gear(50, 50, 46, 32, 8), _circle(50, 50, 13)],
    'warning': [[(50, 8), (96, 88), (4, 88)], _box(45, 36, 10, 30),
                _box(45, 72, 10, 9)],

    'info': [_ring(50, 50, 44, 34), _circle(50, 32, 6), _box(45, 44, 10, 30)],
    'question': [_ring(50, 50, 44, 34), (38, 38), (44, 30), (56, 30), (63, 38),
                 (62, 48), (50, 56), (50, 64), _box(45, 72, 10, 9)],
    'lightbulb': [_ring(50, 40, 27, 18), (40, 60), (60, 60), (56, 72), (44, 72),
                  _box(41, 76, 18, 8), _box(43, 88, 14, 6)],
    'search': [_ring(45, 44, 30, 21), (64, 64), (90, 90), (78, 92), (62, 78)],
    'rocket': [[(50, 4), (66, 26), (66, 58), (50, 72), (34, 58), (34, 26)],
               [(34, 40), (14, 56), (14, 72), (34, 62)],
               [(66, 40), (86, 56), (86, 72), (66, 62)],
               _ring(50, 32, 11, 6),
               [(42, 72), (50, 98), (58, 72)]],

    'globe': [_ring(50, 50, 44, 34), _box(6, 46, 88, 8), (22, 20), (78, 20),
              (78, 34), (22, 34), (22, 66), (78, 66), (78, 80), (22, 80),
              _ring(50, 50, 20, 14)],
    'link': [(38, 62), (22, 78), (8, 64), (22, 50), (34, 50), (46, 62), (38, 70),
             (26, 58), (22, 62), (30, 70),
             (62, 38), (78, 22), (92, 36), (78, 50), (66, 50), (54, 38), (62, 30),
             (74, 42), (78, 38), (70, 30)],
    'graph_up': [_box(8, 76, 14, 0), _box(8, 52, 14, 24), _box(22, 52, 0, 0),
                 _box(36, 76, 14, 42), _box(50, 34, 14, 42),
                 _box(64, 76, 14, 58), _box(78, 18, 14, 58), _box(4, 80, 92, 10)],
    'stack_money': [_ring(50, 50, 44, 34), _box(50, 34, 6, 34)],
    'puzzle': [(10, 10), (40, 10), (40, 20), (46, 12), (56, 20), (46, 28),
               (56, 36), (56, 44), (90, 44), (90, 90), (56, 90), (56, 78),
               (44, 78), (44, 90), (10, 90)],
    'map_pin': [[(50, 4), (78, 28), (78, 54), (50, 96), (22, 54), (22, 28)],
                _ring(50, 38, 15, 9)],

    'clipboard': [_box(12, 18, 76, 78), _box(34, 4, 32, 16), _box(40, 10, 20, 6)],
    'document': [(16, 4), (60, 4), (86, 30), (86, 96), (16, 96),
                 (60, 4), (60, 30), (86, 30)],
    'people': [_ring(30, 30, 19, 0), (4, 84), (4, 68), (56, 68), (56, 84),
               _ring(70, 30, 19, 0), (44, 84), (44, 68), (96, 68), (96, 84)],
    'grid': [_box(8, 8, 36, 36), _box(56, 8, 36, 36), _box(8, 56, 36, 36),
             _box(56, 56, 36, 36)],
    'window': [_box(8, 16, 84, 68), _box(8, 16, 84, 14), _circle(18, 23, 4),
               _circle(28, 23, 4), _circle(38, 23, 4)],
    'gauge': [_arc_ring(50, 60, 44, 33, 180, 360),
              (48, 56), (82, 28), (90, 40), (56, 68)],
    'arrow_right': [(12, 38), (64, 38), (64, 20), (96, 50), (64, 80), (64, 62), (12, 62)],
    'arrow_down': [(38, 12), (62, 12), (62, 64), (80, 64), (50, 96), (20, 64), (38, 64)],
    'arrow_up': [(38, 88), (62, 88), (62, 36), (80, 36), (50, 4), (20, 36), (38, 36)],
    'arrow_swap': [(6, 26), (72, 26), (72, 8), (98, 32), (72, 56), (72, 42), (6, 42),
                   (94, 74), (28, 74), (28, 92), (2, 68), (28, 44), (28, 58), (94, 58)],
    'arrow_turn': [(10, 8), (26, 8), (26, 50), (72, 50), (72, 38), (96, 60),
                   (72, 82), (72, 70), (10, 70)],
    'trash': [[(10, 22), (90, 22), (84, 96), (16, 96)], _box(30, 8, 40, 14),
              _box(40, 36, 6, 44), _box(56, 36, 6, 44)],

    'plus': [_box(44, 10, 12, 80), _box(10, 44, 80, 12)],
    'minus': [_box(10, 44, 80, 12)],
    'lock': [_box(14, 44, 72, 52), _arc_ring(50, 46, 25, 14, 180, 360),
             _ring(50, 68, 9, 5)],
    'unlock': [_box(14, 44, 72, 52), (50, 46), (26, 30), (36, 22), (58, 36),
               _ring(50, 68, 9, 5)],
    'memory': [_box(6, 26, 88, 48), _box(18, 36, 16, 28), _box(42, 36, 16, 28),
               _box(66, 36, 16, 28), _box(20, 8, 8, 18), _box(44, 8, 8, 18),
               _box(68, 8, 8, 18), _box(20, 74, 8, 18), _box(44, 74, 8, 18),
               _box(68, 74, 8, 18)],
    'paging': [[(4, 24), (30, 12), (58, 12), (84, 24), (96, 36), (4, 36)],
               _box(10, 44, 24, 42), _box(40, 44, 24, 42), _box(70, 44, 24, 42)],

    'compression': [_box(6, 10, 88, 18), _box(6, 41, 88, 18), _box(6, 72, 88, 18),
                    (38, 30), (62, 30), (50, 20), (38, 70), (62, 70), (50, 80)],
    'split': [(50, 6), (56, 18), (56, 48), (10, 48), (10, 92), (22, 92), (22, 60),
              (78, 60), (78, 92), (90, 92), (90, 48), (44, 48), (44, 18)],
    'merge': [(10, 8), (22, 8), (22, 40), (78, 40), (78, 8), (90, 8), (90, 56),
              (78, 68), (78, 56), (22, 56), (22, 68), (10, 56)],
    'check_circle': [_ring(50, 50, 46, 36),
                     (32, 50), (44, 63), (70, 35), (78, 43), (44, 78), (24, 57)],
    'x_circle': [_ring(50, 50, 46, 36), (32, 32), (42, 22), (50, 32), (58, 22),
                 (68, 32), (58, 42), (68, 52), (68, 68), (58, 68), (50, 58),
                 (42, 68), (32, 68), (32, 52), (42, 42)],
    'ban': [_ring(50, 50, 46, 36), (28, 72), (72, 28), (66, 22), (22, 66)],
    'eye': [_eye_lens(50, 50, 46, 26), _ring(50, 50, 15, 9)],
    'wrench': [(92, 8), (76, 24), (78, 40), (92, 32), (98, 44), (84, 52), (68, 44),
               (30, 86), (12, 68), (50, 26), (60, 34), (50, 44), (66, 52), (74, 38),
               (68, 28), (84, 12)],
    'key': [_ring(30, 34, 22, 12), _box(42, 46, 8, 46), _box(50, 62, 22, 8),
            _box(50, 80, 16, 8)],
    'magnet': [_arc_ring(50, 52, 34, 20, 180, 360), _box(16, 52, 14, 42),
               _box(70, 52, 14, 42)],
    'tree': [[(50, 6), (76, 40), (62, 40), (82, 70), (18, 70), (38, 40),
              (24, 40)], _box(44, 70, 12, 26)],

    'flow': [_box(6, 12, 26, 26), _box(68, 12, 26, 26), _box(37, 62, 26, 26),
             (20, 38), (20, 50), (80, 50), (80, 38), (74, 44), (26, 44),
             (50, 40), (50, 62), (44, 56), (56, 56)],
    'list': [_box(8, 16, 14, 12), _box(32, 16, 60, 12), _box(8, 44, 14, 12),
             _box(32, 44, 60, 12), _box(8, 72, 14, 12), _box(32, 72, 60, 12)],
    'quote': [(10, 70), (10, 40), (40, 40), (40, 70), (24, 70), (24, 56), (32, 56),
              (32, 46), (18, 46), (18, 56), (26, 56), (26, 70),
              (56, 70), (56, 40), (86, 40), (86, 70), (70, 70), (70, 56), (78, 56),
              (78, 46), (64, 46), (64, 56), (72, 56), (72, 70)],
    'microscope': [[(60, 8), (76, 20), (48, 60), (34, 50)], _box(30, 56, 40, 10),
                   _box(20, 66, 60, 12), _box(44, 78, 12, 18), _box(28, 92, 44, 8)],

    'beaker': [(34, 8), (66, 8), (66, 30), (86, 84), (86, 92), (14, 92), (14, 84),
               (34, 30), (22, 64), (78, 64), (86, 84), (86, 92), (14, 92), (14, 84)],
    'compass': [_ring(50, 50, 46, 38), (64, 36), (54, 56), (36, 64), (46, 44)],
    'scale_balance': [_box(46, 8, 8, 80), _box(20, 22, 60, 8), _box(28, 84, 44, 10),
                      _box(4, 44, 22, 8), _box(74, 44, 22, 8),
                      _ring(15, 62, 14, 8), _ring(85, 62, 14, 8)],
    'hourglass': [(18, 6), (82, 6), (82, 18), (58, 50), (82, 82), (82, 94), (18, 94),
                  (18, 82), (42, 50), (18, 18), (50, 56), (62, 86), (38, 86)],
    'network': [_circle(50, 16, 13), _circle(14, 80, 13), _circle(86, 80, 13),
                _box(44, 27, 12, 40), _box(20, 46, 60, 8), _box(20, 44, 8, 12),
                _box(72, 44, 8, 12)],
    'stack_layers_up': [(50, 6), (92, 28), (50, 50), (8, 28),
                        (8, 48), (50, 70), (92, 48), (92, 60), (50, 82), (8, 60),
                        (8, 70), (50, 92), (92, 70), (92, 78), (50, 100), (8, 78)],
    'contour': _curve_points(10, 62, 88, 30) + [(62, 88), (10, 88)]
               + [(72, 66), (88, 66), (88, 88), (72, 88)]
               + [(72, 20), (88, 20), (88, 38), (72, 38)]
               + [(2, 88), (96, 88), (96, 96), (2, 96)],
    'matrix': [p for row in _heat_grid(4, 4, 22, 6, 6) for p in row],
    'frames': [_box(10, 4 + i * 16, 80, 11) for i in range(3)]
              + [(24, 58), (76, 58), (86, 80), (14, 80), (34, 70), (66, 70), (66, 78), (34, 78)],
    'cache': [_box(6, 20, 22, 18), _box(34, 20, 22, 18), _box(62, 20, 22, 18),
              _box(6, 46, 30, 18), _box(42, 46, 22, 18), _box(70, 46, 22, 18),
              _box(6, 72, 38, 18), _box(50, 72, 18, 18), _box(74, 72, 20, 18)],
    'bar_chart': [_box(10, 56, 20, 40), _box(40, 26, 20, 70), _box(70, 42, 20, 54)],
    'scatter': [_circle(20, 30, 9), _circle(46, 20, 6), _circle(74, 34, 10),
                _circle(28, 62, 7), _circle(56, 56, 9), _circle(82, 72, 6),
                _circle(44, 84, 5)],
    'compare': [_box(6, 18, 14, 14), _box(28, 18, 62, 14), _box(6, 46, 14, 14),
                _box(28, 46, 62, 14), _box(6, 74, 14, 14), _box(28, 74, 62, 14),
                _box(20, 44, 8, 48), _box(84, 44, 8, 48)],
    'threads': [_ring(50, 50, 42, 40), _ring(50, 50, 27, 40), _circle(50, 50, 9)],
    'power': [_arc_ring(50, 56, 46, 32, 48, 132), (46, 4), (58, 4), (58, 50), (46, 50)],
    'dependency': [_ring(20, 22, 14, 7), _ring(80, 22, 14, 7), _ring(50, 82, 14, 7),
                   _box(30, 18, 40, 8), _box(14, 32, 8, 36), _box(78, 32, 8, 36),
                   (50, 34), (58, 44), (42, 44), (44, 62), (50, 72), (56, 62)],
    'pointer': [(10, 6), (10, 74), (30, 58), (42, 84), (56, 78), (44, 54), (70, 50)],
    'loop': [_arc_stroke(50, 50, 32, 250, 340, 14), _arc_stroke(50, 50, 32, 70, 160, 14),
             (64, 16), (88, 8), (84, 32), (12, 84), (36, 92), (40, 68)],
    'ledger': [_box(10, 8, 80, 84), _box(10, 8, 22, 84), _box(38, 24, 44, 8),
               _box(38, 42, 44, 8), _box(38, 60, 44, 8), _box(38, 78, 44, 8)],
    'app_window': [_box(6, 14, 88, 74), _box(6, 14, 88, 16), _circle(16, 22, 4),
                   _circle(26, 22, 4), _circle(36, 22, 4), _box(18, 40, 22, 14),
                   _box(46, 40, 22, 14), _box(18, 60, 50, 10)],
    'lab': [[(38, 6), (62, 6), (62, 26), (86, 82), (86, 94), (14, 94),
             (14, 82), (38, 26)],
            _box(20, 66, 60, 28), _circle(36, 76, 6), _circle(54, 84, 5),
            _circle(66, 70, 4)],

    'stopwatch': [_ring(50, 56, 40, 30), _box(38, 4, 24, 10), _box(46, 14, 8, 8),
                  _box(20, 14, 18, 8), _box(62, 14, 18, 8)],
    'buckets': [_box(6, 8, 26, 36), _box(38, 8, 26, 36), _box(70, 8, 24, 36),
                _box(6, 50, 26, 42), _box(38, 50, 26, 42), _box(70, 50, 24, 42)],
    'tradeoff': [(6, 22), (44, 22), (44, 34), (25, 42), (6, 34),
                 (6, 58), (44, 58), (44, 70), (25, 78), (6, 70),
                 (56, 18), (94, 18), (94, 26), (56, 26),
                 (56, 44), (86, 44), (86, 52), (56, 52),
                 (56, 70), (78, 70), (78, 78), (56, 78)],
    'checklist': [_box(6, 10, 22, 22), _box(38, 16, 56, 10), _box(6, 42, 22, 22),
                  _box(38, 48, 56, 10), _box(6, 74, 22, 22), _box(38, 80, 56, 10)],
    'reserve': [_box(4, 10, 24, 30), _box(4, 10, 4, 4), _box(28, 10, 4, 4),
                _box(4, 44, 24, 30), _box(4, 44, 4, 4), _box(28, 44, 4, 4),
                _box(40, 10, 24, 30), _box(40, 10, 4, 4), _box(60, 10, 4, 4),
                _box(40, 44, 24, 30), _box(40, 44, 4, 4), _box(60, 44, 4, 4),
                _box(72, 10, 24, 30), _box(72, 10, 4, 4), _box(92, 10, 4, 4),
                _box(72, 44, 24, 30), _box(72, 44, 4, 4), _box(92, 44, 4, 4)],
    'roadmap': [_circle(16, 22, 12), _circle(50, 50, 12), _circle(84, 22, 12),
                _box(24, 18, 22, 8), _box(60, 26, 20, 20), _box(60, 42, 8, 8)],
    'os_stack': [_box(6, 8, 88, 20), _box(6, 34, 88, 20), _box(6, 60, 88, 20),
                 _box(6, 86, 88, 10)],
    'dispersed': _scatter_holes(),
    'contiguous': [_box(4, 18, 92, 14), _box(4, 38, 92, 14), _box(4, 58, 92, 14)],
    'physical_frames': [_box(4, 10, 30, 18), _box(70, 10, 26, 18),
                        _box(4, 40, 22, 18), _box(52, 40, 44, 18),
                        _box(30, 70, 34, 18), _box(72, 70, 24, 18)],
    'zone': [[(4, 34), (22, 12), (78, 12), (96, 34), (78, 56), (22, 56)],
             _box(22, 66, 56, 12), _box(14, 84, 72, 10)],

    'restrict': [_box(4, 10, 92, 14), _box(4, 32, 92, 14), _box(4, 54, 92, 14),
                 _box(28, 76, 44, 8)],
    'integrate': [[(4, 40), (30, 26), (48, 40), (30, 54)],
                  [(52, 40), (70, 26), (96, 40), (70, 54)],
                  _box(30, 60, 40, 10), _box(30, 78, 40, 8)],

    'measure': [_box(6, 22, 88, 56), _box(6, 22, 88, 12), _box(16, 44, 10, 26),
                _box(34, 36, 10, 34), _box(52, 48, 10, 22), _box(70, 30, 10, 40)],
    'compact': [(4, 16), (24, 16), (24, 84), (4, 84), (30, 30), (46, 30), (46, 70),
                (30, 70), (52, 40), (68, 40), (68, 60), (52, 60),
                (74, 48), (94, 48), (94, 52), (74, 52)],
    'expand': [_box(8, 44, 40, 12), _box(24, 28, 12, 44),
               (58, 12), (94, 12), (94, 48), (58, 52), (94, 52), (94, 88),
               (58, 12), (58, 48), (94, 48)],
    'thread_risk': [_ring(50, 50, 46, 36), _box(20, 48, 60, 6),
                    _box(44, 48, 12, 30), _box(32, 72, 36, 10)],
    'pinned': [_box(20, 26, 60, 48), _box(30, 12, 40, 14), _box(30, 74, 40, 14),
               _box(8, 40, 12, 20), _box(80, 40, 12, 20)],
    'package': [(50, 4), (94, 24), (94, 76), (50, 96), (6, 76), (6, 24),
                (6, 24), (50, 44), (94, 24), (50, 44), (50, 96)],
    'repo': [_circle(20, 20, 10), _circle(80, 20, 10), _circle(50, 84, 10),
             _box(20, 26, 8, 34), _box(72, 26, 8, 34), _box(26, 16, 48, 8),
             (46, 34), (56, 34), (56, 78), (46, 78)],
    'verified': [_ring(50, 50, 46, 36),
                 (30, 50), (42, 63), (70, 33), (79, 41), (42, 79), (21, 57)],
    'page': [(18, 4), (58, 4), (82, 28), (82, 96), (18, 96),
             (58, 4), (58, 28), (82, 28)],
    'converge': [(4, 10), (22, 10), (60, 46), (60, 66), (40, 66), (40, 50),
                 (96, 10), (78, 10), (40, 46), (40, 66), (60, 66), (60, 46)],
    'gpu_card': [_box(8, 22, 84, 50), _box(8, 34, 84, 10), _box(16, 30, 8, 8),
                 _box(30, 30, 8, 8), _box(44, 30, 8, 8), _box(16, 76, 12, 8),
                 _box(34, 76, 12, 8), _box(52, 76, 12, 8), _ring(50, 56, 13, 7)],
})

LINE.update({
    'cpu_pins': dict(lw=7, parts=[
        ([(36, 8), (36, 24)], False), ([(50, 8), (50, 24)], False),
        ([(64, 8), (64, 24)], False), ([(36, 76), (36, 92)], False),
        ([(50, 76), (50, 92)], False), ([(64, 76), (64, 92)], False),
        ([(8, 36), (24, 36)], False), ([(8, 50), (24, 50)], False),
        ([(8, 64), (24, 64)], False), ([(76, 36), (92, 36)], False),
        ([(76, 50), (92, 50)], False), ([(76, 64), (92, 64)], False)]),
    'clock_hands': dict(lw=8, parts=[([(50, 26), (50, 52), (72, 62)], False)]),
    'terminal_marks': dict(lw=7, parts=[
        ([(26, 38), (44, 50), (26, 62)], False), ([(52, 64), (76, 64)], False)]),
    'document_lines': dict(lw=6, parts=[
        ([(30, 46), (70, 46)], False), ([(30, 60), (70, 60)], False),
        ([(30, 74), (56, 74)], False)]),
    'gauge_ticks': dict(lw=7, parts=[
        ([(50, 58), (50, 24)], False), ([(24, 40), (32, 47)], False),
        ([(76, 40), (68, 47)], False)]),
    'stopwatch_hands': dict(lw=7, parts=[
        ([(50, 56), (50, 30)], False), ([(50, 56), (70, 62)], False)]),
    'checklist_ticks': dict(lw=6, parts=[
        ([(11, 20), (17, 27), (25, 13)], False),
        ([(11, 52), (17, 59), (25, 45)], False),
        ([(11, 84), (17, 91), (25, 77)], False)]),
    'gpu_pins': dict(lw=6, parts=[
        ([(84, 30), (96, 30)], False), ([(84, 44), (96, 44)], False),
        ([(84, 58), (96, 58)], False), ([(70, 76), (70, 90)], False),
        ([(82, 76), (82, 90)], False)]),
})

COMPOUND.update({
    'cpu': ('cpu', 'cpu_pins', None),
    'clock': ('clock', 'clock_hands', None),
    'terminal': ('terminal', 'terminal_marks', None),
    'document': ('document', 'document_lines', None),
    'gauge': ('gauge', 'gauge_ticks', None),
    'check': ('check_circle', None, None),
    'cross': ('x_circle', None, None),
    'gpu': ('gpu_card', 'gpu_pins', None),
    'stopwatch': ('stopwatch', 'stopwatch_hands', None),
    'checklist': ('checklist', 'checklist_ticks', None),
})


def _norm(parts):
    """Normalise a subpath list.

    Accepts three shapes:
      * [[ (x,y), ... ], ...]  - list of polygons
      * [ (x,y), (x,y), ... ]   - a single flat polygon
      * [ x, x, ... ]          - malformed (dropped)
    """
def _is_pt(o):
    return (isinstance(o, (list, tuple)) and len(o) == 2
            and all(isinstance(v, (int, float)) for v in o))


def _norm(parts):
    """Normalise a subpath list.

    Accepts:
      * [[(x,y), ...], ...]  list of polygons
      * [(x,y), (x,y), ...]   a single flat polygon
      * [x, x, ...]          malformed (dropped)
    """
    if not parts:
        return []
    if _is_pt(parts[0]):
        if len(parts) >= 3 and all(_is_pt(q) for q in parts):
            return [[tuple(q) for q in parts]]
        return []
    out = []
    for p in parts:
        if not isinstance(p, (list, tuple)) or len(p) < 3:
            continue
        if _is_pt(p[0]):
            if all(_is_pt(q) for q in p):
                out.append([tuple(q) for q in p])
            continue
        pts = [tuple(q) for q in p if _is_pt(q)]
        if len(pts) >= 3:
            out.append(pts)
    return out


def _fmt(pts, prec=2):
    return ' '.join(('%.*f,%.*f' % (prec, x, prec, y)) for x, y in pts)


def svg(name, color='#0A1224', size=None, stroke_scale=1.0):
    """Return an <svg> string for a single icon."""
    if name in COMPOUND:
        base, ov1, ov2 = COMPOUND[name]
    else:
        base, ov1, ov2 = name, None, None

    parts = []
    for pts in _norm(SOLID.get(base, [])):
        parts.append('<polygon points="%s" fill="%s"/>' % (_fmt(pts), color))

    for ov in (ov1, ov2):
        if ov and ov in LINE:
            d = LINE[ov]
            sw = max(1.4, d['lw'] * stroke_scale)
            for pts, closed in d['parts']:
                if len(pts) < 2:
                    continue
                if closed:
                    parts.append('<polygon points="%s" fill="none" '
                                 'stroke="%s" stroke-width="%.2f" '
                                 'stroke-linejoin="round"/>'
                                 % (_fmt(pts), color, sw))
                else:
                    pd = 'M ' + ' L '.join('%.2f,%.2f' % p for p in pts)
                    parts.append('<path d="%s" fill="none" stroke="%s" '
                                 'stroke-width="%.2f" stroke-linecap="round" '
                                 'stroke-linejoin="round"/>'
                                 % (pd, color, sw))

    if not parts:
        raise KeyError('unknown icon: %s' % name)
    dim = ' width="%s" height="%s"' % (size, size) if size else ''
    return ('<svg viewBox="0 0 100 100" xmlns="http://www.w3.org/2000/svg" '
            '%s fill="none">%s</svg>' % (dim, ''.join(parts)))


def css_mask(name):
    """Return a data-URI usable as a CSS mask (black fill)."""
    import base64
    s = svg(name, color='#000000')
    b64 = base64.b64encode(s.encode()).decode()
    return 'url("data:image/svg+xml;base64,%s")' % b64


ALL = sorted(set(list(SOLID.keys()) + list(COMPOUND.keys())))

