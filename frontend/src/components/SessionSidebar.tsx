// Sidebar — ChatGPT familiar, minimal
interface SessionSidebarProps {
  sessions: string[];
  currentSession: string;
  onSelect: (id: string) => void;
  onNew: () => void;
}

export default function SessionSidebar({ sessions, currentSession, onSelect, onNew }: SessionSidebarProps) {
  return (
    <aside style={{flex:1, display:'flex', flexDirection:'column', minHeight:0}}>
      <div style={{padding:'12px'}}>
        <button onClick={onNew} style={{width:'100%', display:'flex', alignItems:'center', gap:'8px', justifyContent:'center', background:'var(--text)', color:'var(--bg)', borderColor:'var(--text)'}}>
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.6"><path d="M6 2.5 V9.5 M2.5 6 H9.5"/></svg>
          New chat
        </button>
      </div>

      <div style={{padding:'0 12px 8px'}} className="mono">
        <div style={{fontSize:'11px', color:'var(--muted)', letterSpacing:'0.08em'}}>History</div>
      </div>

      <div style={{flex:1, overflowY:'auto', padding:'0 8px 12px', display:'flex', flexDirection:'column', gap:'4px'}}>
        {sessions.length === 0 ? (
          <div style={{padding:'24px 12px', textAlign:'center'}}>
            <div className="mono" style={{fontSize:'12px', color:'var(--muted)'}}>No history yet</div>
            <div style={{marginTop:'6px', fontSize:'13px', color:'var(--text)', opacity:0.7}}>Your conversations will appear here.</div>
          </div>
        ) : (
          sessions.map(id => {
            const selected = id === currentSession;
            return (
              <button
                key={id}
                onClick={() => onSelect(id)}
                style={{
                  textAlign:'left',
                  padding:'10px 12px',
                  display:'flex', alignItems:'center', gap:'10px',
                  background: selected ? 'var(--panel)' : 'transparent',
                  border: selected ? '1px solid var(--line)' : '1px solid transparent',
                  color: selected ? 'var(--text)' : 'var(--muted)',
                  borderRadius:'8px',
                  fontWeight: selected ? 500 : 400,
                }}
              >
                <svg width="14" height="14" viewBox="0 0 14 14" fill="none" stroke={selected ? 'var(--text)' : 'var(--muted)'} strokeWidth="1.3" style={{flexShrink:0}}>
                  <path d="M3 3.5 H11 V10.5 H3 Z"/><path d="M3 5.5 H11"/>
                </svg>
                <span className="mono" style={{fontSize:'12px', whiteSpace:'nowrap', overflow:'hidden', textOverflow:'ellipsis', flex:1}}>{id}</span>
              </button>
            );
          })
        )}
      </div>

      <div style={{padding:'12px', borderTop:'1px solid var(--line)', display:'flex', alignItems:'center', gap:'10px'}}>
        <div style={{width:'28px', height:'28px', borderRadius:'50%', background:'var(--panel)', border:'1px solid var(--line)', display:'flex', alignItems:'center', justifyContent:'center'}}>
          <span className="mono" style={{fontSize:'10px', color:'var(--muted)'}}>YOU</span>
        </div>
        <div style={{minWidth:0, flex:1}}>
          <div style={{fontSize:'13px', fontWeight:500}}>You</div>
          <div className="mono" style={{fontSize:'11px', color:'var(--muted)'}}>Local • Minimal</div>
        </div>
      </div>
    </aside>
  );
}
