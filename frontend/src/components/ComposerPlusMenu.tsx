import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { AttachIcon, BrainIcon, ChevronIcon, FileIcon, FolderIcon, PlugIcon, PlusIcon } from './icons';

/// A file staged in the composer draft, serialized into the prompt on send.
export interface Attachment {
  id: string;
  name: string;
  size: number;
  content: string;
}

interface WorkspaceEntry {
  path: string;
  name: string;
  is_dir: boolean;
  size: number | null;
}

interface SkillRow {
  name: string;
  display_name?: string;
  description: string;
  enabled: boolean;
}

interface ComposerPlusMenuProps {
  /** Visible session — scopes file browsing to its workspace. */
  sessionId: string;
  onAttach: (a: Attachment) => void;
  onInsertSkill: (name: string) => void;
  onAddMcp: () => void;
  /** Cross-chat memory kill-switch for this session. */
  memoryEnabled: boolean;
  onToggleMemory: () => void;
}

const OS_FILE_CAP = 12 * 1024;
const OS_FILE_MAX_BYTES = 5 * 1024 * 1024;

export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

const mkId = () => `att-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;

const panelStyle: React.CSSProperties = {
  position: 'absolute',
  bottom: 'calc(100% + 10px)',
  left: 0,
  width: 300,
  background: 'var(--chip)',
  border: '1px solid var(--line-2)',
  borderRadius: 14,
  padding: 6,
  zIndex: 60,
};

export default function ComposerPlusMenu({ sessionId, onAttach, onInsertSkill, onAddMcp, memoryEnabled, onToggleMemory }: ComposerPlusMenuProps) {
  const [open, setOpen] = useState(false);
  const [view, setView] = useState<'root' | 'files' | 'skills'>('root');
  const [wsPath, setWsPath] = useState('');
  const [entries, setEntries] = useState<WorkspaceEntry[]>([]);
  const [skills, setSkills] = useState<SkillRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Close on Escape / outside click.
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

  const loadFiles = async (path: string) => {
    setView('files');
    setWsPath(path);
    setErr(null);
    setLoading(true);
    try {
      setEntries(await invoke<WorkspaceEntry[]>('list_workspace_files', { path, sessionId }));
    } catch (e) {
      setEntries([]);
      setErr(String(e));
    } finally {
      setLoading(false);
    }
  };

  const loadSkills = async () => {
    setView('skills');
    setErr(null);
    setLoading(true);
    try {
      setSkills(await invoke<SkillRow[]>('list_skills'));
    } catch (e) {
      setSkills([]);
      setErr(String(e));
    } finally {
      setLoading(false);
    }
  };

  const openRoot = () => {
    setView('root');
    setErr(null);
  };

  const pickOsFiles = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = Array.from(e.target.files ?? []);
    e.target.value = '';
    for (const f of files) {
      if (f.size > OS_FILE_MAX_BYTES) {
        alert(`${f.name}: larger than 5 MB`);
        continue;
      }
      // Read only the cap slice — never materialize a whole large file.
      let text = await f.slice(0, OS_FILE_CAP).text();
      if (text.includes('\u0000')) {
        alert(`${f.name}: binary file — only text files can be attached`);
        continue;
      }
      if (f.size > OS_FILE_CAP) text += '\n…[truncated at 12 KB]';
      onAttach({ id: mkId(), name: f.name, size: f.size, content: text });
    }
    setOpen(false);
  };

  const attachWorkspaceFile = async (entry: WorkspaceEntry) => {
    setLoading(true);
    setErr(null);
    try {
      const r = await invoke<{ path: string; content: string; size: number; truncated: boolean }>(
        'read_workspace_file',
        { path: entry.path, sessionId },
      );
      const content = r.truncated ? `${r.content}\n…[truncated at 24 KB]` : r.content;
      onAttach({ id: mkId(), name: r.path, size: r.size, content });
      setOpen(false);
    } catch (e) {
      setErr(String(e));
    } finally {
      setLoading(false);
    }
  };

  const upPath = wsPath.includes('/') ? wsPath.slice(0, wsPath.lastIndexOf('/')) : '';

  const menuItems = [
    {
      key: 'file',
      icon: <AttachIcon size={15} />,
      label: 'Attach file',
      hint: 'text files · 12 KB cap',
      onClick: () => fileInputRef.current?.click(),
    },
    {
      key: 'ws',
      icon: <FolderIcon size={15} />,
      label: 'Workspace files',
      hint: 'browse & attach',
      onClick: () => loadFiles(''),
    },
    {
      key: 'skills',
      icon: <BrainIcon size={15} />,
      label: 'Skills',
      hint: 'insert a skill reference',
      onClick: loadSkills,
    },
    {
      key: 'mcp',
      icon: <PlugIcon size={15} />,
      label: 'Add MCP server',
      hint: 'connect a tool server',
      onClick: () => {
        setOpen(false);
        onAddMcp();
      },
    },
  ];

  return (
    <div ref={rootRef} style={{ position: 'relative' }}>
      <input ref={fileInputRef} type="file" multiple style={{ display: 'none' }} onChange={pickOsFiles} />
      <button
        className="btn-ghost"
        aria-label="Add files, workspace, skills"
        aria-expanded={open}
        onClick={() => setOpen(o => !o)}
        style={{
          width: 28,
          height: 28,
          padding: 0,
          borderRadius: 8,
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'center',
          border: 'none',
          background: open ? 'var(--chip)' : 'transparent',
          color: 'var(--muted)',
          transition: 'all .15s',
          flexShrink: 0,
        }}
      >
        <span style={{ display: 'flex', transform: open ? 'rotate(45deg)' : 'none', transition: 'transform .18s' }}>
          <PlusIcon size={15} />
        </span>
      </button>

      {open && (
        <div style={panelStyle}>
          {view === 'root' && (
            <>
              {menuItems.map(m => (
                <button key={m.key} className="pm-item" onClick={m.onClick}>
                  <span style={{ display: 'flex', color: 'var(--muted)' }}>{m.icon}</span>
                  <span style={{ flex: 1, minWidth: 0 }}>
                    <div>{m.label}</div>
                    <div className="mono" style={{ fontSize: 10.5, color: 'var(--faint)', marginTop: 1 }}>{m.hint}</div>
                  </span>
                  <span style={{ display: 'flex', color: 'var(--faint)' }}>
                    <ChevronIcon size={13} className="pm-chev" />
                  </span>
                </button>
              ))}
              <button
                key="memory"
                className="pm-item"
                onClick={onToggleMemory}
                title={memoryEnabled ? 'Turn memory off for this chat' : 'Turn memory on for this chat'}
              >
                <span style={{ display: 'flex', color: 'var(--muted)' }}>
                  <BrainIcon size={15} />
                </span>
                <span style={{ flex: 1, minWidth: 0 }}>
                  <div>Memory</div>
                  <div className="mono" style={{ fontSize: 10.5, color: 'var(--faint)', marginTop: 1 }}>
                    {memoryEnabled ? 'on · remembers across chats' : 'off · this chat only'}
                  </div>
                </span>
                <span
                  className="mono"
                  style={{
                    fontSize: 10, padding: '2px 8px', borderRadius: 999, border: '1px solid',
                    ...(memoryEnabled
                      ? { color: 'var(--ok-text)', borderColor: 'var(--ok-border)' }
                      : { color: 'var(--faint)', borderColor: 'var(--line)' }),
                  }}
                >
                  {memoryEnabled ? 'On' : 'Off'}
                </span>
              </button>
            </>
          )}

          {(view === 'files' || view === 'skills') && (
            <>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '4px 6px 8px', borderBottom: '1px solid var(--line)', marginBottom: 4 }}>
                <button
                  className="pm-back"
                  aria-label="Back"
                  onClick={() => (view === 'files' && wsPath ? loadFiles(upPath) : openRoot())}
                >
                  <ChevronIcon size={13} className="pm-chev-left" />
                </button>
                <div style={{ minWidth: 0, flex: 1 }}>
                  <div style={{ fontSize: 12.5, fontWeight: 600 }}>{view === 'files' ? 'Workspace files' : 'Skills'}</div>
                  {view === 'files' && (
                    <div className="mono" style={{ fontSize: 10.5, color: 'var(--faint)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                      {wsPath || '/'}
                    </div>
                  )}
                </div>
              </div>

              {loading && (
                <div className="mono" style={{ padding: '10px 10px', fontSize: 11.5, color: 'var(--muted)' }}>loading…</div>
              )}
              {err && (
                <div className="mono" style={{ padding: '10px 10px', fontSize: 11, color: 'var(--error, #f87171)', wordBreak: 'break-word' }}>{err}</div>
              )}

              {!loading && view === 'files' && (
                <div style={{ maxHeight: 260, overflowY: 'auto' }}>
                  {entries.length === 0 && !err && (
                    <div className="mono" style={{ padding: '10px 10px', fontSize: 11.5, color: 'var(--faint)' }}>empty folder</div>
                  )}
                  {entries.map(e => (
                    <button
                      key={e.path}
                      className="pm-item"
                      onClick={() => (e.is_dir ? loadFiles(e.path) : attachWorkspaceFile(e))}
                      title={e.path}
                    >
                      <span style={{ display: 'flex', color: 'var(--muted)' }}>
                        {e.is_dir ? <FolderIcon size={14} /> : <FileIcon size={14} />}
                      </span>
                      <span style={{ flex: 1, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{e.name}</span>
                      {!e.is_dir && e.size != null && (
                        <span className="mono" style={{ fontSize: 10, color: 'var(--faint)' }}>{fmtBytes(e.size)}</span>
                      )}
                    </button>
                  ))}
                </div>
              )}

              {!loading && view === 'skills' && (
                <div style={{ maxHeight: 260, overflowY: 'auto' }}>
                  {skills.length === 0 && !err && (
                    <div className="mono" style={{ padding: '10px 10px', fontSize: 11.5, color: 'var(--faint)' }}>no skills installed</div>
                  )}
                  {skills.map(s => (
                    <button
                      key={s.name}
                      className="pm-item"
                      disabled={!s.enabled}
                      onClick={() => {
                        setOpen(false);
                        onInsertSkill(s.name);
                      }}
                    >
                      <span style={{ display: 'flex', color: 'var(--muted)' }}>
                        <BrainIcon size={14} />
                      </span>
                      <span style={{ flex: 1, minWidth: 0 }}>
                        <div style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{s.display_name || s.name}</div>
                        {s.description && (
                          <div className="mono" style={{ fontSize: 10.5, color: 'var(--faint)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                            {s.description}
                          </div>
                        )}
                      </span>
                    </button>
                  ))}
                </div>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
