import React, { useState, useRef, useEffect } from 'react';

interface SessionSidebarProps {
  sessions: string[];
  currentSession: string;
  onSelect: (id: string) => void;
  onNew: () => void;
  onDelete: (id: string) => void;
  onRename: (oldId: string, newId: string) => void;
}

export default function SessionSidebar({
  sessions,
  currentSession,
  onSelect,
  onNew,
  onDelete,
  onRename,
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

  return (
    <aside style={{ flex: 1, display: 'flex', flexDirection: 'column', minHeight: 0 }}>
      {/* New chat button */}
      <div style={{ padding: '12px' }}>
        <button
          onClick={onNew}
          style={{
            width: '100%',
            display: 'flex',
            alignItems: 'center',
            gap: '8px',
            justifyContent: 'center',
            background: 'var(--text)',
            color: 'var(--bg)',
            borderColor: 'var(--text)',
            fontWeight: 550,
            borderRadius: '10px',
            padding: '10px',
          }}
        >
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.8">
            <path d="M6 2.5 V9.5 M2.5 6 H9.5" />
          </svg>
          New chat
        </button>
      </div>

      {/* Header */}
      <div style={{ padding: '0 14px 8px' }} className="mono">
        <div style={{ fontSize: '11px', color: 'var(--muted)', letterSpacing: '0.08em', textTransform: 'uppercase' }}>
          History ({sessions.length})
        </div>
      </div>

      {/* Session list */}
      <div
        style={{
          flex: 1,
          overflowY: 'auto',
          padding: '0 8px 12px',
          display: 'flex',
          flexDirection: 'column',
          gap: '2px',
        }}
      >
        {sessions.length === 0 ? (
          <div style={{ padding: '24px 12px', textAlign: 'center' }}>
            <div className="mono" style={{ fontSize: '12px', color: 'var(--muted)' }}>
              No history yet
            </div>
            <div style={{ marginTop: '6px', fontSize: '12px', color: 'var(--muted)', opacity: 0.8 }}>
              Your chats will appear here.
            </div>
          </div>
        ) : (
          sessions.map(id => {
            const selected = id === currentSession;
            const isEditing = editingId === id;
            const isConfirmingDelete = confirmDeleteId === id;

            if (isEditing) {
              return (
                <div
                  key={id}
                  style={{
                    padding: '6px 8px',
                    borderRadius: '8px',
                    background: 'var(--panel)',
                    border: '1px solid var(--rosso)',
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
                      background: 'var(--bg)',
                      border: '1px solid var(--line)',
                      color: 'var(--text)',
                      borderRadius: '5px',
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
                      color: '#22c55e',
                      cursor: 'pointer',
                      display: 'flex',
                      alignItems: 'center',
                    }}
                  >
                    <svg width="13" height="13" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.8">
                      <path d="M2.5 6.5 L4.5 8.5 L9.5 3.5" />
                    </svg>
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
                    borderRadius: '8px',
                    background: 'rgba(227,6,19,0.12)',
                    border: '1px solid rgba(227,6,19,0.3)',
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                    gap: '8px',
                  }}
                  onClick={e => e.stopPropagation()}
                >
                  <span className="mono" style={{ fontSize: '11px', color: '#ff8a80', fontWeight: 600 }}>
                    Delete chat?
                  </span>
                  <div style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
                    <button
                      onClick={e => handleConfirmDelete(e, id)}
                      style={{
                        padding: '3px 8px',
                        background: 'var(--rosso)',
                        border: 'none',
                        color: 'white',
                        borderRadius: '4px',
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
                        borderRadius: '4px',
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
                  background: selected ? 'var(--panel)' : 'transparent',
                  border: selected ? '1px solid var(--line)' : '1px solid transparent',
                  color: selected ? 'var(--text)' : 'var(--muted)',
                  borderRadius: '8px',
                  cursor: 'pointer',
                  transition: 'background .12s',
                }}
              >
                {/* Chat bubble icon */}
                <svg
                  width="13"
                  height="13"
                  viewBox="0 0 14 14"
                  fill="none"
                  stroke={selected ? 'var(--text)' : 'var(--muted)'}
                  strokeWidth="1.3"
                  style={{ flexShrink: 0 }}
                >
                  <path d="M3 3.5 H11 V10.5 H3 Z" />
                  <path d="M3 5.5 H11" />
                </svg>

                {/* Session title */}
                <span
                  title={id}
                  className="mono"
                  style={{
                    fontSize: '12px',
                    whiteSpace: 'nowrap',
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    flex: 1,
                    fontWeight: selected ? 500 : 400,
                  }}
                >
                  {id}
                </span>

                {/* Actions (Rename & Delete) */}
                <div
                  className="session-actions"
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    gap: '2px',
                    flexShrink: 0,
                  }}
                  onClick={e => e.stopPropagation()}
                >
                  <button
                    onClick={e => handleStartRename(e, id)}
                    title="Rename chat"
                    className="btn-ghost"
                    style={{
                      padding: '3px 4px',
                      borderRadius: '4px',
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
                      borderRadius: '4px',
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
          })
        )}
      </div>

      {/* Footer */}
      <div
        style={{
          padding: '12px',
          borderTop: '1px solid var(--line)',
          display: 'flex',
          alignItems: 'center',
          gap: '10px',
        }}
      >
        <div
          style={{
            width: '28px',
            height: '28px',
            borderRadius: '50%',
            background: 'var(--panel)',
            border: '1px solid var(--line)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
          }}
        >
          <span className="mono" style={{ fontSize: '10px', color: 'var(--muted)' }}>
            YOU
          </span>
        </div>
        <div style={{ minWidth: 0, flex: 1 }}>
          <div style={{ fontSize: '13px', fontWeight: 500 }}>You</div>
          <div className="mono" style={{ fontSize: '11px', color: 'var(--muted)' }}>
            Local • Minimal
          </div>
        </div>
      </div>

      <style>{`
        .session-row .session-actions {
          opacity: 0;
          transition: opacity 0.15s ease-in-out;
        }
        .session-row:hover .session-actions,
        .session-row:focus-within .session-actions {
          opacity: 1;
        }
        .session-row:hover {
          background: rgba(255, 255, 255, 0.04) !important;
        }
      `}</style>
    </aside>
  );
}
