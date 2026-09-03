// Tool list — minimal, English, no mock
import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';

export default function ToolList({ isOpen, onClose, tools }: { isOpen: boolean; onClose: () => void; tools: string[] }) {
  const [toolDefs, setToolDefs] = useState<Array<{ name: string; description: string }>>([]);
  useEffect(() => {
    if (isOpen) {
      invoke<Array<{ name: string; description: string }>>('list_tools')
        .then(list => setToolDefs(list.filter(t => t.name !== 'test_tool')))
        .catch(()=>{});
    }
  }, [isOpen]);
  if (!isOpen) return null;
  const filtered = tools.filter(t=> t !== 'test_tool');
  if (filtered.length===0) return null;
  return (
    <div style={{
      position:'fixed', top:0, right:0, bottom:0, width:'320px',
      background:'var(--panel)',
      borderLeft:'1px solid var(--line)', zIndex:50,
      display:'flex', flexDirection:'column'
    }}>
      <div style={{padding:'16px', borderBottom:'1px solid var(--line)', display:'flex', justifyContent:'space-between', alignItems:'center'}}>
        <div>
          <div style={{fontWeight:600, fontSize:'13px'}}>Tools</div>
          <div className="mono" style={{fontSize:'11px', color:'var(--muted)'}}>{filtered.length} available • no mock</div>
        </div>
        <button className="btn-ghost btn-ico" onClick={onClose} aria-label="Close" style={{borderRadius:'999px'}}>
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5"><path d="M2 2 L10 10 M10 2 L2 10"/></svg>
        </button>
      </div>
      <div style={{flex:1, overflowY:'auto', padding:'12px', display:'flex', flexDirection:'column', gap:'8px'}}>
        {toolDefs.length===0 ? (
          <div className="mono" style={{color:'var(--muted)', fontSize:'12px', textAlign:'center', padding:'24px 0'}}>Loading…</div>
        ) : (
          toolDefs.map(tool => (
            <div key={tool.name} style={{padding:'12px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)'}}>
              <div style={{fontFamily:'var(--font-mono)', fontWeight:600, fontSize:'12px'}}>{tool.name}</div>
              <div style={{color:'var(--muted)', fontSize:'12px', lineHeight:1.5, marginTop:'4px'}}>{tool.description || 'Local tool'}</div>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
