import { useEffect, useRef, useState } from 'react';
import { ChevronIcon, CheckIcon, SparklesIcon } from './icons';

interface ModelPresetBadgeProps {
  providers: Array<{ id: string; name: string; model: string }>;
  selected: string;
  onOpenSettings?: () => void;
}

export default function ModelPresetBadge({ providers, selected, onOpenSettings }: ModelPresetBadgeProps) {
  const [open, setOpen] = useState(false);
  const [hovered, setHovered] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [open]);

  if (providers.length === 0) {
    return onOpenSettings ? (
      <button
        onClick={onOpenSettings}
        className="btn-ghost"
        style={{
          fontSize: '12px',
          padding: '6px 12px',
          borderRadius: '8px',
          border: '1px dashed var(--line)',
          color: 'var(--muted)',
          display: 'flex',
          alignItems: 'center',
          gap: '6px',
        }}
      >
        <span>+ Add Provider</span>
      </button>
    ) : null;
  }

  const current = providers.find(p => p.id === selected) || providers[0];

  return (
    <div ref={rootRef} style={{ position: 'relative', display: 'inline-flex' }}>
      <button
        onClick={() => setOpen(o => !o)}
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          gap: '6px',
          padding: '6px 10px',
          fontSize: '14px',
          fontWeight: 600,
          color: open ? 'var(--text)' : hovered ? 'var(--text)' : 'var(--muted)',
          background: open ? 'var(--control-hover)' : hovered ? 'var(--control-hover)' : 'transparent',
          border: 'none',
          borderRadius: '8px',
          cursor: 'pointer',
          transition: 'all .15s ease',
          maxWidth: '100%',
        }}
        aria-expanded={open}
        aria-label="Select model"
      >
        <span>{current.name}</span>
        <span style={{ fontSize: '12px', fontWeight: 400, color: 'var(--faint)' }}>{current.model}</span>
        <span style={{ flexShrink: 0, transition: 'transform .2s', transform: open ? 'rotate(180deg)' : 'none', color: 'var(--faint)' }}>
          <ChevronIcon size={12} />
        </span>
      </button>

      {open && (
        <div
          style={{
            position: 'absolute',
            top: 'calc(100% + 6px)',
            left: 0,
            minWidth: 240,
            maxWidth: 320,
            background: 'var(--panel)',
            border: '1px solid var(--line)',
            borderRadius: '12px',
            padding: '6px',
            zIndex: 60,
            backdropFilter: 'blur(16px)',
          }}
        >
          <div style={{ padding: '6px 8px 4px', fontSize: '11px', fontWeight: 500, color: 'var(--faint)', textTransform: 'uppercase', letterSpacing: '0.04em' }}>
            Model
          </div>
          {providers.map(p => {
            const isSelected = p.id === selected;
            return (
              <button
                key={p.id}
                onClick={() => {
                  window.dispatchEvent(new CustomEvent('model-select', { detail: { id: p.id } }));
                  setOpen(false);
                }}
                style={{
                  width: '100%',
                  textAlign: 'left',
                  display: 'flex',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  padding: '8px 10px',
                  borderRadius: '8px',
                  background: isSelected ? 'var(--control-hover)' : 'transparent',
                  border: 'none',
                  cursor: 'pointer',
                  transition: 'background .12s',
                  color: 'var(--text)',
                }}
              >
                <div style={{ display: 'flex', flexDirection: 'column', gap: '2px', minWidth: 0 }}>
                  <div style={{ display: 'flex', gap: '6px', alignItems: 'center' }}>
                    <SparklesIcon size={12} className="text-muted" />
                    <span style={{ fontWeight: 500, fontSize: '13px' }}>{p.name}</span>
                  </div>
                  <span className="mono" style={{ fontSize: '11px', color: 'var(--muted)', paddingLeft: '18px', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {p.model}
                  </span>
                </div>
                {isSelected && (
                  <span style={{ color: 'var(--accent)', flexShrink: 0, marginLeft: '8px' }}>
                    <CheckIcon size={14} />
                  </span>
                )}
              </button>
            );
          })}
          <div style={{ height: '1px', background: 'var(--line)', margin: '6px 0' }} />
          <button
            onClick={() => { onOpenSettings?.(); setOpen(false); }}
            style={{
              width: '100%',
              textAlign: 'left',
              padding: '7px 10px',
              borderRadius: '8px',
              border: 'none',
              background: 'transparent',
              cursor: 'pointer',
              color: 'var(--muted)',
              fontSize: '12px',
              fontWeight: 500,
            }}
          >
            Manage providers…
          </button>
        </div>
      )}
    </div>
  );
}
