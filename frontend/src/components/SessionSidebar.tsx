import React, { useState, useRef, useEffect } from 'react';
import ApertureLogo from './ApertureLogo';
import { PlusIcon, CheckIcon, PanelIcon, SettingsIcon } from './icons';

interface SessionSidebarProps {
  sessions: string[];
  currentSession: string;
  onSelect: (id: string) => void;
  onNew: () => void;
  onDelete: (id: string) => void;
  onRename: (oldId: string, newId: string) => void;
  onOpenSettings: () => void;
  onToggleSidebar: () => void;
}

/// Group sessions by recency. Session ids embed a millisecond timestamp
/// (`session-<ms>`); anything unparsable falls into "Earlier" — no
/// fabricated grouping.
function groupSessions(sessions: string[]): { label: string; items: string[] }[] {
  const today: string[] = [];
  const week: string[] = [];
  const earlier: string[] = [];
  const dayMs = 86_400_000;
  for (const id of sessions) {
    const m = /^session-(\d+)$/.exec(id);
    if (m) {
      const ageDays = (Date.now() - Number(m[1])) / dayMs;
      if (ageDays < 1) today.push(id);
      else if (ageDays < 7) week.push(id);
      else earlier.push(id);
    } else {
      earlier.push(id);
    }
  }
  return [
    { label: 'Today', items: today },
    { label: 'Previous 7 days', items: week },
    { label: 'Earlier', items: earlier },
  ].filter(g => g.items.length > 0);
}

export default function SessionSidebar({
  sessions,
  currentSession,
  onSelect,
  onNew,
  onDelete,
  onRename,
  onOpenSettings,
  onToggleSidebar,
}: SessionSidebarProps) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editName, setEditName] = useState('');
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (editingId && inputRef.current) {
      inputRef.current.focus();
      inputRef.current.select();
    }
  }, [editingId]);

  const handleStartRename = (e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    setConfirmDeleteId(null);
    setEditingId(id);
    setEditName(id);
  };

  const handleSaveRename = (e?: React.MouseEvent | React.FormEvent) => {
    if (e) e.stopPropagation();
    if (!editingId) return;
    const trimmed = editName.trim();
    if (trimmed && trimmed !== editingId) {
      onRename(editingId, trimmed);
    }
    setEditingId(null);
  };

  const handleCancelRename = (e?: React.MouseEvent) => {
    if (e) e.stopPropagation();
    setEditingId(null);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      handleSaveRename();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      handleCancelRename();
    }
  };

  const handleStartDelete = (e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    setEditingId(null);
    setConfirmDeleteId(id);
  };

  const handleConfirmDelete = (e: React.MouseEvent, id: string) => {
    e.stopPropagation();
    onDelete(id);
    setConfirmDeleteId(null);
  };

  const handleCancelDelete = (e: React.MouseEvent) => {
    e.stopPropagation();
    setConfirmDeleteId(null);
  };

  const renderRow = (id: string) => {
    const selected = id === currentSession;
    const isEditing = editingId === id;
    const isConfirmingDelete = confirmDeleteId === id;

    if (isEditing) {
      return (
        <div
          key={id}
          style={{
            padding: '6px 8px',
            borderRadius: '10px',
            background: 'var(--panel)',
            border: '1px solid var(--accent-border)',
            display: 'flex',
            alignItems: 'center',
            gap: '6px',
          }}
          onClick={e => e.stopPropagation()}
        >
          <input
            ref={inputRef}
            value={editName}
            onChange={e => setEditName(e.target.value)}
            onKeyDown={handleKeyDown}
            style={{
              flex: 1,
              minWidth: 0,
              background: 'var(--panel-2)',
              border: '1px solid var(--line)',
              color: 'var(--text)',
              borderRadius: '6px',
              padding: '4px 8px',
              fontSize: '12px',
              outline: 'none',
            }}
          />
          <button
            onClick={handleSaveRename}
            title="Save name"
            style={{
              padding: '4px',
              background: 'transparent',
              border: 'none',
              color: 'var(--accent-2)',
              cursor: 'pointer',
              display: 'flex',
              alignItems: 'center',
            }}
          >
            <CheckIcon size={13} />
          </button>
          <button
            onClick={handleCancelRename}
            title="Cancel"
            style={{
              padding: '4px',
              background: 'transparent',
              border: 'none',
              color: 'var(--muted)',
              cursor: 'pointer',
              display: 'flex',
              alignItems: 'center',
            }}
          >
            <svg width="13" height="13" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.8">
              <path d="M3 3 L9 9 M9 3 L3 9" />
            </svg>
          </button>
        </div>
      );
    }

    if (isConfirmingDelete) {
      return (
        <div
          key={id}
          style={{
            padding: '8px 10px',
            borderRadius: '10px',
            background: 'var(--error-bg)',
            border: '1px solid var(--error-border)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: '8px',
          }}
          onClick={e => e.stopPropagation()}
        >
          <span className="mono" style={{ fontSize: '11px', color: 'var(--error)', fontWeight: 600 }}>
            Delete chat?
          </span>
          <div style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
            <button
              onClick={e => handleConfirmDelete(e, id)}
              style={{
                padding: '3px 8px',
                background: 'var(--danger-solid)',
                border: 'none',
                color: 'var(--danger-solid-text)',
                borderRadius: '5px',
                fontSize: '11px',
                cursor: 'pointer',
                fontWeight: 600,
              }}
            >
              Delete
            </button>
            <button
              onClick={handleCancelDelete}
              style={{
                padding: '3px 8px',
                background: 'var(--panel)',
                border: '1px solid var(--line)',
                color: 'var(--muted)',
                borderRadius: '5px',
                fontSize: '11px',
                cursor: 'pointer',
              }}
            >
              Cancel
            </button>
          </div>
        </div>
      );
    }

    return (
      <div
        key={id}
        onClick={() => onSelect(id)}
        className="session-row"
        style={{
          position: 'relative',
          padding: '8px 10px',
          display: 'flex',
          alignItems: 'center',
          gap: '8px',
          background: selected ? 'var(--row-selected)' : 'transparent',
          border: selected ? '1px solid var(--row-selected-line)' : '1px solid transparent',
          color: selected ? 'var(--text)' : 'var(--muted)',
          borderRadius: '10px',
          cursor: 'pointer',
          transition: 'background .15s',
        }}
      >
        {selected && (
          <span style={{ width: 6, height: 6, borderRadius: '50%', background: 'var(--accent)', flexShrink: 0 }} />
        )}
        <span
          title={id}
          className="mono"
          style={{
            fontSize: '12.5px',
            whiteSpace: 'nowrap',
            overflow: 'hidden',
            textOverflow: 'ellipsis',
            flex: 1,
            fontWeight: selected ? 500 : 400,
          }}
        >
          {id}
        </span>

        <div
          className="session-actions"
          style={{ display: 'flex', alignItems: 'center', gap: '2px', flexShrink: 0 }}
          onClick={e => e.stopPropagation()}
        >
          <button
            onClick={e => handleStartRename(e, id)}
            title="Rename chat"
            className="btn-ghost"
            style={{
              padding: '3px 4px',
              borderRadius: '5px',
              color: 'var(--muted)',
              display: 'flex',
              alignItems: 'center',
              cursor: 'pointer',
              border: 'none',
            }}
          >
            <svg width="12" height="12" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
              <path d="M9.5 2.5 L11.5 4.5 L4.5 11.5 H2.5 V9.5 L9.5 2.5 Z" />
            </svg>
          </button>
          <button
            onClick={e => handleStartDelete(e, id)}
            title="Delete chat"
            className="btn-ghost"
            style={{
              padding: '3px 4px',
              borderRadius: '5px',
              color: 'var(--muted)',
              display: 'flex',
              alignItems: 'center',
              cursor: 'pointer',
              border: 'none',
            }}
          >
            <svg width="12" height="12" viewBox="0 0 14 14" fill="none" stroke="currentColor" strokeWidth="1.3">
              <path d="M3 4.5 H11 M5.5 4.5 V2.5 H8.5 V4.5 M4 4.5 L4.8 11.5 H9.2 L10 4.5" />
            </svg>
          </button>
        </div>
      </div>
    );
  };

  const groups = groupSessions(sessions);

  return (
    <aside style={{ flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0 }}>
      {/* Brand header */}
      <div
        style={{
          height: 56,
          padding: '0 14px',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          borderBottom: '1px solid var(--line)',
          flexShrink: 0,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
          <ApertureLogo size={22} />
          <span style={{ fontFamily: 'var(--font-head)', fontWeight: 500, fontSize: 14, letterSpacing: '-0.01em' }}>
            Maverick
          </span>
          <span
            className="mono"
            style={{
              fontSize: 10,
              color: 'var(--accent-2)',
              background: 'var(--accent-dim)',
              border: '1px solid var(--accent-border)',
              padding: '1px 6px',
              borderRadius: 6,
              fontWeight: 600,
            }}
          >
            agent
          </span>
        </div>
        <button
          className="btn-ghost btn-ico"
          onClick={onToggleSidebar}
          aria-label="Collapse sidebar"
          title="Collapse sidebar"
        >
          <PanelIcon size={16} />
        </button>
      </div>

      {/* New chat */}
      <div style={{ padding: 12 }}>
        <button
          onClick={onNew}
          style={{
            width: '100%',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            background: 'var(--surface-quiet)',
            color: 'var(--text)',
            fontWeight: 500,
            borderRadius: '12px',
            padding: '10px 14px',
            fontSize: '13px',
          }}
        >
          <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <PlusIcon size={15} className="accent-ico" />
            Start new chat
          </span>
        </button>
      </div>

      {/* Grouped history */}
      <div style={{ flex: 1, overflowY: 'auto', padding: '0 8px 12px' }}>
        {groups.length === 0 ? (
          <div style={{ padding: '24px 12px', textAlign: 'center' }}>
            <div className="mono" style={{ fontSize: '12px', color: 'var(--muted)' }}>
              No history yet
            </div>
            <div style={{ marginTop: 6, fontSize: '12px', color: 'var(--muted)', opacity: 0.8 }}>
              Your chats will appear here.
            </div>
          </div>
        ) : (
          groups.map(group => (
            <div key={group.label} style={{ marginBottom: 14 }}>
              <div style={{ padding: '0 10px 4px', fontSize: 11, fontWeight: 500, color: 'var(--faint)' }}>
                {group.label}
              </div>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                {group.items.map(renderRow)}
              </div>
            </div>
          ))
        )}
      </div>

      {/* Footer — user + settings */}
      <div style={{ padding: 12, borderTop: '1px solid var(--line)', background: 'var(--panel-2)' }}>
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 10,
            padding: 6,
            borderRadius: 12,
          }}
        >
          <div
            style={{
              width: 32,
              height: 32,
              borderRadius: '50%',
              background: 'var(--avatar-bg)',
              color: 'var(--accent-2)',
              border: '1px solid rgba(29,78,216,0.5)',
              fontSize: 12,
              fontWeight: 600,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              flexShrink: 0,
            }}
          >
            OP
          </div>
          <div style={{ minWidth: 0, flex: 1, lineHeight: 1.3 }}>
            <div style={{ fontSize: 13, fontWeight: 500 }}>Operator</div>
            <div style={{ fontSize: 11, color: 'var(--muted)' }}>Local session</div>
          </div>
          <button
            className="btn-ghost btn-ico"
            onClick={onOpenSettings}
            aria-label="Settings"
            title="Settings"
          >
            <SettingsIcon size={16} />
          </button>
        </div>
      </div>

      <style>{`
        .accent-ico { color: var(--accent); }
        .session-row .session-actions { opacity: 0; transition: opacity 0.15s ease-in-out; }
        .session-row:hover .session-actions,
        .session-row:focus-within .session-actions { opacity: 1; }
        .session-row:hover { background: var(--row-hover) !important; }
      `}</style>
    </aside>
  );
}
