import React, { useState, useRef, useEffect } from 'react';
import ApertureLogo from './ApertureLogo';
import { PlusIcon, CheckIcon, PanelIcon, SettingsIcon, EditIcon, TrashIcon } from './icons';

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
        className="session-row-wrap"
        style={{ position: 'relative' }}
      >
        <button
          type="button"
          onClick={() => onSelect(id)}
          aria-current={selected ? 'true' : undefined}
          className="session-row"
          style={{
            width: '100%',
            padding: '7px 10px 7px 10px',
            display: 'flex',
            alignItems: 'center',
            gap: '8px',
            background: selected ? 'var(--control-hover)' : 'transparent',
            color: selected ? 'var(--text)' : 'var(--muted)',
            border: 'none',
            borderRadius: '8px',
            cursor: 'pointer',
            transition: 'all .12s ease',
            fontFamily: 'inherit',
            textAlign: 'left',
            // Reserve room for the hover-only rename/delete actions overlay.
            paddingRight: '58px',
          }}
        >
          <span
            title={id}
            style={{
              fontSize: '13px',
              whiteSpace: 'nowrap',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              flex: 1,
              fontWeight: selected ? 500 : 400,
              color: selected ? 'var(--text)' : 'inherit',
            }}
          >
            {id}
          </span>
        </button>

        <div
          className="session-actions"
          style={{
            position: 'absolute',
            right: '8px',
            top: '50%',
            transform: 'translateY(-50%)',
            display: 'flex',
            alignItems: 'center',
            gap: '2px',
          }}
        >
          <button
            type="button"
            onClick={e => handleStartRename(e, id)}
            title="Rename chat"
            className="btn-ghost"
            style={{
              padding: '4px',
              borderRadius: '4px',
              color: 'var(--muted)',
              display: 'flex',
              alignItems: 'center',
              cursor: 'pointer',
              border: 'none',
            }}
          >
            <EditIcon size={12} />
          </button>
          <button
            type="button"
            onClick={e => handleStartDelete(e, id)}
            title="Delete chat"
            className="btn-ghost"
            style={{
              padding: '4px',
              borderRadius: '4px',
              color: 'var(--muted)',
              display: 'flex',
              alignItems: 'center',
              cursor: 'pointer',
              border: 'none',
            }}
          >
            <TrashIcon size={12} />
          </button>
        </div>
      </div>
    );

  };

  const groups = groupSessions(sessions);

  return (
    <aside style={{ flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0, background: 'var(--panel-2)' }}>
      {/* Brand & collapse header */}
      <div
        style={{
          height: 48,
          padding: '0 12px',
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          flexShrink: 0,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 8, paddingLeft: '4px' }}>
          <ApertureLogo size={20} />
          <span style={{ fontWeight: 600, fontSize: '14px', letterSpacing: '-0.02em', color: 'var(--text)' }}>
            Maverick
          </span>
        </div>

        <button
          className="btn-ghost btn-ico"
          onClick={onToggleSidebar}
          aria-label="Close sidebar"
          title="Close sidebar"
          style={{ color: 'var(--muted)', borderRadius: '8px', padding: '6px' }}
        >
          <PanelIcon size={16} />
        </button>
      </div>

      {/* New chat button */}
      <div style={{ padding: '4px 10px 8px' }}>
        <button
          type="button"
          onClick={onNew}
          style={{
            width: '100%',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            background: 'var(--panel)',
            border: '1px solid var(--line)',
            color: 'var(--text)',
            borderRadius: '8px',
            padding: '8px 12px',
            fontSize: '13px',
            fontWeight: 500,
            cursor: 'pointer',
            transition: 'all .12s ease',
          }}
          className="hover:bg-[var(--control-hover)]"
        >
          <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
            <PlusIcon size={14} />
            <span>New chat</span>
          </span>
        </button>
      </div>


      {/* Grouped history */}
      <div style={{ flex: 1, overflowY: 'auto', padding: '6px 8px 12px' }}>
        {groups.length === 0 ? (
          <div style={{ padding: '32px 12px', textAlign: 'center' }}>
            <div style={{ fontSize: '13px', color: 'var(--muted)' }}>
              No chats yet
            </div>
          </div>
        ) : (
          groups.map(group => (
            <div key={group.label} style={{ marginBottom: 16 }}>
              <div style={{ padding: '6px 10px 4px', fontSize: '11px', fontWeight: 600, color: 'var(--faint)', letterSpacing: '0.02em' }}>
                {group.label}
              </div>
              <div style={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
                {group.items.map(renderRow)}
              </div>
            </div>
          ))
        )}
      </div>

      {/* Footer — user + settings */}
      <div style={{ padding: '8px 10px', borderTop: '1px solid var(--line)', background: 'var(--panel-2)' }}>
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 10,
            padding: '6px 8px',
            borderRadius: 8,
          }}
          className="hover:bg-[var(--control-hover)] transition-colors"
        >
          <div
            style={{
              width: 28,
              height: 28,
              borderRadius: '50%',
              background: 'var(--panel)',
              color: 'var(--text)',
              border: '1px solid var(--line)',
              fontSize: 11,
              fontWeight: 600,
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              flexShrink: 0,
            }}
          >
            OP
          </div>
          <div style={{ minWidth: 0, flex: 1, lineHeight: 1.2 }}>
            <div style={{ fontSize: 13, fontWeight: 500, color: 'var(--text)' }}>Operator</div>
          </div>
          <button
            className="btn-ghost btn-ico"
            onClick={onOpenSettings}
            aria-label="Settings"
            title="Settings"
            style={{ color: 'var(--muted)', borderRadius: '6px', padding: '5px' }}
          >
            <SettingsIcon size={15} />
          </button>
        </div>
      </div>

      <style>{`
        .session-row-wrap .session-actions { opacity: 0; transition: opacity 0.12s ease-in-out; }
        .session-row-wrap:hover .session-actions,
        .session-row-wrap:focus-within .session-actions { opacity: 1; }
        .session-row:hover { background: var(--control-hover) !important; }
        .session-row:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
      `}</style>
    </aside>
  );
}

