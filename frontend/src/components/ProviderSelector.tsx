// Provider selector — minimal, no mock
interface ProviderSelectorProps {
  providers: Array<{ id: string; name: string }>;
  selected: string;
  onChange: (id: string) => void;
  onOpenSettings?: () => void;
}

export default function ProviderSelector({ providers, selected, onChange, onOpenSettings }: ProviderSelectorProps) {
  const visible = providers.filter(p => p.id !== 'mock');
  if (visible.length === 0) {
    return onOpenSettings ? (
      <button
        onClick={onOpenSettings}
        className="btn-ghost"
        style={{
          fontSize: '11px',
          padding: '4px 10px',
          borderRadius: '999px',
          border: '1px dashed var(--line)',
          color: 'var(--muted)',
          display: 'flex',
          alignItems: 'center',
          gap: '6px'
        }}
      >
        <span>+ Configure Provider</span>
      </button>
    ) : null;
  }

  const current = visible.find(p => p.id === selected) || visible[0];
  return (
    <div style={{display:'flex', alignItems:'center', gap:'8px'}}>
      <span className="mono" style={{fontSize:'11px', color:'var(--muted)'}}>Model</span>
      <div style={{position:'relative'}}>
        <select
          value={current.id}
          onChange={e=>onChange(e.target.value)}
          style={{minWidth:'140px', padding:'8px 28px 8px 12px', fontSize:'13px', borderRadius:'999px'}}
        >
          {visible.map(p=> <option key={p.id} value={p.id}>{p.name}</option>)}
        </select>
        <svg width="12" height="12" viewBox="0 0 12 12" style={{position:'absolute', right:'10px', top:'50%', transform:'translateY(-50%)', pointerEvents:'none'}} fill="none" stroke="var(--muted)" strokeWidth="1.3"><path d="M2.5 4 L6 7.5 L9.5 4"/></svg>
      </div>
    </div>
  );
}
