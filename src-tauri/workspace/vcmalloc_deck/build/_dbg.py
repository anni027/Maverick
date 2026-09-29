"""Overflow audit: measure every element against the 1280x720 slide box."""
import os
import json
from playwright.sync_api import sync_playwright

CHROME = r"C:\Program Files\Google\Chrome\Application\chrome.exe"
EDGE = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"

JS = """
() => {
  const out = [];
  document.querySelectorAll('.slide').forEach((s, i) => {
    const sb = s.getBoundingClientRect();
    s.querySelectorAll('*').forEach(el => {
      if (el.tagName === 'svg' || el.closest('svg')) return;
      const r = el.getBoundingClientRect();
      if (r.width === 0 || r.height === 0) return;
      const dx = Math.max(0, Math.round(r.right - sb.right));
      const dy = Math.max(0, Math.round(r.bottom - sb.bottom));
      const dx2 = Math.max(0, Math.round(sb.left - r.left));
      const dy2 = Math.max(0, Math.round(sb.top - r.top));
      const over = Math.max(dx, dy, dx2, dy2);
      if (over > 10) {
        out.push({
          slide: i + 1,
          tag: el.tagName.toLowerCase(),
          cls: (el.className || '').toString().slice(0, 40),
          txt: (el.textContent || '').trim().slice(0, 46),
          over: over,
          right: Math.round(r.right - sb.right),
          bottom: Math.round(r.bottom - sb.bottom)
        });
      }
      if (el.scrollHeight - el.clientHeight > 8 && el.clientHeight > 0) {
        out.push({slide: i + 1, tag: el.tagName.toLowerCase(),
                  cls: (el.className || '').toString().slice(0, 40),
                  txt: (el.textContent || '').trim().slice(0, 46),
                  clip: el.scrollHeight - el.clientHeight});
      }
    });
  });
  return out;
}
"""

with sync_playwright() as p:
    exe = CHROME if os.path.exists(CHROME) else EDGE
    b = p.chromium.launch(executable_path=exe)
    pg = b.new_page(viewport={'width': 1320, 'height': 760})
    pg.goto('file:///' + os.path.abspath('deck.html').replace('\\', '/'),
            wait_until='networkidle')
    pg.wait_for_timeout(600)
    issues = pg.evaluate(JS)
    b.close()

if not issues:
    print('CLEAN - no overflow or clipping detected on any slide')
else:
    print('ISSUES:', len(issues))
    for it in issues:
        print('  slide %-3s %-7s over=%-4s %s | %s'
              % (it['slide'], it['tag'], it.get('over', it.get('clip')),
                 it['cls'], it['txt']))


