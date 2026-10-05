import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { FolderIcon } from './icons';

interface WorkspaceInfo {
  path: string;
  is_default: boolean;
}

function basename(p: string): string {
  const parts = p.replace(/\\/g, '/').replace(/\/+$/, '').split('/');
  return parts.pop() || p;
}

/**
 * Session workspace chip for the app header: shows the folder this chat
 * works in, with a popover to browse (native dialog), type a path, use the
 * default, or pick from recents. Takes effect on the next tool call.
 */
export default function WorkspaceChip({ sessionId }: { sessionId: string }) {
  const [info, setInfo] = useState<WorkspaceInfo | null>(null);
  const [recents, setRecents] = useState<string[]>([]);
  const [openMenu, setOpenMenu] = useState(false);
  const [manual, setManual] = useState('');
  const [busy, setBusy] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  const load = async (sid: string) => {
    try {
      const [i, r] = await Promise.all([
        invoke<WorkspaceInfo>('get_session_workspace', { sessionId: sid }),
        invoke<string[]>('list_recent_workspaces'),
      ]);
      setInfo(i);
      setRecents(r.filter(p => p !== i.path));
      setManual('');
    } catch {
      setInfo(null);
    }
  };

  useEffect(() => {
    setOpenMenu(false);
    load(sessionId);
  }, [sessionId]);

  // Close on Escape / outside click.
  useEffect(() => {
    if (!openMenu) return;
    const onDown = (e: MouseEvent) => {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpenMenu(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpenMenu(false);
    };
    document.addEventListener('mousedown', onDown);
    document.addEventListener('keydown', onKey);
    return () => {
      document.removeEventListener('mousedown', onDown);
      document.removeEventListener('keydown', onKey);
    };
  }, [openMenu ]);

  const apply = async (path: string | null) => {
    setBusy(true);
    try {
      await invoke('set_session_workspace', { sessionId, path });
      await load(sessionId);
    } catch (e) {
      alert(String(e));
    } finally {
      setBusy(false);
    }
  };

  const browse = async () => {
    try {
      const picked = await open({ directory: true, multiple: false, title: 'Choose workspace folder' });
      if (typeof picked === 'string' && picked) await apply(picked);
    } catch (e) {
      alert(String(e));
    }
  };

  return (
    <div ref={rootRef} style={{ position: 'relative' }}>
      <button
        className="btn-ghost"
        onClick={() => setOpenMenu(o => !o)}
        title={info ? `Workspace: ${info.path}${info.is_default ? ' (default)' : ''}` : 'Workspace'}
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: '6px',
          padding: '5px 10px',
          borderRadius: '8px',
          color: 'var(--muted)',
          fontSize: '12px',
          maxWidth: '220px',
        }}
      >
        <span style={{ display: 'flex', flexShrink: 0 }}>
          <FolderIcon size={14} />
        </span>
        <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
          {info ? basename(info.path) : '…'}
        </span>
        {!info?.is_default && info && (
          <span className="mono" style={{ fontSize: '9px', color: 'var(--accent)', flexShrink: 0 }}>●</span>
        )}
      </button>

      {openMenu && (
        <div
          style={{
            position: 'absolute',
            top: 'calc(100% + 8px)',
            left: 0,
            width: '320px',
            background: 'var(--chip)',
            border: '1px solid var(--line-2)',
            borderRadius: '14px',
            padding: '10px',
            zIndex: 70,
            display: 'flex',
            flexDirection: 'column',
            gap: '8px',
            boxShadow: '0 12px 32px rgba(0,0,0,0.35)',
          }}
        >
          <div>
            <div className="mono" style={{ fontSize: '10px', color: 'var(--faint)', marginBottom: '4px' }}>
              THIS CHAT'S FOLDER{info && !info.is_default ? '' : ' (DEFAULT)'}
            </div>
            <div className="mono" style={{ fontSize: '11px', color: 'var(--text)', wordBreak: 'break-all', lineHeight: 1.5 }}>
              {info?.path ?? '…'}
            </div>
          </div>

          <button className="btn-accent" onClick={browse} disabled={busy} style={{ borderRadius: 999 }}>
            Browse…
          </button>

          <div style={{ display: 'flex', gap: '6px' }}>
            <input
              value={manual}
              onChange={e => setManual(e.target.value)}
              onKeyDown={e => {
                if (e.key === 'Enter' && manual.trim()) apply(manual.trim());
              }}
              placeholder="Or paste a folder path…"
              style={{ flex: 1, minWidth: 0, fontSize: '12px' }}
            />
            <button className="btn-ghost" onClick={() => manual.trim() && apply(manual.trim())} disabled={busy || !manual.trim()} style={{ borderRadius: 999 }}>
              Set
            </button>
          </div>

          {info && !info.is_default && (
            <button className="btn-ghost" onClick={() => apply(null)} disabled={busy} style={{ borderRadius: 999 }}>
              Use default workspace
            </button>
          )}

          {recents.length > 0 && (
            <div>
              <div className="mono" style={{ fontSize: '10px', color: 'var(--faint)', marginBottom: '4px' }}>RECENT</div>
              <div style={{ display: 'flex', flexDirection: 'column', gap: '2px', maxHeight: '160px', overflowY: 'auto' }}>
                {recents.map(r => (
                  <button
                    key={r}
                    className="pm-item"
                    onClick={() => apply(r)}
                    disabled={busy}
                    title={r}
                    style={{ fontSize: '12px' }}
                  >
                    <span style={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {basename(r)}
                    </span>
                    <span className="mono" style={{ fontSize: '10px', color: 'var(--faint)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', maxWidth: '160px' }}>
                      {r}
                    </span>
                  </button>
                ))}
              </div>
            </div>
          )}
          <div className="mono" style={{ fontSize: '10px', color: 'var(--faint)' }}>
            Applies to the next tool call in this chat.
          </div>
        </div>
      )}
    </div>
  );
}
