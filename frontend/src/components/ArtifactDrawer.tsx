import { useEffect, useRef, useState } from 'react';
import type { Artifact } from '../types';
import MarkdownRenderer from './MarkdownRenderer';

interface ArtifactDrawerProps {
  artifacts: Artifact[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onClose: () => void;
}

function extFor(a: Artifact): string {
  return a.language === 'html' ? 'html' : a.language === 'svg' ? 'svg' : 'md';
}

/**
 * Right overlay drawer for the artifact viewer (Claude-style): Preview/Code
 * tabs, prev/next across the session's artifacts, copy + download. HTML/SVG
 * render in a sandboxed iframe (`allow-scripts`, no same-origin), Markdown
 * through our own renderer. Draggable left edge; Esc closes.
 */
export default function ArtifactDrawer({ artifacts, activeId, onSelect, onClose }: ArtifactDrawerProps) {
  const active = artifacts.find(a => a.id === activeId) ?? null;
  const index = active ? artifacts.findIndex(a => a.id === active.id) : -1;
  const [tab, setTab] = useState<'preview' | 'code'>('preview');
  const [copied, setCopied] = useState(false);

  // Fresh artifact → Preview tab, uncopied.
  useEffect(() => {
    setTab('preview');
    setCopied(false);
  }, [activeId]);

  // Esc closes (the composer + menu have their own handlers when open).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [onClose]);

  // Draggable left edge (sidebar pattern). Persists across opens.
  const [width, setWidth] = useState<number>(() => {
    try {
      const raw = localStorage.getItem('maverick.artifactWidth');
      const n = raw ? Number(raw) : NaN;
      if (Number.isFinite(n) && n >= 360) return Math.min(n, 900);
    } catch { /* corrupted — fall back */ }
    return 560;
  });
  const widthRef = useRef(width);
  widthRef.current = width;
  const startResize = (e: React.PointerEvent) => {
    e.preventDefault();
    const startX = e.clientX;
    const startW = widthRef.current;
    const onMove = (ev: PointerEvent) => {
      const w = Math.min(
        Math.max(200, window.innerWidth - 100),
        Math.max(360, Math.round(startW + (startX - ev.clientX))),
      );
      setWidth(w);
    };
    const onUp = () => {
      document.removeEventListener('pointermove', onMove);
      document.removeEventListener('pointerup', onUp);
      document.body.style.cursor = '';
      try {
        localStorage.setItem('maverick.artifactWidth', String(widthRef.current));
      } catch { /* private mode — session-only width */ }
    };
    document.addEventListener('pointermove', onMove);
    document.addEventListener('pointerup', onUp);
    document.body.style.cursor = 'ew-resize';
  };

  if (!active) return null;

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(active.code);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch { /* clipboard unavailable */ }
  };
  const download = () => {
    const base = active.title.split(/[\\/]/).pop()?.trim() || `artifact-${index + 1}`;
    const name = /\.[a-z0-9]+$/i.test(base) ? base : `${base}.${extFor(active)}`;
    const type = active.language === 'html' ? 'text/html' : active.language === 'svg' ? 'image/svg+xml' : 'text/markdown';
    const blob = new Blob([active.code], { type: `${type};charset=utf-8` });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = name;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 5000);
  };
  const go = (dir: -1 | 1) => {
    const next = artifacts[(index + dir + artifacts.length) % artifacts.length];
    if (next) onSelect(next.id);
  };

  return (
    <div
      style={{
        position: 'fixed',
        top: 0,
        right: 0,
        bottom: 0,
        width: `${width}px`,
        maxWidth: 'calc(100vw - 100px)',
        zIndex: 60,
        background: 'var(--panel)',
        borderLeft: '1px solid var(--line)',
        boxShadow: '-16px 0 48px rgba(0,0,0,0.4)',
        display: 'flex',
        flexDirection: 'column',
      }}
      role="dialog"
      aria-label="Artifact viewer"
    >
      {/* Drag handle */}
      <div
        onPointerDown={startResize}
        title="Drag to resize"
        style={{
          position: 'absolute',
          left: '-6px',
          top: 0,
          bottom: 0,
          width: '12px',
          cursor: 'ew-resize',
          zIndex: 2,
          touchAction: 'none',
        }}
      />
      {/* Title row */}
      <div style={{ display: 'flex', alignItems: 'center', gap: '8px', padding: '12px 12px 0' }}>
        <div className="mono" style={{ fontSize: '11px', color: 'var(--muted)', textTransform: 'uppercase', letterSpacing: '0.04em' }}>
          Artifact
        </div>
        <div style={{ flex: 1, minWidth: 0, fontSize: '13px', fontWeight: 600, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={active.title}>
          {active.title}
        </div>
        {artifacts.length > 1 && (
          <div className="mono" style={{ fontSize: '11px', color: 'var(--faint)', whiteSpace: 'nowrap' }}>
            {index + 1} of {artifacts.length}
          </div>
        )}
        <button className="btn-ghost btn-ico" onClick={onClose} aria-label="Close viewer" style={{ borderRadius: '999px', flexShrink: 0 }}>
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5"><path d="M2 2 L10 10 M10 2 L2 10" /></svg>
        </button>
      </div>
      {/* Toolbar row */}
      <div style={{ display: 'flex', alignItems: 'center', gap: '6px', padding: '10px 12px', borderBottom: '1px solid var(--line)' }}>
        <div style={{ display: 'flex', gap: '4px', background: 'var(--bg)', border: '1px solid var(--line)', borderRadius: 999, padding: '2px' }}>
          {(['preview', 'code'] as const).map(t => (
            <button
              key={t}
              onClick={() => setTab(t)}
              style={{
                padding: '4px 14px',
                borderRadius: 999,
                border: 'none',
                background: tab === t ? 'var(--text)' : 'transparent',
                color: tab === t ? 'var(--bg)' : 'var(--muted)',
                fontSize: '12px',
                cursor: 'pointer',
                textTransform: 'capitalize',
              }}
            >
              {t}
            </button>
          ))}
        </div>
        {artifacts.length > 1 && (
          <span style={{ display: 'inline-flex', gap: '2px' }}>
            <button className="btn-ghost btn-ico" onClick={() => go(-1)} aria-label="Previous artifact" style={{ borderRadius: '8px' }}>‹</button>
            <button className="btn-ghost btn-ico" onClick={() => go(1)} aria-label="Next artifact" style={{ borderRadius: '8px' }}>›</button>
          </span>
        )}
        <span style={{ flex: 1 }} />
        <button className="btn-ghost" onClick={copy} style={{ borderRadius: 999, fontSize: '12px' }}>
          {copied ? 'Copied!' : 'Copy'}
        </button>
        <button className="btn-ghost" onClick={download} style={{ borderRadius: 999, fontSize: '12px' }}>
          Download
        </button>
      </div>
      {/* Content */}
      <div style={{ flex: 1, minHeight: 0, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
        {tab === 'preview' ? (
          active.language === 'markdown' ? (
            <div style={{ flex: 1, overflowY: 'auto', padding: '16px 18px' }}>
              <MarkdownRenderer content={active.code} />
            </div>
          ) : (
            <iframe
              key={`${active.id}-${active.rev ?? 0}`}
              title={active.title}
              sandbox="allow-scripts"
              srcDoc={active.code}
              style={{ flex: 1, width: '100%', border: 'none', background: '#fff' }}
            />
          )
        ) : (
          <div style={{ flex: 1, minHeight: 0, overflow: 'auto', padding: '14px 16px' }}>
            {active.note && (
              <div className="mono" style={{ fontSize: '11px', color: 'var(--muted)', marginBottom: '10px' }}>
                {active.note}
              </div>
            )}
            <pre className="mono" style={{ margin: 0, fontSize: '12px', lineHeight: 1.6, whiteSpace: 'pre-wrap', wordBreak: 'break-word', color: 'var(--code-fg)' }}>
              <code>{active.code}</code>
            </pre>
          </div>
        )}
      </div>
    </div>
  );
}
