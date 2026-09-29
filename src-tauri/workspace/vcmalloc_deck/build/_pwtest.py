from playwright.sync_api import sync_playwright

CHROME = r"C:\Program Files\Google\Chrome\Application\chrome.exe"
EDGE = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe"
import os
exe = CHROME if os.path.exists(CHROME) else EDGE

HTML = """<html><body style="margin:0;background:#0A1224">
<div style="font-family:'Segoe UI';color:#5CD3CE;font-size:64px;
     padding:60px">render ok</div></body></html>"""

with sync_playwright() as p:
    b = p.chromium.launch(executable_path=exe)
    pg = b.new_page(viewport={'width': 800, 'height': 300},
                    device_scale_factor=2)
    pg.set_content(HTML)
    pg.wait_for_timeout(400)
    pg.screenshot(path='_pwtest.png')
    print('OK via', exe, '| version', b.version)
    b.close()

