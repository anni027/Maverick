// Provider selector — renders the Stitch-style model pill using the real
// selected provider + model (no mock labels). Dropdown lists every
// configured provider with its effective model slug.
interface ProviderSelectorProps {
  providers: Array<{ id: string; name: string; model: string }>;
  selected: string;
  onChange: (id: string) => void;
  onOpenSettings?: () => void;
}

export default function ProviderSelector({ providers, selected, onChange, onOpenSettings }: ProviderSelectorProps) {
  const visible = providers;
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
      <div style={{position:'relative'}}>
        <select
          value={current.id}
          onChange={e=>onChange(e.target.value)}
          style={{minWidth:'150px', padding:'7px 28px 7px 12px', fontSize:'13px', borderRadius:'999px', background:'var(--panel)', color:'var(--text)', border:'1px solid var(--line)'}}
        >
          {visible.map(p=> (
            <option key={p.id} value={p.id} title={p.model}>
              {p.name} — {p.model}
            </option>
          ))}
        </select>
        <svg width="12" height="12" viewBox="0 0 12 12" style={{position:'absolute', right:'10px', top:'50%', transform:'translateY(-50%)', pointerEvents:'none'}} fill="none" stroke="var(--muted)" strokeWidth="1.3"><path d="M2.5 4 L6 7.5 L9.5 4"/></svg>
      </div>
    </div>
  );
}