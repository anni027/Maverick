"""HTML/CSS deck generator for VCMalloc.

Emits a single self-contained HTML file (inline CSS + inline SVG icons +
base64 charts) at exactly 1280x720 per slide, which a headless browser then
rasterises to PNG for the PPTX.
"""
import base64
import os
import svgicons as SI

# ---------------------------------------------------------------- tokens
CSS = """
*,*::before,*::after{box-sizing:border-box;margin:0;padding:0}
:root{
  --ink:#0A1224; --ink-soft:#2B3A55; --slate:#5B6B8C; --muted:#8A98B4;
  --hair:#D8DFEA; --hair-soft:#ECF0F7; --wash:#F5F7FB; --white:#fff;
  --teal:#0E9E99; --teal-d:#07706E; --teal-l:#5CD3CE; --teal-xl:#D3F2F0; --teal-bg:#EBFAF9;
  --blue:#2F6BED; --blue-d:#1D4ED8; --blue-l:#93B4F7; --blue-xl:#DEE9FD;
  --amb:#E08A0C;  --amb-d:#A36207;  --amb-l:#F7C86B;  --amb-xl:#FDF1DB;
  --red:#DC3535;  --red-d:#9B1C1C;  --red-l:#F59E9E;  --red-xl:#FCE4E4;
  --grn:#1EA05F;  --grn-d:#157043;  --grn-xl:#DFF5E9;
  --pur:#7C3FED;  --pur-d:#5B28A8;  --pur-xl:#EEE7FD;
  --d1:#070F1E; --d2:#0B182E; --d3:#10213D; --dline:#22395E;
  --dtxt:#E8EFF9; --dtxt2:#9FB2CE;
  --W:1280px; --H:720px;
  --font:'Segoe UI','Segoe UI Variable Text',system-ui,-apple-system,sans-serif;
  --mono:'Cascadia Code','Consolas','SF Mono',monospace;
}
body{background:#3A4250;font-family:var(--font);-webkit-font-smoothing:antialiased;
     text-rendering:optimizeLegibility}

/* ---------------------------------------------------------- slide shell */
.slide{position:relative;width:var(--W);height:var(--H);overflow:hidden;
       background:var(--white);margin:0 auto 26px;page-break-after:always}
.slide.dark{background:linear-gradient(135deg,var(--d1) 0%,var(--d3) 100%)}
.pad{position:absolute;inset:0;padding:52px 70px 58px;display:flex;
     flex-direction:column;overflow:hidden}

/* -------------------------------------------------------------- header */
.eyebrow{display:flex;align-items:center;gap:12px;font-size:12.5px;font-weight:700;
  letter-spacing:.14em;text-transform:uppercase;color:var(--teal)}
.eyebrow::before{content:'';width:5px;height:26px;border-radius:3px;background:var(--teal)}
.eyebrow.blue{color:var(--blue)}.eyebrow.blue::before{background:var(--blue)}
.eyebrow.pur{color:var(--pur)}.eyebrow.pur::before{background:var(--pur)}
.eyebrow.amb{color:var(--amb)}.eyebrow.amb::before{background:var(--amb)}
h1.title{font-size:37px;font-weight:600;letter-spacing:-.015em;line-height:1.1;
  color:var(--ink);margin-top:12px}
h1.title.sm{font-size:33px}
.sub{font-size:14.5px;color:var(--slate);margin-top:9px;line-height:1.5;max-width:1010px}

/* -------------------------------------------------------------- footer */
.foot{position:absolute;left:70px;right:70px;bottom:26px;display:flex;
  align-items:center;justify-content:space-between;font-size:10.5px;
  color:var(--muted);border-top:1px solid var(--hair);padding-top:9px;
  letter-spacing:.05em}
.foot b{font-weight:700}
.foot .src{font-style:italic;font-size:9.5px;color:var(--muted);
  border-top:none;padding:0;position:absolute;left:70px;bottom:6px;
  transform:translateY(100%)}

/* --------------------------------------------------------------- cards */
.card{background:var(--white);border:1px solid var(--hair);border-radius:14px;
  position:relative;overflow:hidden}
.card.pad{padding:22px 24px}
.card.top-accent::before{content:'';position:absolute;left:14px;right:14px;top:0;
  height:4px;background:var(--accent,var(--teal))}
.card.left-accent::before{content:'';position:absolute;left:0;top:12px;bottom:12px;
  width:4px;border-radius:0 3px 3px 0;background:var(--accent,var(--teal))}
.card.teal{--accent:var(--teal)}.card.blue{--accent:var(--blue)}
.card.amb{--accent:var(--amb)}.card.red{--accent:var(--red)}
.card.grn{--accent:var(--grn)}.card.pur{--accent:var(--pur)}
.card.fill-teal{background:var(--teal-bg);border-color:var(--teal-l)}
.card.fill-amb{background:var(--amb-xl);border-color:var(--amb-l)}
.card.fill-blue{background:var(--blue-xl);border-color:var(--blue-l)}
.card.fill-wash{background:var(--wash)}
.card.fill-ink{background:var(--ink);border-color:var(--ink);color:var(--dtxt)}

/* ----------------------------------------------------------- icon badge */
.ibadge{width:54px;height:54px;border-radius:14px;display:grid;place-items:center;
  flex:0 0 auto}
.ibadge svg{width:26px;height:26px}
.ibadge.tint-teal{background:var(--teal-xl)}.ibadge.tint-blue{background:var(--blue-xl)}
.ibadge.tint-amb{background:var(--amb-xl)}.ibadge.tint-red{background:var(--red-xl)}
.ibadge.tint-grn{background:var(--grn-xl)}.ibadge.tint-pur{background:var(--pur-xl)}
.ibadge.tint-ink{background:var(--d3)}
.ibadge.sm{width:34px;height:34px;border-radius:10px}
.ibadge.sm svg{width:17px;height:17px}
.ibadge.lg{width:66px;height:66px;border-radius:18px}
.ibadge.lg svg{width:32px;height:32px}
.ibadge.dark{background:var(--d3)}

/* ------------------------------------------------------------ stat tile */
.stat{display:flex;flex-direction:column}
.stat .val{font-size:38px;font-weight:700;line-height:1;letter-spacing:-.02em;
  color:var(--c,var(--teal));margin-top:14px}
.stat .val em{font-style:normal;font-size:19px}
.stat .lab{font-size:14px;font-weight:700;color:var(--ink);margin-top:9px;line-height:1.25}
.stat .note{font-size:11.5px;color:var(--slate);margin-top:7px;line-height:1.42}

/* --------------------------------------------------------------- chips */
.chip{display:inline-flex;align-items:center;justify-content:center;
  border-radius:999px;padding:4px 13px;font-size:12px;font-weight:700;
  letter-spacing:.02em;line-height:1.25}
.chip.mono{font-family:var(--mono);font-size:11px;padding:4px 12px}
.row{display:flex;align-items:center}
.grow{flex:1 1 auto;min-width:0}
.center{text-align:center}

/* ------------------------------------------------------------- bullets */
ul.bul{list-style:none;display:flex;flex-direction:column;gap:11px}
ul.bul li{position:relative;padding-left:22px;font-size:12.5px;line-height:1.45;
  color:var(--ink-soft)}
ul.bul li::before{content:'';position:absolute;left:0;top:6px;width:7px;height:7px;
  border-radius:2px;background:var(--dot,var(--teal))}
ul.bul li b{color:var(--ink);font-weight:700}
ul.bul.tight li{font-size:11.5px;line-height:1.4}

/* ------------------------------------------------------------ kpi strip */
.kpis{display:grid;gap:16px}
.kpi{border-radius:12px;padding:13px 17px;background:var(--teal-xl)}
.kpi .v{font-size:22px;font-weight:700;color:var(--c);line-height:1.1}
.kpi .l{font-size:11px;font-weight:700;color:var(--ink-soft);margin-top:5px;
  line-height:1.35;white-space:pre-line}

/* -------------------------------------------------------------- blocks */
.blk{border-radius:9px;display:flex;flex-direction:column;align-items:center;
  justify-content:center;text-align:center;position:relative}
.blk .t{font-size:11.5px;font-weight:700;line-height:1.2}
.blk .s{font-size:9.5px;margin-top:3px;line-height:1.25}

/* ----------------------------------------------------------- bar strips */
.bar{display:flex;border-radius:7px;overflow:hidden;background:var(--wash);
  border:1px solid var(--hair)}
.bar .seg{height:100%}
.memgrid{display:flex;gap:5px;padding:7px 9px;border-radius:8px;background:var(--ink)}
.memgrid .f{height:20px;flex:1;border-radius:4px}

/* -------------------------------------------------------------- charts */
.chart{width:100%;display:block}
.chartwrap{display:flex;flex-direction:column;gap:6px}
.chartcap{font-size:14px;font-weight:700;color:var(--ink)}
.chartsub{font-size:11px;color:var(--slate)}

/* --------------------------------------------------------------- misc */
.mono{font-family:var(--mono)}
.t-teal{color:var(--teal)}.t-blue{color:var(--blue)}.t-amb{color:var(--amb)}
.t-red{color:var(--red)}.t-grn{color:var(--grn)}.t-pur{color:var(--pur)}
.muted{color:var(--muted)}.slate{color:var(--slate)}.ink{color:var(--ink)}
.b{font-weight:700}
.tag{display:inline-block;padding:3px 10px;border-radius:6px;font-size:10.5px;
  font-weight:700;letter-spacing:.06em;text-transform:uppercase}
.divider{height:1px;background:var(--hair)}
.num{font-variant-numeric:tabular-nums}
"""


def icon(name, color, cls=''):
    return SI.svg(name, color)


def b64(path):
    with open(path, 'rb') as f:
        return 'data:image/png;base64,' + base64.b64encode(f.read()).decode()


# =====================================================================
#  reusable fragments
# =====================================================================
def head(eyebrow, title, sub=None, accent='teal', sm=False):
    h = [f'<div class="eyebrow {accent}">{eyebrow}</div>',
         f'<h1 class="title{" sm" if sm else ""}">{title}</h1>']
    if sub:
        h.append(f'<div class="sub">{sub}</div>')
    return ''.join(h)


def foot(num, label, total=16, src=None):
    s = (f'<div class="foot"><span>{label}</span>'
         f'<span><b>{num} / {total}</b></span></div>')
    if src:
        s = ('<div class="foot" style="bottom:30px">'
             f'<span>{label}</span><span><b>{num} / {total}</b></span></div>'
             f'<div style="position:absolute;left:70px;bottom:10px;font-size:9.5px;'
             f'font-style:italic;color:var(--muted)">{src}</div>')
    return s


def badge(name, tint='tint-teal', color='var(--teal-d)', sm=False, lg=False):
    c = 'ibadge %s%s%s' % (tint, ' sm' if sm else '', ' lg' if lg else '')
    return f'<div class="{c}">{icon(name, color)}</div>'


def block(t, s, bg, tc='#fff', sc=None, w=None, h=None, style=''):
    st = 'background:%s;color:%s;' % (bg, tc)
    if w:
        st += 'width:%dpx;' % w
    if h:
        st += 'height:%dpx;' % h
    st += style
    return (f'<div class="blk" style="{st}"><div class="t">{t}</div>'
            f'<div class="s" style="color:{sc or tc}">{s}</div></div>')


def kpi(v, l, c='var(--teal)', bg='var(--teal-xl)'):
    return (f'<div class="kpi" style="background:{bg}">'
            f'<div class="v" style="--c:{c}">{v}</div>'
            f'<div class="l">{l}</div></div>')


def chart_card(path, title, sub, style='', body=''):
    """Chart panel. Height is driven by the image's natural aspect ratio so
    the panel can never clip the chart."""
    img = b64(path)
    from PIL import Image as _Im
    with _Im.open(path) as im:
        ar = im.height / float(im.width)
    # style may carry 'maxh:NNNpx' -> clamp the rendered width accordingly
    maxh = None
    for tok in style.split(';'):
        if tok.strip().startswith('maxh:'):
            maxh = int(tok.split(':')[1].replace('px', '').strip())
    return f'''<div class="card" style="{style};padding:14px 18px 12px;display:flex;
  flex-direction:column">
  <div style="display:flex;align-items:center;gap:9px;flex:0 0 auto">
    <span style="width:19px;height:19px;display:block;flex:0 0 auto">{icon('bar_chart','var(--teal)')}</span>
    <div class="chartcap">{title}</div>
  </div>
  {f'<div class="chartsub" style="flex:0 0 auto">{sub}</div>' if sub else ''}
  <img class="chart" src="{img}" style="flex:1 1 auto;min-height:0;width:100%;
    object-fit:contain;object-position:top center;margin-top:6px;{body}"
    data-ar="{ar:.4f}"{f' data-maxh="{maxh}"' if maxh else ''}/>
</div>'''


# =====================================================================
#  SLIDES
# =====================================================================
def s1_title():
    bars = ''.join(
        f'<div style="position:absolute;left:{8.25 + i * 0.52}in;'
        f'top:{1.05 + (i % 3) * 0.30}in;width:0.44in;height:2.55in;'
        f'border-radius:6px;background:var(--teal);opacity:{0.26 + i * 0.06}"></div>'
        for i in range(9))
    segs = ''.join(
        f'<div style="flex:1;background:var(--teal-l)"></div>'
        if i else '' for i in range(5))
    import random
    rnd = random.Random(11)
    phys = ''.join(
        f'<div class="f" style="position:absolute;left:{8.25 + rnd.random() * 3.9}in;'
        f'top:{5.05 + rnd.random() * 1.15}in;width:0.36in;height:0.36in;'
        f'background:var(--amb-l);opacity:.72"></div>' for _ in range(11))
    return f'''<div class="slide dark">
  <div style="position:absolute;inset:0;background:
     radial-gradient(1100px 620px at 82% 18%,rgba(14,158,153,.20),transparent 62%)"></div>
  {bars}
  <div style="position:absolute;left:8.25in;top:4.05in;width:4.42in;height:.40in;
       border-radius:8px;background:var(--teal-l);display:flex;overflow:hidden">
    {''.join(f'<div style="flex:1;border-right:1px solid var(--d1)"></div>' for _ in range(5))}
  </div>
  <div style="position:absolute;left:8.25in;top:4.60in;width:4.42in;text-align:center;
       font-size:9.5px;font-weight:700;letter-spacing:.14em;color:var(--teal-l)">
     ONE CONTIGUOUS VIRTUAL RANGE</div>
  {phys}
  <div style="position:absolute;left:8.25in;top:6.30in;width:4.42in;text-align:center;
       font-size:9.5px;font-weight:700;letter-spacing:.14em;color:var(--amb-l)">
     SCATTERED PHYSICAL FRAMES</div>

  <div style="position:absolute;left:70px;top:118px;width:770px">
    <div class="eyebrow" style="color:var(--teal-l)">IEEE Transactions on Computers
      &nbsp;·&nbsp; Vol. 72, No. 12 &nbsp;·&nbsp; 2023</div>
    <div style="font-size:70px;font-weight:700;color:#fff;letter-spacing:-.03em;
         line-height:1;margin-top:22px">VCMalloc</div>
    <div style="font-size:31px;color:var(--dtxt);line-height:1.24;margin-top:14px">
      A Virtually Contiguous<br/>Memory Allocator</div>
    <div style="height:3px;width:74px;background:var(--teal);margin:24px 0 18px"></div>
    <div style="font-size:14.5px;color:var(--dtxt2);line-height:1.55;max-width:660px">
      A custom allocator for Microsoft&nbsp;Windows that keeps user data virtually
      contiguous while the operating system retains full control of physical memory.</div>
    <div style="font-size:12px;color:var(--dtxt2);line-height:1.6;margin-top:30px">
      Yacine Hadjadj &nbsp;·&nbsp; Chakib Mustapha Anouar Zouaoui &nbsp;·&nbsp;
      Nasreddine Taleb &nbsp;·&nbsp; Mohamed El Bahri<br/>
      Miloud Chikr El Mezouar &nbsp;·&nbsp; Sarah Mazari</div>
    <div style="font-size:12px;color:var(--muted);margin-top:9px">
      RCAM Laboratory, Djillali Liabes University, Sidi Bel Abbes, Algeria</div>
    <div class="row" style="margin-top:24px;gap:11px;display:inline-flex;
         border:1px solid var(--dline);background:var(--d3);border-radius:20px;
         padding:9px 18px">
      <span style="width:17px;height:17px;display:block">{icon('repo','var(--teal-l)')}</span>
      <span style="font-size:12px;color:var(--dtxt)">github.com/ycinhdj/vcmalloc</span>
    </div>
  </div>
</div>'''


def s2_summary():
    tiles = [
        ('+28%', 'Allocation speed', 'Cycles vs. MIMalloc on large allocations (10,000 x 10,000).', 'var(--teal)', 'bolt', 'tint-teal'),
        ('+26%', 'Reallocation speed', 'Fewer cycles to grow a matrix, via frame remapping instead of copying.', 'var(--blue)', 'loop', 'tint-blue'),
        ('0 B', 'Fragmentation', 'Average fragmentation reported as exactly zero at every structure size tested.', 'var(--grn)', 'verified', 'tint-grn'),
        ('+31%', 'Matrix multiply', 'Cache-aware gain on C = A x B naive multiplication (M up to 8,000).', 'var(--pur)', 'matrix', 'tint-pur'),
    ]
    t = ''
    for v, lab, note, c, ic, tn in tiles:
        t += f'''<div class="card top-accent" style="padding:20px 22px 18px;
          --accent:{c}">
          <div class="stat" style="--c:{c}">
            {badge(ic, tn, c, sm=True)}
            <div class="val">{v}</div>
            <div class="lab">{lab}</div>
            <div class="note">{note}</div>
          </div>
        </div>'''
    return f'''<div class="slide"><div class="pad">
  {head('Executive summary', 'What VCMalloc delivers',
        'A general-purpose allocator optimised for virtual contiguity, cache behaviour and low fragmentation.')}
  <div class="kpis" style="grid-template-columns:repeat(4,1fr);margin-top:26px">{t}</div>
  <div class="card fill-teal" style="margin-top:26px;padding:20px 24px;
       display:flex;gap:18px;align-items:flex-start">
    {badge('lightbulb', 'tint-teal', 'var(--teal-d)')}
    <div>
      <div style="font-size:15px;font-weight:700;color:var(--ink)">The central result</div>
      <div style="font-size:13px;color:var(--ink-soft);line-height:1.5;margin-top:6px">
        MIMalloc already beats Malloc on speed, but pays up to 225% more memory.
        VCMalloc keeps Malloc-like memory consumption while matching or beating
        both on cycles &mdash; the only allocator in the comparison that produced
        perfectly contiguous allocations.</div>
    </div>
  </div>
  {foot(2, 'VCMalloc &nbsp;·&nbsp; Executive summary')}
</div></div>'''


def s3_problem():
    return f'''<div class="slide"><div class="pad">
  {head('Motivation', 'Why contiguity is a performance problem',
        'Perfectly adjacent structures eliminate fragmentation and improve both cache and program behaviour.')}
  <div style="display:grid;grid-template-columns:1fr 1fr;gap:26px;margin-top:24px">
    <div class="card top-accent teal" style="padding:20px 24px">
      <div style="font-size:16px;font-weight:700;color:var(--ink)">With perfect contiguity</div>
      <ul class="bul" style="margin-top:15px">
        <li><b>Total fragmentation elimination</b> &mdash; all structures adjacent</li>
        <li><b>Fewer cache defects</b> &mdash; prefetchers work on predictable spans</li>
        <li><b>Better readability</b> &mdash; layout mirrors the algorithm</li>
        <li><b>GPU-ready</b> &mdash; devices that require contiguity become usable</li>
      </ul>
    </div>
    <div class="card top-accent amb fill-amb" style="padding:20px 24px">
      <div style="font-size:16px;font-weight:700;color:var(--ink)">But adjacent blocks break resizing</div>
      <ul class="bul" style="margin-top:15px;--dot:var(--amb)">
        <li><b>Realloc and free move data</b> &mdash; time-consuming</li>
        <li><b>Every trailing reference must be updated</b> &mdash; linear cost</li>
        <li><b>Pre-allocating large blocks instead is wasteful</b> &mdash; sizing is
            guesswork and memory is stranded</li>
      </ul>
    </div>
  </div>
  <div class="card" style="margin-top:24px;padding:20px 24px">
    <div style="font-size:15px;font-weight:700;color:var(--ink)">The design tension</div>
    <div style="font-size:13px;color:var(--ink-soft);line-height:1.5;margin-top:8px">
      Contiguity gives locality but forbids the one operation data structures
      need most. Existing contiguous allocators (CMA, GCMA) sidestep this by
      being static &mdash; all in-kernel, all fixed size. VCMalloc has to keep
      contiguity while resizing.</div>
  </div>
  {foot(3, 'VCMalloc &nbsp;·&nbsp; Motivation')}
</div></div>'''


def s4_contiguity():
    import random
    rnd = random.Random(5)
    disp = ''.join(f'<div style="position:absolute;left:{6 + f}%;top:9px;width:62px;'
                   f'height:28px;border-radius:5px;background:var(--amb)"></div>'
                   for f in (6, 30, 55, 79))
    contig = ''.join(f'<div style="flex:1;background:var(--teal)"></div>'
                     for _ in range(4))
    def phys(on):
        cells = ''.join(
            f'<div class="f" style="background:{"var(--teal-l)" if (i in on) else "var(--teal-xl)"}"></div>'
            for i in range(7))
        return f'<div class="memgrid" style="position:absolute;left:12px;right:12px;' \
               f'bottom:10px">{cells}</div>'
    return f'''<div class="slide"><div class="pad">
  {head('Core idea', 'Virtual contiguity without physical contiguity',
        'The application sees one gap-free address range. The operating system still scatters the backing frames.')}
  <div style="display:grid;grid-template-columns:1fr 1fr;gap:26px;margin-top:22px">

    <div class="card top-accent amb" style="padding:18px 22px;height:330px">
      <div style="font-size:16px;font-weight:700;color:var(--ink)">Conventional allocator</div>
      <div style="font-size:12px;color:var(--slate);margin-top:5px">
        User pointers land wherever the heap allows</div>
      <div style="position:relative;height:48px;margin-top:20px;border-radius:8px;
           background:var(--wash);border:1px solid var(--hair)">
        <div style="position:absolute;left:10px;top:6px;font-size:9px;font-weight:700;
             letter-spacing:.1em;color:var(--muted)">VIRTUAL ADDRESS SPACE</div>
        {disp}
      </div>
      <div style="text-align:center;font-size:11.5px;font-weight:700;color:var(--amb-d);
           margin-top:9px">Data structures are dispersed across the heap</div>
      <div style="position:relative;height:92px;margin-top:18px;border-radius:10px;
           background:var(--ink)">
        <div style="position:absolute;left:12px;top:9px;font-size:9px;font-weight:700;
             letter-spacing:.1em;color:var(--muted)">PHYSICAL MEMORY</div>
        {phys(set())}
      </div>
      <div style="text-align:center;font-size:11.5px;color:var(--muted);margin-top:8px">
        Frames interleaved with unrelated data</div>
    </div>

    <div class="card top-accent teal fill-teal" style="padding:18px 22px;height:330px">
      <div style="font-size:16px;font-weight:700;color:var(--ink)">VCMalloc</div>
      <div style="font-size:12px;color:var(--slate);margin-top:5px">
        One hypercontainer, many containers, still physically scattered</div>
      <div style="position:relative;height:48px;margin-top:20px;border-radius:8px;
           background:#fff;border:1px solid var(--teal);display:flex;gap:5px;padding:9px 6px">
        {contig}
      </div>
      <div style="text-align:center;font-size:11.5px;font-weight:700;color:var(--teal-d);
           margin-top:9px">Containers are adjacent and in order</div>
      <div style="position:relative;height:92px;margin-top:18px;border-radius:10px;
           background:var(--ink)">
        <div style="position:absolute;left:12px;top:9px;font-size:9px;font-weight:700;
             letter-spacing:.1em;color:var(--muted)">PHYSICAL MEMORY</div>
        {phys({0, 2, 3, 6})}
      </div>
      <div style="text-align:center;font-size:11.5px;color:var(--muted);margin-top:8px">
        Frames still scattered &mdash; contiguity is virtual only</div>
    </div>
  </div>

  <div class="card" style="margin-top:22px;padding:16px 22px;display:flex;
       gap:16px;align-items:center">
    {badge('key', 'tint-teal', 'var(--teal-d)', sm=True)}
    <div style="font-size:12.5px;color:var(--ink-soft);line-height:1.45">
      VCMalloc is deliberately <b>not</b> a drop-in malloc: the hypercontainer
      argument changes the call signature, so the user opts in and keeps
      explicit control of layout.</div>
  </div>
  {foot(4, 'VCMalloc &nbsp;·&nbsp; Core idea')}
</div></div>'''


def s5_model():
    cont = ''.join(f'<div class="blk" style="width:56px;height:44px;'
                   f'background:var(--teal);color:#fff;border-radius:7px;'
                   f'font-size:11px;font-weight:700">C{i+1}</div>' for i in range(4))
    scat = ''.join(f'<div class="blk" style="width:56px;height:44px;'
                   f'background:var(--teal-l);color:var(--ink);border-radius:7px;'
                   f'font-size:11px;font-weight:700;transform:translateY({(0,4,-2,2)[i]}px)">'
                   f'C{i+1}</div>' for i in range(4))
    ctx = [
        ('VCMCC', 'Container Context', 'One memory zone. Keeps index, pointer, size, reference, offset, mapping address and next free address.', 'memory', 'teal', 'tint-teal'),
        ('VCMHCC', 'Hyper Container Context', 'Many containers. Tracks counts, PFN storage, container pointers and references, and the first / last container.', 'layers', 'blue', 'tint-blue'),
        ('VCMHCM', 'Hyper Container Manager', 'The root. Stores every HCC and hypercontainer address, and resolves an address to its index.', 'grid', 'pur', 'tint-pur'),
    ]
    cc = ''
    for code, name, desc, ic, col, tn in ctx:
        cc += f'''<div class="card" style="padding:16px 18px;height:136px">
          <div style="display:flex;gap:12px;align-items:flex-start">
            {badge(ic, tn, 'var(--%s)' % col, sm=True)}
            <div><div style="font-size:15px;font-weight:700;color:var(--{col})">{code}</div>
            <div style="font-size:10px;font-weight:700;color:var(--muted);
                 letter-spacing:.05em;margin-top:2px">{name.upper()}</div></div>
          </div>
          <div style="font-size:11px;color:var(--ink-soft);line-height:1.4;margin-top:10px">{desc}</div>
        </div>'''
    return f'''<div class="slide"><div class="pad">
  {head('Abstraction', 'Containers, hypercontainers and the manager',
        'Three concepts formalise virtual contiguity: a memory zone, an ordered set of zones, and the table that tracks them.')}
  <div style="display:grid;grid-template-columns:1fr 372px;gap:24px;margin-top:20px">

    <div class="card fill-wash" style="padding:22px 24px;height:452px;position:relative">
      <!-- level 1 -->
      {block('Hyper Container Manager', 'VCMHCM  ·  indexes + addresses of every hypercontainer',
             'var(--ink)', '#fff', 'var(--dtxt2)', w=340, h=76)}
      <div style="text-align:center;padding-top:14px">
        <span style="display:inline-block;width:1px;height:16px;background:var(--hair)"></span>
      </div>
      <!-- level 2 -->
      <div style="display:flex;justify-content:space-between;gap:26px">
        {block('Hypercontainer 1', 'reserved virtual range', 'var(--blue)', '#fff', 'var(--blue-xl)', w=246, h=92)}
        {block('Hypercontainer 2', 'reserved virtual range', 'var(--blue)', '#fff', 'var(--blue-xl)', w=246, h=92)}
      </div>
      <div style="display:flex;justify-content:space-around;padding-top:12px">
        <span style="display:inline-block;width:1px;height:14px;background:var(--hair)"></span>
        <span style="display:inline-block;width:1px;height:14px;background:var(--hair)"></span>
      </div>
      <!-- level 3 -->
      <div style="display:flex;justify-content:space-around;gap:20px">
        <div style="display:flex;gap:9px">{cont}</div>
        <div style="display:flex;gap:9px">{scat}</div>
      </div>
      <div style="display:flex;justify-content:space-around;margin-top:9px">
        <div style="font-size:11px;font-weight:700;color:var(--teal-d)">
          containers &mdash; adjacent and ordered</div>
        <div style="font-size:11px;color:var(--muted)">
          still ordered, hypercontainer moved</div>
      </div>
      <!-- rule -->
      <div class="card" style="position:absolute;left:24px;right:24px;bottom:18px;
           padding:13px 18px;display:flex;gap:14px;align-items:center;
           border-color:var(--teal-l)">
        {badge('contiguous', 'tint-teal', 'var(--teal-d)', sm=True)}
        <div style="font-size:12px;color:var(--ink-soft);line-height:1.4">
          <b>Contiguity rule:</b> all containers sharing a hypercontainer are
          adjacent; different hypercontainers are dispersed.</div>
      </div>
    </div>

    <div style="display:flex;flex-direction:column;gap:11px">{cc}</div>
  </div>
  {foot(5, 'VCMalloc &nbsp;·&nbsp; Abstraction')}
</div></div>'''


def s6_flow():
    steps = [
        ('1', 'Reserve', 'vc_hcalloc', 'Pick an address after the last hypercontainer, stepping by system allocation granularity until one is free. No physical memory is committed yet.', 'reserve', 'var(--blue)', 'tint-blue'),
        ('2', 'Allocate', 'vc_malloc', "Create a container inside a chosen hypercontainer, request page frames from the OS and map them into the container's virtual range.", 'plus', 'var(--teal)', 'tint-teal'),
        ('3', 'Resize', 'vc_resize', 'Release or request pages inside the hypercontainer and revise offsets. Guest bytes copied are always smaller than one page.', 'loop', 'var(--pur)', 'tint-pur'),
        ('4', 'Reallocate', 'vc_realloc', "If the container cannot grow in place, move it to the end of the hypercontainer by remapping zones rather than copying data.", 'arrow_swap', 'var(--amb)', 'tint-amb'),
        ('5', 'Release', 'vc_hcfree', 'Unmap and free the frames, return the virtual range, then update the manager.', 'trash', 'var(--red)', 'tint-red'),
    ]
    cards = ''
    for num, name, api, desc, ic, c, tn in steps:
        arrow = ('<div style="align-self:center;width:0;height:0;border-top:7px solid transparent;'
                 'border-bottom:7px solid transparent;border-left:9px solid var(--hair)"></div>')
        cards += f'''<div class="card" style="flex:1;padding:0;position:relative">
          <div style="height:4px;background:{c};border-radius:0"></div>
          <div style="padding:18px 18px 20px">
            <div style="display:flex;align-items:center;justify-content:space-between">
              <div style="width:30px;height:30px;border-radius:50%;background:{c};color:#fff;
                   display:grid;place-items:center;font-size:13px;font-weight:700">{num}</div>
              <span style="width:26px;height:26px;display:block">{icon(ic, c)}</span>
            </div>
            <div style="font-size:17px;font-weight:700;color:var(--ink);margin-top:16px">{name}</div>
            <div class="chip mono" style="margin-top:10px;background:var(--{tn[5:]});color:{c}">{api}</div>
            <div style="font-size:11.5px;color:var(--ink-soft);line-height:1.45;margin-top:14px">{desc}</div>
          </div>
        </div>'''
        cards = cards.replace('</div>\n        </div>', '</div>')
        cards += '</div>'
    apis = ''.join(
        f'<span class="chip mono" style="background:var(--d3);color:var(--teal-l);'
        f'border:1px solid var(--dline)">{a}</span>'
        for a in ['VirtualAlloc', 'AllocateUserPhysicalPages', 'MapUserPhysicalPages', 'FreeUserPhysicalPages'])
    return f'''<div class="slide"><div class="pad">
  {head('Mechanism', 'What actually happens on each operation',
        'Five steps, driven by the Windows AWE API rather than the ordinary heap.')}
  <div style="display:flex;gap:12px;margin-top:24px;align-items:stretch">
    {cards}
  </div>
  <div class="card fill-ink" style="margin-top:22px;padding:18px 24px;display:flex;
       gap:24px;align-items:center">
    <div style="flex:0 0 auto">
      <div class="row" style="gap:12px">{badge('os_stack','tint-ink','var(--teal-l)',sm=True)}
      <span style="font-size:15px;font-weight:700;color:#fff">Windows AWE API</span></div>
      <div style="font-size:11.5px;color:var(--dtxt2);margin-top:8px">The foundation</div>
    </div>
    <div class="row" style="gap:8px;flex-wrap:wrap;flex:0 0 520px">{apis}</div>
    <div style="font-size:11px;color:var(--dtxt2);line-height:1.45;flex:1">
      Meta-information itself is heap-allocated and grows implicitly &mdash; so
      VCMalloc offers APIs to pre-extend contexts and avoid repeated reallocation.</div>
  </div>
  {foot(6, 'VCMalloc &nbsp;·&nbsp; Mechanism')}
</div></div>'''


def s7_realloc():
    strat = [
        ('In order', 'vc_resize', 'var(--teal)', 'tint-teal', 'contiguous',
         'Keeps the allocation order intact, so the data structure stays linear. Containers after the resized one are shifted to hold their positions. The pointer never changes.',
         'Preserves order', 'Shifts trailing containers'),
        ('Out of order', 'vc_realloc', 'var(--amb)', 'tint-amb', 'dispersed',
         'Only the affected container moves, and only when it cannot grow in place &mdash; it is relocated to the end of the hypercontainer. Cheaper, but leaves fragmentation between 0 and SP-1 bytes.',
         'Touches one container', 'Residual fragmentation'),
        ('Multiple', 'vc_mresize', 'var(--pur)', 'tint-pur', 'matrix',
         'Resizes s consecutive containers in a single remapping pass &mdash; the right choice when a multi-dimensional array grows, since all dimensions resize together.',
         'One remap for M rows', 'Ideal for ND arrays'),
    ]
    cards = ''
    for name, api, c, tn, ic, desc, plus, minus in strat:
        cards += f'''<div class="card" style="flex:1;padding:0;position:relative">
          <div style="height:4px;background:{c}"></div>
          <div style="padding:20px 20px 0">
            <div style="display:flex;gap:14px;align-items:center">
              {badge(ic, tn, c)}
              <div style="font-size:18px;font-weight:700;color:var(--ink)">{name}</div>
            </div>
            <div class="chip mono" style="margin-top:14px;background:{'var(--teal-xl)' if c=='var(--teal)' else 'var(--amb-xl)' if c=='var(--amb)' else 'var(--pur-xl)'};color:{c}">{api}</div>
            <div style="font-size:12px;color:var(--ink-soft);line-height:1.5;margin-top:14px;
                 min-height:120px">{desc}</div>
          </div>
          <div style="position:absolute;left:20px;right:20px;bottom:18px">
            <div class="divider" style="margin-bottom:10px"></div>
            <div class="row" style="gap:8px;margin-bottom:7px">
              <span style="width:15px;height:15px;display:block">{icon('check','var(--grn-d)')}</span>
              <span style="font-size:11px;font-weight:700;color:var(--grn-d)">{plus}</span></div>
            <div class="row" style="gap:8px">
              <span style="width:15px;height:15px;display:block">{icon('cross','var(--red-d)')}</span>
              <span style="font-size:11px;font-weight:700;color:var(--red-d)">{minus}</span></div>
          </div>
        </div>'''
    return f'''<div class="slide"><div class="pad">
  {head('Contiguity under change', 'Three ways to reallocate',
        'Each strategy trades a different cost: preserving order, avoiding fragmentation, or resizing many arrays at once.', accent='pur')}
  <div style="display:flex;gap:22px;margin-top:22px">{cards}</div>
  <div class="card fill-ink" style="margin-top:24px;padding:18px 24px;display:flex;
       gap:22px;align-items:center">
    {badge('beaker','tint-ink','var(--teal-l)')}
    <div style="flex:0 0 260px">
      <div style="font-size:14px;font-weight:700;color:#fff">Fragmentation bound</div>
      <div class="mono" style="font-size:13px;color:var(--teal-l);font-weight:700;margin-top:7px">
        F = SP &minus; Smod&nbsp; when S&Delta; &gt; 0</div>
      <div class="mono" style="font-size:13px;color:var(--teal-l);font-weight:700;margin-top:3px">
        F = Smod&nbsp; otherwise</div>
    </div>
    <div style="width:1px;align-self:stretch;background:var(--dline)"></div>
    <div style="font-size:12px;color:var(--dtxt2);line-height:1.5">
      Worst case is one page short of full. Guest bytes copied are always smaller
      than one page, which is what keeps reallocation cheap compared with the
      traditional copy-everything approach.</div>
  </div>
  {foot(7, 'VCMalloc &nbsp;·&nbsp; Reallocation')}
</div></div>'''


def s8_api():
    groups = [
        ('Allocation', 'var(--teal)', 'tint-teal', 'plus', [
            ('vc_hcalloc(s)', 'Pre-allocate a virtual hypercontainer of size s'),
            ('vc_malloc(hc, s)', 'Allocate s bytes in hypercontainer hc'),
            ('vcr_malloc(hc, ref, s)', 'As above, and track a user reference'),
            ('vca_malloc(s)', 'Allocate s bytes and track a reference'),
        ]),
        ('Reallocation', 'var(--blue)', 'tint-blue', 'loop', [
            ('vc_resize(ptr, s)', 'Resize a container in place to s bytes'),
            ('vc_mresize(ptr, sizes, s)', 'Resize s consecutive containers in one pass'),
            ('vc_realloc(ptr, s)', 'Resize in place, or move the container to the end of hc'),
        ]),
        ('Deallocation', 'var(--red)', 'tint-red', 'trash', [
            ('vc_hcfree(hc)', 'Deallocate an entire hypercontainer'),
            ('vc_hcmfree(hcm)', 'Free every hypercontainer in the manager'),
        ]),
    ]
    gs = ''
    for gname, c, tn, ic, rows in groups:
        rr = ''
        for fn, desc in rows:
            rr += f'''<div style="display:flex;align-items:center;gap:16px;margin-top:7px">
              <span class="chip mono" style="background:var(--wash);border:1px solid var(--hair);
                    color:{c};min-width:200px">{fn}</span>
              <span style="font-size:11px;color:var(--ink-soft)">{desc}</span></div>'''
        gs += f'''<div class="card left-accent" style="--accent:{c};padding:10px 18px;
          margin-top:9px">
          <div style="display:flex;align-items:center;gap:13px">
            {badge(ic, tn, c, sm=True)}
            <span style="font-size:14.5px;font-weight:700;color:var(--ink)">{gname}</span>
            <span style="margin-left:auto;font-size:11px;color:var(--muted)">
              {len(rows)} call{'' if len(rows)==1 else 's'}</span>
          </div>{rr}
        </div>'''
    code = ''.join(
        f'<div class="mono" style="font-size:10px;color:var(--ink-soft);'
        f'line-height:1.6">{l}</div>' for l in [
            'char* hc = vc_hcalloc(hc_size);',
            'double** A = vc_malloc(hc,',
            '                  sizeof(double*)*M);',
            'for (i = 0; i &lt; M; i++)',
            '  A[i] = vc_malloc(hc,',
            '                 sizeof(double)*M);',
            'vc_hcfree(hc);'])
    return f'''<div class="slide"><div class="pad">
  {head('Interface', 'The VCMalloc API surface',
        'Seven entry points across three families &mdash; roughly 1,500 lines of C and C++.', accent='amb')}
  <div style="display:grid;grid-template-columns:1fr 330px;gap:24px;margin-top:14px;
       flex:1 1 auto;min-height:0">
    <div style="display:flex;flex-direction:column;justify-content:flex-start">{gs}</div>
    <div style="display:flex;flex-direction:column">
      <div class="card fill-wash" style="padding:15px 18px">
        <div class="row" style="gap:12px">{badge('terminal','tint-ink','var(--teal-l)',sm=True)}
          <span style="font-size:14px;font-weight:700;color:var(--ink)">Typical use</span></div>
        <div style="margin-top:10px">{code}</div>
      </div>
      <div class="card fill-amb" style="padding:15px 18px;margin-top:11px">
        <div class="row" style="gap:12px">{badge('thread_risk','tint-amb','var(--amb-d)',sm=True)}
          <span style="font-size:14px;font-weight:700;color:var(--ink)">No concurrency yet</span></div>
        <div style="font-size:11.5px;color:var(--ink-soft);line-height:1.45;margin-top:9px">
          The current version has no concurrent allocation strategy. Multi-threaded
          users must add their own synchronisation.</div>
      </div>
      <div class="card" style="padding:14px 18px;margin-top:11px;display:flex;gap:13px">
        {badge('pinned','tint-teal','var(--teal)',sm=True)}
        <div style="font-size:11px;color:var(--ink-soft);line-height:1.4">
          4&nbsp;KB page size chosen deliberately over 2&nbsp;MB large pages, to
          avoid early reservation and system-wide fragmentation.</div>
      </div>
    </div>
  </div>
  {foot(8, 'VCMalloc &nbsp;·&nbsp; Interface')}
</div></div>'''


def s9_alloc():
    row = ''.join([
        kpi('+28%', 'Cycles saved vs. MIMalloc\nat 10,000 x 10,000', 'var(--teal)', 'var(--teal-xl)'),
        kpi('+16.5%', 'Cycles saved vs. Malloc\nat 10,000 x 10,000', 'var(--blue)', 'var(--blue-xl)'),
        kpi('1.53 GiB', 'VCMalloc peak memory -\nbelow Malloc, far below MIMalloc', 'var(--grn)', 'var(--grn-xl)'),
        kpi('0 bytes', 'Average fragmentation\nat every size tested', 'var(--pur)', 'var(--pur-xl)'),
    ])
    return f'''<div class="slide"><div class="pad">
  {head('Results  ·  basic operations', 'Allocation: faster and perfectly contiguous',
        'Windows 11 default Malloc vs. MIMalloc vs. VCMalloc on 2-D matrix allocation. Lower cycles and lower fragmentation are both better.')}
  <div style="display:grid;grid-template-columns:1fr 1fr;gap:22px;margin-top:20px;
       flex:1 1 auto;min-height:0">
    {chart_card('charts/alloc_cycles.png', 'CPU cycles per allocation', 'Fewer cycles = faster')}
    {chart_card('charts/frag_avg.png', 'Average fragmentation', 'Log scale &mdash; VCMalloc stays at zero')}
  </div>
  <div class="kpis" style="grid-template-columns:repeat(4,1fr);margin-top:20px">{row}</div>
  {foot(9, 'VCMalloc &nbsp;·&nbsp; Results', src='Source: Hadjadj et al., IEEE Trans. Computers, vol. 72, no. 12 (2023), Table V.')}
</div></div>'''


def s10_realloc():
    note = '''<div class="card fill-amb" style="height:214px;padding:14px 18px">
      <div class="row" style="gap:11px">''' + badge('info', 'tint-amb', 'var(--amb-d)', sm=True) + '''
        <span style="font-size:13px;font-weight:700;color:var(--ink)">Data-integrity note</span></div>
      <div style="font-size:11px;color:var(--ink-soft);line-height:1.45;margin-top:10px">
        The 10Kx10K row in the paper's Table&nbsp;VI prints <b>2.98M</b> cycles for
        VCMalloc, inconsistent with the <b>2.53B</b> figure in Table&nbsp;V for the
        same matrix. Plotting it would imply a 1,000x speed-up that cannot be real, so
        that group is <b>excluded</b> from the cycle chart. Its fragmentation columns
        are coherent and are shown opposite.</div>
    </div>'''
    ret = ('<div class="card" style="padding:14px 20px;display:flex;align-items:center;gap:16px">'
           '<span style="width:22px;height:22px;display:block;flex:0 0 auto">'
           + icon('trash', 'var(--amb)')
           + '</span><div style="font-size:12.5px;color:var(--ink-soft);line-height:1.45">'
             'Memory <b>not</b> returned to the OS at 10,000&nbsp;x&nbsp;10,000: '
             'Malloc holds on to <b>0.78%</b>, MIMalloc to <b>99.70%</b> '
             '&mdash; VCMalloc releases <b>99.36%</b>.</div></div>')
    return f'''<div class="slide"><div class="pad">
  {head('Results  ·  resizing and release', 'Reallocation and deallocation',
        'Remapping keeps growth cheap; releasing returns memory far more completely than MIMalloc.')}
  <div style="display:grid;grid-template-columns:1fr 1fr 1fr;gap:18px;margin-top:16px;
       flex:1 1 auto;min-height:0">
    {chart_card('charts/realloc_cycles.png', 'Reallocation cycles', 'Ordered &mdash; lower is better')}
    {chart_card('charts/realloc_frag.png', 'Reallocation fragmentation', 'VCMalloc stays lowest')}
    {chart_card('charts/freed_pct.png', 'Memory returned on free', 'f_prctg &mdash; handed back to the OS')}
  </div>
  <div style="display:grid;grid-template-columns:1fr 1fr;gap:20px;margin-top:14px;
       flex:0 0 auto">
    {kpi('+25.7%', 'Fewer reallocation cycles than Malloc at 2,000 x 1,000', 'var(--teal)', 'var(--teal-xl)')}
    {ret}
  </div>
  <div class="card fill-amb" style="margin-top:12px;padding:11px 18px;flex:0 0 auto;
       display:flex;gap:12px;align-items:center">
    {badge('info','tint-amb','var(--amb-d)',sm=True)}
    <div style="font-size:11px;color:var(--ink-soft);line-height:1.4">
      <b>Data-integrity note:</b> the 10Kx10K row in the paper's Table&nbsp;VI
      prints <b>2.98M</b> cycles for VCMalloc, inconsistent with the <b>2.53B</b>
      figure in Table&nbsp;V for the same matrix. That group is excluded from the
      cycle chart; its fragmentation columns are coherent and are shown above.</div>
  </div>
  {foot(10, 'VCMalloc &nbsp;·&nbsp; Results', src='Source: Hadjadj et al., IEEE Trans. Computers, vol. 72, no. 12 (2023), Tables VI-VII.')}
</div></div>'''


def s11_matmul():
    tiles = [('+31%', 'Peak gain at M = 8,000', 'var(--teal)', 'bolt', 'tint-teal'),
             ('+28%', 'Cycle efficiency reported', 'var(--blue)', 'gauge', 'tint-blue'),
             ('&minus;L3', 'Far fewer L3 cache misses', 'var(--pur)', 'cache', 'tint-pur'),
             ('Lower', 'Energy consumption', 'var(--grn)', 'power', 'tint-grn')]
    tt = ''
    for v, lab, c, ic, tn in tiles:
        tt += f'''<div class="card" style="padding:14px 18px;display:flex;
          gap:14px;align-items:center">
          {badge(ic, tn, c, sm=True)}
          <div><div style="font-size:21px;font-weight:700;color:{c};line-height:1">{v}</div>
          <div style="font-size:11.5px;font-weight:700;color:var(--ink);margin-top:5px">{lab}</div></div>
        </div>'''
    return f'''<div class="slide"><div class="pad">
  {head('Results  ·  application impact', 'Matrix multiplication: where contiguity pays',
        'C = A x B, naive triple loop, square matrices from 4,000 to 8,000. VCMalloc is the only allocator that can keep the matrices contiguous.')}
  <div style="display:grid;grid-template-columns:1fr 330px;gap:24px;margin-top:20px;
       flex:1 1 auto;min-height:0">
    {chart_card('charts/matmul.png', 'Relative optimisation vs. Malloc', 'CPU cycles &mdash; higher is better')}
    <div style="display:flex;flex-direction:column;gap:12px">{tt}</div>
  </div>
  <div class="card fill-teal" style="margin-top:20px;padding:16px 22px;display:flex;
       gap:16px;align-items:center">
    {badge('lightbulb','tint-teal','var(--teal-d)')}
    <div style="font-size:12.5px;color:var(--ink-soft);line-height:1.45">
      Because VCMalloc keeps the matrices contiguous, it supports two access models
      &mdash; indexed 2-D pointers or an affine 1-D vector. MIMalloc and Malloc can
      only do the former.</div>
  </div>
  {foot(11, 'VCMalloc &nbsp;·&nbsp; Results', src='Source: Hadjadj et al., IEEE Trans. Computers, vol. 72, no. 12 (2023), Figs. 4-7.')}
</div></div>'''


def s12_spec():
    row = ''.join([
        kpi('&minus;1.2%', 'Cycles vs. Malloc\non omnetpp_r', 'var(--teal)', 'var(--teal-xl)'),
        kpi('&minus;2.5%', 'Cycles vs. Malloc\non ldecod_r', 'var(--blue)', 'var(--blue-xl)'),
        kpi('+0.8%', 'MIMalloc advantage over Malloc,\nas reported in the literature', 'var(--amb)', 'var(--amb-xl)'),
        kpi('&minus;14%', 'MIMalloc memory penalty\non omnetpp_r', 'var(--red)', 'var(--red-xl)'),
    ])
    return f'''<div class="slide"><div class="pad">
  {head('Results  ·  real-world applications', 'SPEC CPU 2017: the best compromise',
        'Three applications from the SPEC CPU 2017 suite. MIMalloc edges out Malloc on speed but costs far more memory; VCMalloc lands in between without the overhead.')}
  <div style="display:grid;grid-template-columns:1fr 1fr;gap:22px;margin-top:20px;
       flex:1 1 auto;min-height:0">
    {chart_card('charts/spec_cycles.png', 'CPU cycles', 'omnetpp_r &nbsp;·&nbsp; ldecod_r &nbsp;·&nbsp; ImageValidator')}
    {chart_card('charts/spec_mem.png', 'Resident memory  (log scale)', 'MIMalloc costs 18% more on ldecod, 20% more on omnetpp')}
  </div>
  <div class="kpis" style="grid-template-columns:repeat(4,1fr);margin-top:20px">{row}</div>
  {foot(12, 'VCMalloc &nbsp;·&nbsp; Results', src='Source: Hadjadj et al., IEEE Trans. Computers, vol. 72, no. 12 (2023), Table VIII.')}
</div></div>'''


def s13_tradeoff():
    v = [('MIMalloc', 'var(--blue)', 'Low fragmentation, but up to 225% more memory. Fast, and expensive.'),
         ('Malloc', 'var(--amb)', 'Reasonable memory, but the worst fragmentation &mdash; up to 792 MiB average.'),
         ('VCMalloc', 'var(--teal)', 'Malloc-like memory with near-zero fragmentation. The balanced option.')]
    vv = ''
    for name, c, txt in v:
        vv += f'''<div class="card left-accent" style="--accent:{c};padding:15px 20px">
          <div style="font-size:15px;font-weight:700;color:{c}">{name}</div>
          <div style="font-size:11.5px;color:var(--ink-soft);line-height:1.45;margin-top:6px">{txt}</div>
        </div>'''
    return f'''<div class="slide"><div class="pad">
  {head('Synthesis', 'The trade-off, in one picture',
        'Fragmentation against memory footprint, across all three structure sizes. The lower-left corner is ideal.')}
  <div style="display:grid;grid-template-columns:1fr 330px;gap:24px;margin-top:20px;
       flex:1 1 auto;min-height:0">
    {chart_card('charts/mem_vs_frag.png', 'Memory footprint vs. average fragmentation', 'Each point is one allocator at one matrix size')}
    <div style="display:flex;flex-direction:column;gap:12px">{vv}</div>
  </div>
  <div class="card fill-ink" style="margin-top:20px;padding:16px 22px;display:flex;
       gap:16px;align-items:center">
    {badge('scale_balance','tint-ink','var(--teal-l)')}
    <div style="font-size:12.5px;color:var(--dtxt);line-height:1.45">
      VCMalloc does not win every single row. Its advantage is that it is the only
      allocator that is never the worst choice on either axis.</div>
  </div>
  {foot(13, 'VCMalloc &nbsp;·&nbsp; Synthesis', src='Source: Hadjadj et al., IEEE Trans. Computers, vol. 72, no. 12 (2023), Tables V-VIII.')}
</div></div>'''


def s14_limits():
    items = [
        ('thread_risk', 'No concurrency', 'var(--red)', 'tint-red',
         'The current version implements no concurrent allocation strategy. Multi-threaded applications must add their own synchronisation.'),
        ('code', 'Not a drop-in malloc', 'var(--amb)', 'tint-amb',
         'The hypercontainer argument changes the call signature, so existing source must be adapted. On large codebases this is slow and error-prone.'),
        ('pinned', 'Page size is 4 KB', 'var(--pur)', 'tint-pur',
         'Large 2 MB pages would cut TLB pressure, but must be reserved at boot. VCMalloc trades TLB efficiency for predictable fragmentation.'),
        ('measure', 'Evaluation is narrow', 'var(--blue)', 'tint-blue',
         'Results come from micro-benchmarks, matrix multiplication and three SPEC CPU 2017 applications on one Windows 11 workstation.'),
    ]
    cards = ''
    for ic, name, c, tn, desc in items:
        cards += f'''<div class="card" style="flex:1;padding:0;position:relative">
          <div style="height:4px;background:{c};border-radius:14px 14px 0 0"></div>
          <div style="padding:22px 20px">
            {badge(ic, tn, c, lg=True)}
            <div style="font-size:16px;font-weight:700;color:var(--ink);margin-top:18px">{name}</div>
            <div style="font-size:12px;color:var(--ink-soft);line-height:1.5;margin-top:12px">{desc}</div>
          </div>
        </div>'''
    return f'''<div class="slide"><div class="pad">
  {head('Limitations', 'What VCMalloc does not do yet',
        'The results are strong, but the design carries real costs that a production deployment must weigh.', accent='amb')}
  <div style="display:flex;gap:22px;margin-top:24px">{cards}</div>
  <div class="card fill-blue" style="margin-top:24px;padding:18px 24px;display:flex;
       gap:18px;align-items:flex-start">
    {badge('info','tint-blue','var(--blue-d)')}
    <div style="font-size:12.5px;color:var(--ink-soft);line-height:1.5">
      Adopting VCMalloc is a deliberate trade: you give up a transparent malloc
      interface and gain an explicit, user-controlled memory layout. That suits
      workloads &mdash; imaging, vision, linear algebra &mdash; where contiguity is
      worth the integration cost.</div>
  </div>
  {foot(14, 'VCMalloc &nbsp;·&nbsp; Limitations')}
</div></div>'''


def s15_future():
    items = [('expand', 'Other operating systems', 'var(--teal)', 'tint-teal',
              'Port the allocator beyond Windows, where the AWE API is specific to this platform.'),
             ('integrate', 'Physically contiguous backing', 'var(--blue)', 'tint-blue',
              'Combine virtual contiguity with a physically contiguous allocator such as GCMA, removing the last remapping step.'),
             ('puzzle', 'Irregular data structures', 'var(--pur)', 'tint-pur',
              'Design layouts for non-rectangular structures &mdash; graphs, sparse matrices, trees &mdash; that fit the container model.'),
             ('gpu', 'GPGPU workloads', 'var(--amb)', 'tint-amb',
              'Extend the techniques to GPU memory, where contiguity is already a hard requirement.')]
    cards = ''
    for i, (ic, name, c, tn, desc) in enumerate(items):
        arrow = ('<div style="align-self:center;width:0;height:0;border-top:7px solid transparent;'
                 'border-bottom:7px solid transparent;border-left:9px solid var(--hair)"></div>')
        cards += f'''<div class="card" style="flex:1;padding:24px 20px;text-align:center">
          <div style="display:flex;justify-content:center">{badge(ic, tn, c, lg=True)}</div>
          <div style="font-size:16px;font-weight:700;color:var(--ink);margin-top:18px;
               line-height:1.25">{name}</div>
          <div style="font-size:12px;color:var(--ink-soft);line-height:1.5;margin-top:12px;
               text-align:left">{desc}</div>
        </div>'''
        if i < len(items) - 1:
            cards += arrow
    return f'''<div class="slide"><div class="pad">
  {head('Outlook', 'Where VCMalloc goes next',
        'The authors point to four concrete extensions of the current design.', accent='pur')}
  <div style="display:flex;gap:16px;margin-top:26px;align-items:stretch">{cards}</div>
  <div class="card fill-ink" style="margin-top:26px;padding:20px 26px;display:flex;
       gap:22px;align-items:center">
    {badge('repo','tint-ink','var(--teal-l)')}
    <div>
      <div style="font-size:16px;font-weight:700;color:#fff">Open source and reusable today</div>
      <div style="font-size:12px;color:var(--dtxt2);margin-top:6px;line-height:1.45">
        About 1,500 lines of C and C++. Compile it as an object file or link it as a library.</div>
    </div>
    <div class="chip mono" style="margin-left:auto;background:var(--d3);
         border:1px solid var(--dline);color:var(--teal-l);font-size:12px;padding:9px 18px">
      github.com/ycinhdj/vcmalloc</div>
  </div>
  {foot(15, 'VCMalloc &nbsp;·&nbsp; Outlook')}
</div></div>'''


def s16_close():
    pts = [('Zero fragmentation', 'perfectly contiguous allocations at every size tested', 'verified'),
           ('Faster operations', 'up to 28% faster allocation, 26% faster reallocation', 'bolt'),
           ('Bounded memory', "close to Malloc, avoiding MIMalloc's 225% overhead", 'scale_balance'),
           ('Real-world gains', 'up to 31% on matrix multiplication and better on SPEC CPU', 'graph_up')]
    ps = ''
    for t, d, ic in pts:
        ps += f'''<div style="display:flex;gap:13px;align-items:flex-start;width:420px">
          <span style="width:24px;height:24px;display:block;flex:0 0 auto">{icon(ic,'var(--teal-l)')}</span>
          <div><div style="font-size:15px;font-weight:700;color:#fff">{t}</div>
          <div style="font-size:12px;color:var(--dtxt2);margin-top:5px;line-height:1.45">{d}</div></div>
        </div>'''
    bars = ''.join(
        f'<div style="position:absolute;left:{9.90 + (i % 4) * 0.62}in;'
        f'top:{1.30 + (i // 4) * 2.10}in;width:0.28in;height:{(0.9 + (i % 4) * 0.42)}in;'
        f'border-radius:5px;background:var(--teal);opacity:{0.22 + i * 0.07}"></div>'
        for i in range(8))
    return f'''<div class="slide dark">
  {bars}
  <div style="position:absolute;right:96px;bottom:74px;width:92px;height:92px">
    {icon('verified','rgba(92,211,206,.55)')}</div>
  <div style="position:absolute;left:70px;top:150px;width:1000px">
    <div class="eyebrow" style="color:var(--teal-l)">In summary</div>
    <div style="font-size:44px;font-weight:600;color:#fff;line-height:1.2;margin-top:18px;
         letter-spacing:-.02em">Contiguity is worth<br/>explicit control</div>
    <div style="height:3px;width:74px;background:var(--teal);margin:26px 0 30px"></div>
    <div style="display:grid;grid-template-columns:1fr 1fr;gap:26px 40px">{ps}</div>
    <div style="margin-top:46px;padding-top:16px;border-top:1px solid var(--dline);
         display:flex;justify-content:space-between;font-size:11px;color:var(--muted)">
      <span>Hadjadj, Zouaoui, Taleb, El Bahri, Chikr El Mezouar, Mazari &nbsp;·&nbsp;
            IEEE Trans. Computers 72(12):3431-3442 &nbsp;·&nbsp; 2023</span>
      <span style="font-weight:700">16 / 16</span>
    </div>
  </div>
</div>'''


# =====================================================================
#  ASSEMBLE
# =====================================================================
SLIDES = [s1_title, s2_summary, s3_problem, s4_contiguity, s5_model, s6_flow,
          s7_realloc, s8_api, s9_alloc, s10_realloc, s11_matmul, s12_spec,
          s13_tradeoff, s14_limits, s15_future, s16_close]


def build_html(path='deck.html'):
    body = '\n'.join(fn() for fn in SLIDES)
    html = f'''<!doctype html><html><head><meta charset="utf-8">
<title>VCMalloc</title><style>{CSS}</style></head><body>{body}</body></html>'''
    with open(path, 'w', encoding='utf-8') as f:
        f.write(html)
    print('wrote', path, os.path.getsize(path), 'bytes')
    return path


if __name__ == '__main__':
    build_html()



