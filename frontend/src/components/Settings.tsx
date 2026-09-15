// Settings — minimal, English, no mock, no generic power badge
import React, { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

interface SettingsProps {
  isOpen: boolean; onClose: () => void;
  providers: Array<{ id: string; name: string }>;
  currentProvider: string; onProviderChange: (id: string) => void;
  onRefresh?: () => void;
}

const KNOWN_PROVIDERS: Array<{id:string; name:string; hint:string}> = [
  {id:'xai', name:'xAI', hint:'grok-4 • api.x.ai/v1'},
  {id:'openai', name:'OpenAI', hint:'gpt-4o • api.openai.com/v1'},
  {id:'anthropic', name:'Anthropic', hint:'claude-3.5 • api.anthropic.com/v1'},
];

// Preset custom OpenAI-compatible providers (one-click add).
const PRESET_PROVIDERS: Array<{id:string; name:string; base_url:string; model:string; kind:'openai'|'anthropic'; hint:string}> = [
  {id:'kilo', name:'Kilo AI Gateway', base_url:'https://api.kilo.ai/api/gateway', model:'kilo-auto/free', kind:'openai', hint:'500+ models • kilo.ai — free tier, no credits needed'},
];

export default function Settings({ isOpen, onClose, providers, currentProvider: _cp, onProviderChange, onRefresh }: SettingsProps) {
  const [apiKeys, setApiKeys] = useState<Record<string,string>>({});
  const [providerSettings, setProviderSettings] = useState<Record<string, {base_url?: string, model?: string, kind?: string}>>({});
  const [mcpServers, setMcpServers] = useState<Array<{name:string;transport:string;command?:string;args:string;url?:string;enabled:boolean}>>([]);
  const [uiConfig, setUiConfig] = useState({ theme:'dark', show_tool_calls:true, auto_scroll:true, compact_mode:false });
  const [defaultProvider, setDefaultProvider] = useState('');
  const [activeTab, setActiveTab] = useState<'providers'|'mcp'|'skills'|'ui'>('providers');
  const [skills, setSkills] = useState<Array<{name:string, description:string, path:string, scope:string, display_name?: string, enabled:boolean}>>([]);
  const [newSkill, setNewSkill] = useState({ name:'', content:'' });
  const [hubFetch, setHubFetch] = useState({ owner:'', name:'', version:'', url:'https://agentskills.io' });
  const [skillSearch, setSkillSearch] = useState('');
  const [loading, setLoading] = useState(true);
  const [newMcp, setNewMcp] = useState({ name:'', transport:'stdio', command:'', args:'', url:'' });
  const [newCustom, setNewCustom] = useState({ id:'', base_url:'', model:'', api_key:'', kind:'openai' as 'openai'|'anthropic' });
  const [saving, setSaving] = useState<string|null>(null);
  const [showAdvanced, setShowAdvanced] = useState<Record<string, boolean>>({});
  const [customKeys, setCustomKeys] = useState<Record<string, string>>({});
  const [kiloModels, setKiloModels] = useState<Array<{id:string; name:string; context_length?: number; is_free?: boolean}>>([]);
  const [kiloLoading, setKiloLoading] = useState(false);
  const [kiloError, setKiloError] = useState<string|null>(null);
  const [showKiloPicker, setShowKiloPicker] = useState<string|null>(null); // providerId or 'new'
  const [kiloSearch, setKiloSearch] = useState('');
  const [kiloFilter, setKiloFilter] = useState<'all'|'free'|'paid'>('all');

  useEffect(()=>{ if(isOpen) { loadConfig(); loadSkills(); } },[isOpen]);
  const loadSkills = async () => {
    try {
      const list = await invoke<Array<{name:string, description:string, path:string, scope:string, display_name?:string, enabled:boolean}>>('list_skills');
      setSkills(list);
    } catch(e){ console.error('list_skills failed', e); }
  };

  const fetchKiloModels = async (target: string) => {
    setShowKiloPicker(target); setKiloLoading(true); setKiloError(null);
    try {
      const list = await invoke<Array<{id:string; name:string; context_length?: number; is_free?: boolean}>>('list_kilo_models');
      setKiloModels(list);
    } catch(e){ setKiloError(String(e)); }
    finally{ setKiloLoading(false); }
  };
  const filteredKilo = kiloModels.filter(m=>{
    if(kiloFilter==='free' && m.is_free!==true) return false;
    if(kiloFilter==='paid' && m.is_free===true) return false;
    if(kiloSearch.trim()){
      const q=kiloSearch.toLowerCase();
      return m.id.toLowerCase().includes(q) || m.name.toLowerCase().includes(q);
    }
    return true;
  });

  const loadConfig = async () => {
    setLoading(true);
    try{
      const c=await invoke<any>('get_config');
      setApiKeys(Object.fromEntries(c.api_keys.map((k:string)=>[k,''])));
      setProviderSettings(c.provider_settings || {});
      if(c.default_provider) setDefaultProvider(c.default_provider);
      const arr=Object.entries(c.mcp_servers).map(([name,cfg]:[string,any])=>({name, transport:cfg.transport, command:cfg.command||'', args:cfg.args?.join(' ')||'', url:cfg.url||'', enabled:cfg.enabled}));
      setMcpServers(arr);
      setUiConfig(c.ui);
    }catch(e){ console.error(e)} finally{ setLoading(false)}
  };
  const handleApiKeyChange=(p:string,v:string)=> setApiKeys(prev=>({...prev,[p]:v}));
  const handleProviderSettingsChange = (id: string, field: 'base_url' | 'model' | 'kind', value: string) => {
    setProviderSettings(prev => ({ ...prev, [id]: { ...(prev[id]||{}), [field]: value }}));
  };
  const saveProviderConfig = async(p:string)=>{
    const k=apiKeys[p];
    const s=providerSettings[p]||{};
    // Need at least one of key, base_url, model to save
    if(!k?.trim() && !s.base_url?.trim() && !s.model?.trim()){
      alert('Enter at least API key, base URL, or model');
      return;
    }
    setSaving(p);
    try{
      if(k?.trim()){
        await invoke('set_api_key',{providerId:p, apiKey:k});
      }
      await invoke('set_provider_settings',{providerId:p, baseUrl: s.base_url?.trim() || null, model: s.model?.trim() || null, kind: s.kind?.trim() || null});
      onRefresh?.();
      setApiKeys(prev=> ({...prev, [p]: ''}));
    } catch(e){ alert(String(e))} finally{ setSaving(null)}
  };
  const removeApiKey=async(p:string)=>{
    if(!confirm(`Remove API key and settings for ${p}?`)) return;
    try{
      await invoke('remove_api_key',{providerId:p});
      await invoke('set_provider_settings',{providerId:p, baseUrl: null, model: null, kind: null});
      setApiKeys(prev=>{ const n={...prev}; delete n[p]; return n;});
      setProviderSettings(prev=>{ const n={...prev}; delete n[p]; return n;});
      onRefresh?.();
    }catch(e){ alert(String(e))}
  };
  const addCustomProvider = async()=>{
    const id=newCustom.id.trim().toLowerCase().replace(/[^a-z0-9_-]/g,'-');
    if(!id) { alert('Enter provider id (e.g. ollama)'); return; }
    if(!newCustom.base_url.trim() || !newCustom.model.trim()){ alert('Base URL and Model are required for custom provider'); return; }
    setSaving('custom');
    try{
      if(newCustom.api_key.trim()){
        await invoke('set_api_key',{providerId:id, apiKey:newCustom.api_key.trim()});
      }
      await invoke('set_provider_settings',{providerId:id, baseUrl:newCustom.base_url.trim(), model:newCustom.model.trim(), kind:newCustom.kind});
      onRefresh?.();
      setNewCustom({id:'', base_url:'', model:'', api_key:'', kind:'openai'});
      // Also load config to show it
      loadConfig();
    }catch(e){ alert(String(e))} finally{ setSaving(null)}
  };
  const handleDefaultProviderChange=async(e:React.ChangeEvent<HTMLSelectElement>)=>{
    const v=e.target.value||undefined; setDefaultProvider(e.target.value);
    try{ await invoke('set_default_provider',{providerId:v}); onProviderChange(e.target.value);}catch(e){ console.error(e)}
  };
  const addMcpServer=async()=>{
    if(!newMcp.name.trim()) return;
    if(newMcp.transport==='stdio' && !newMcp.command.trim()) return;
    if(newMcp.transport==='http' && !newMcp.url?.trim()) return;
    setSaving('mcp');
    try{
      await invoke('add_mcp_server_full',{name:newMcp.name, transport:newMcp.transport, command: newMcp.transport==='stdio'?newMcp.command:undefined, args:newMcp.args.split(' ').filter(a=>a.trim()), url: newMcp.transport==='http'?newMcp.url:undefined});
      loadConfig(); setNewMcp({name:'',transport:'stdio',command:'',args:'',url:''});
    }catch(e){ alert(String(e))} finally{ setSaving(null)}
  };
  const removeMcpServer=async(name:string)=>{
    if(!confirm(`Remove MCP server "${name}"?`)) return;
    try{ await invoke('remove_mcp',{name}); loadConfig(); }catch(e){ alert(String(e))}
  };
  const handleUiChange=async(k:string,v:boolean)=>{
    const nc={...uiConfig,[k]:v}; setUiConfig(nc);
    try{ await invoke('set_ui_config',{ui:nc}); }catch(e){ console.error(e)}
  };
  const handleInstallSkill = async()=>{
    if(!newSkill.name.trim() || !newSkill.content.trim()){ alert('Name and SKILL.md content required'); return; }
    setSaving('skill');
    try{
      await invoke('install_skill',{name:newSkill.name, content:newSkill.content, scope: null});
      setNewSkill({name:'', content:''});
      loadSkills();
    }catch(e){ alert(String(e))} finally{ setSaving(null)}
  };
  const handleRemoveSkill = async(name:string)=>{
    if(!confirm(`Remove skill "${name}"?`)) return;
    try{ await invoke('remove_skill',{name}); loadSkills(); }catch(e){ alert(String(e))}
  };
  const handleFetchHub = async()=>{
    if(!hubFetch.owner.trim() || !hubFetch.name.trim()){ alert('Owner and name required'); return; }
    setSaving('hub');
    try{
      await invoke('fetch_hub_skill',{hubUrl: hubFetch.url || 'https://agentskills.io', owner: hubFetch.owner, name: hubFetch.name, version: hubFetch.version.trim() || null});
      loadSkills();
      setHubFetch(prev=> ({...prev, owner:'', name:'', version:''}));
    }catch(e){ alert(String(e))} finally{ setSaving(null)}
  };

  if(!isOpen) return null;

  return (
    <div style={{position:'fixed', inset:0, background:'rgba(0,0,0,0.45)', backdropFilter:'blur(8px)', display:'flex', alignItems:'center', justifyContent:'center', zIndex:100, padding:'16px'}}>
      <div onClick={e=>e.stopPropagation()} style={{width:'100%', maxWidth:'640px', maxHeight:'90vh', display:'flex', flexDirection:'column', background:'var(--panel)', border:'1px solid var(--line)', borderRadius:'16px', overflow:'hidden'}}>
        <div style={{padding:'16px 20px', borderBottom:'1px solid var(--line)', display:'flex', justifyContent:'space-between', alignItems:'center'}}>
          <div>
            <div style={{fontWeight:600, fontSize:'14px'}}>Settings</div>
            <div className="mono" style={{fontSize:'11px', color:'var(--muted)'}}>Manage providers and MCP servers</div>
          </div>
          <button className="btn-ghost btn-ico" onClick={onClose} aria-label="Close" style={{borderRadius:'999px'}}>
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5"><path d="M2 2 L10 10 M10 2 L2 10"/></svg>
          </button>
        </div>

        <div style={{display:'flex', borderBottom:'1px solid var(--line)', padding:'0 8px', gap:'4px'}}>
          {(['providers','mcp','skills','ui'] as const).map(tab=>(
            <button key={tab} onClick={()=>setActiveTab(tab)} style={{
              flex:1, padding:'8px', borderRadius:'999px', border:'none',
              background: activeTab===tab ? 'var(--text)' : 'transparent',
              color: activeTab===tab ? 'var(--bg)' : 'var(--muted)',
              fontSize:'12px', margin:'8px 0'
            }}>
              {tab === 'providers' ? 'Providers' : tab === 'mcp' ? 'MCP' : tab === 'skills' ? 'Skills' : 'Interface'}
            </button>
          ))}
        </div>

        <div style={{flex:1, overflowY:'auto', padding:'20px'}}>
          {loading ? <div className="mono" style={{textAlign:'center', padding:'32px', color:'var(--muted)', fontSize:'13px'}}>Loading…</div> : (
            <>
              {activeTab==='providers' && (
                <div style={{display:'flex', flexDirection:'column', gap:'16px'}}>
                  <div>
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'6px'}}>Default provider</div>
                    <select value={defaultProvider} onChange={handleDefaultProviderChange} style={{width:'100%'}}>
                      <option value="">Auto (first available)</option>
                      {KNOWN_PROVIDERS.map(p=> <option key={p.id} value={p.id}>{p.name}</option>)}
                      {providers.filter(p=> !KNOWN_PROVIDERS.find(k=>k.id===p.id)).map(p=> <option key={p.id} value={p.id}>{p.name}</option>)}
                    </select>
                    <div className="mono" style={{marginTop:'8px', fontSize:'11px', color:'var(--muted)'}}>New chats will use this provider. You can also switch per-chat via the header.</div>
                  </div>
                  <div style={{height:'1px', background:'var(--line)'}} />
                  <div className="mono" style={{fontSize:'11px', color:'var(--muted)'}}>API keys • stored locally in config.toml • never logged</div>
                  {KNOWN_PROVIDERS.map(p=> {
                    const isConfigured = providers.some(x=> x.id===p.id);
                    const settings = providerSettings[p.id] || {};
                    const isAdvanced = showAdvanced[p.id] || isConfigured;
                    const defaultBase = p.id==='xai' ? 'https://api.x.ai/v1' : p.id==='openai' ? 'https://api.openai.com/v1' : 'https://api.anthropic.com/v1';
                    const defaultModel = p.id==='xai' ? 'grok-4' : p.id==='openai' ? 'gpt-4o' : 'claude-3-5-sonnet-20240620';
                    return (
                      <div key={p.id} style={{padding:'14px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)', opacity: isConfigured ? 1 : 0.95}}>
                        <div style={{display:'flex', alignItems:'center', gap:'8px', marginBottom:'8px'}}>
                          <span style={{fontWeight:600, fontSize:'13px'}}>{p.name}</span>
                          <span className="badge mono">{p.id}</span>
                          <span className="mono" style={{fontSize:'10px', color:'var(--muted)'}}>{p.hint}</span>
                          {isConfigured && <span className="mono" style={{marginLeft:'auto', fontSize:'10px', color:'#16a34a', border:'1px solid rgba(22,163,74,0.3)', padding:'2px 6px', borderRadius:999}}>configured</span>}
                          <button className="btn-ghost" onClick={()=> setShowAdvanced(prev=> ({...prev, [p.id]: !prev[p.id]}))} style={{marginLeft: isConfigured ? '0' : 'auto', fontSize:'10px', padding:'4px 8px', borderRadius:'999px'}}>
                            {isAdvanced ? 'Hide' : 'Base URL / Model'}
                          </button>
                        </div>
                        <div style={{display:'flex', gap:'8px', marginBottom: isAdvanced ? '8px' : '0'}}>
                          <input type="password" placeholder={isConfigured ? '•••••••• (saved) — enter new to replace' : 'sk-...'} value={apiKeys[p.id]||''} onChange={e=>handleApiKeyChange(p.id,e.target.value)} style={{flex:1}} />
                          <button onClick={()=>saveProviderConfig(p.id)} disabled={saving===p.id} style={{borderRadius:'999px', background: isConfigured ? 'var(--panel)' : 'var(--text)', color: isConfigured ? 'var(--text)' : 'var(--bg)'}}>{saving===p.id ? '…' : isConfigured ? 'Save' : 'Save'}</button>
                          {isConfigured && <button className="btn-ghost" onClick={()=>removeApiKey(p.id)} style={{color:'#ff6b6b', borderRadius:'999px'}}>Remove</button>}
                        </div>
                        {isAdvanced && (
                          <div style={{display:'flex', flexDirection:'column', gap:'8px', padding:'10px', background:'var(--panel)', border:'1px solid var(--line)', borderRadius:'8px'}}>
                            <div>
                              <div className="mono" style={{fontSize:'10px', color:'var(--muted)', marginBottom:'4px'}}>Base URL — any OpenAI/Anthropic compatible endpoint</div>
                              <input placeholder={defaultBase} value={settings.base_url || ''} onChange={e=> handleProviderSettingsChange(p.id, 'base_url', e.target.value)} style={{width:'100%'}} />
                              <div className="mono" style={{fontSize:'10px', color:'var(--muted)', marginTop:'4px'}}>Examples: <span style={{color:'var(--text)'}}>http://localhost:11434/v1</span> (Ollama), <span style={{color:'var(--text)'}}>https://api.groq.com/openai/v1</span>, <span style={{color:'var(--text)'}}>https://api.together.xyz/v1</span></div>
                            </div>
                            <div>
                              <div className="mono" style={{fontSize:'10px', color:'var(--muted)', marginBottom:'4px'}}>Model — how to call it</div>
                              <input placeholder={defaultModel} value={settings.model || ''} onChange={e=> handleProviderSettingsChange(p.id, 'model', e.target.value)} style={{width:'100%'}} />
                              <div className="mono" style={{fontSize:'10px', color:'var(--muted)', marginTop:'4px'}}>Examples: <span style={{color:'var(--text)'}}>gpt-4o</span>, <span style={{color:'var(--text)'}}>llama3.2</span>, <span style={{color:'var(--text)'}}>claude-3-5-sonnet-20240620</span></div>
                            </div>
                            <div className="mono" style={{fontSize:'10px', color:'var(--muted)', background:'var(--bg)', padding:'6px 8px', borderRadius:'6px', border:'1px solid var(--line)'}}>
                              Leave blank to use defaults. For custom OpenAI-compatible APIs (Ollama, LM Studio, Groq, Together), set Base URL to the <span style={{color:'var(--text)'}}>/v1</span> endpoint and Model to the model name. The app will call it via the OpenAI ChatCompletions format.
                            </div>
                          </div>
                        )}
                      </div>
                    );
                  })}
                  {/* Custom OpenAI-compatible provider */}
                  <div style={{padding:'14px', border:'1px dashed var(--line)', borderRadius:'12px', background:'var(--panel)'}}>
                    <div style={{fontWeight:600, fontSize:'13px', marginBottom:'4px'}}>Add custom OpenAI-compatible provider</div>
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'10px'}}>For any OpenAI or Anthropic compatible API — Ollama, LM Studio, Groq, Together, custom proxy. The app calls it via OpenAI ChatCompletions.</div>
                    <div style={{display:'flex', flexDirection:'column', gap:'8px'}}>
                      <div style={{display:'flex', gap:'8px'}}>
                        <input placeholder="Provider ID • my-ollama" value={newCustom.id} onChange={e=> setNewCustom({...newCustom, id: e.target.value})} style={{flex:1}} />
                        <select value={newCustom.kind} onChange={e=> setNewCustom({...newCustom, kind: e.target.value as any})} style={{width:'160px'}}>
                          <option value="openai">OpenAI compat</option>
                          <option value="anthropic">Anthropic compat</option>
                        </select>
                      </div>
                      <input placeholder="Base URL • http://localhost:11434/v1" value={newCustom.base_url} onChange={e=> setNewCustom({...newCustom, base_url: e.target.value})} />
                      <div style={{display:'flex', gap:'8px'}}>
                        <input placeholder="Model • llama3.2 / gpt-4o-mini / claude-3-5-sonnet-20240620" value={newCustom.model} onChange={e=> setNewCustom({...newCustom, model: e.target.value})} style={{flex:1}} />
                        {(newCustom.id==='kilo' || newCustom.base_url.includes('kilo.ai')) && (
                          <button className="btn-ghost" onClick={()=> fetchKiloModels('new')} style={{whiteSpace:'nowrap', borderRadius:'999px'}}>Browse 367</button>
                        )}
                      </div>
                      <input type="password" placeholder="API key (optional for local) • sk-... or leave blank for Ollama" value={newCustom.api_key} onChange={e=> setNewCustom({...newCustom, api_key: e.target.value})} />
                      <div style={{display:'flex', gap:'8px', alignItems:'center'}}>
                        <button onClick={addCustomProvider} disabled={saving==='custom' || !newCustom.id.trim() || !newCustom.base_url.trim() || !newCustom.model.trim()} style={{borderRadius:'999px'}}>{saving==='custom' ? 'Adding…' : 'Add custom provider'}</button>
                        <span className="mono" style={{fontSize:'10px', color:'var(--muted)'}}>ID becomes selectable in header • stored in config.toml</span>
                      </div>
                    </div>
                  </div>
                  {/* Show custom providers that are already configured */}
                  {PRESET_PROVIDERS.filter(preset=> !providers.some(pr=> pr.id===preset.id)).length > 0 && (
                    <div style={{display:'flex', flexDirection:'column', gap:'8px', marginBottom:'16px'}}>
                      <div className="mono" style={{fontSize:'11px', color:'var(--muted)'}}>One-click presets</div>
                      {PRESET_PROVIDERS.filter(preset=> !providers.some(pr=> pr.id===preset.id)).map(preset=> (
                        <div key={preset.id} style={{padding:'12px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)', display:'flex', alignItems:'center', gap:'12px', flexWrap:'wrap'}}>
                          <div style={{flex:1, minWidth:0}}>
                            <div style={{display:'flex', gap:'8px', alignItems:'center'}}><span style={{fontWeight:600, fontSize:'13px'}}>{preset.name}</span><span className="badge mono">{preset.id}</span></div>
                            <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginTop:'4px'}}>{preset.hint} • {preset.model}</div>
                          </div>
                          <button className="btn-ghost" onClick={()=> setNewCustom({id:preset.id, base_url:preset.base_url, model:preset.model, api_key:'', kind:preset.kind})} style={{borderRadius:'999px', border:'1px dashed var(--line)'}}>Use preset</button>
                        </div>
                      ))}
                    </div>
                  )}
                  {providers.filter(p=> !KNOWN_PROVIDERS.find(k=>k.id===p.id)).length > 0 && (
                    <div style={{display:'flex', flexDirection:'column', gap:'8px', marginBottom:'16px'}}>
                      <div className="mono" style={{fontSize:'11px', color:'var(--muted)'}}>Custom providers</div>
                      {providers.filter(p=> !KNOWN_PROVIDERS.find(k=>k.id===p.id)).map(p=> {
                        const s = providerSettings[p.id] || {};
                        const isAdvanced = showAdvanced[p.id];
                        return (
                          <div key={p.id} style={{padding:'12px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)', display:'flex', flexDirection:'column', gap:'8px'}}>
                            <div style={{display:'flex', gap:'8px', alignItems:'center', flexWrap:'wrap'}}>
                              <span style={{fontWeight:600, fontSize:'13px'}}>{p.name}</span>
                              <span className="badge mono">{p.id}</span>
                              <span className="mono" style={{fontSize:'10px', color:'var(--muted)'}}>{s.base_url || '—'}</span>
                              <span className="mono" style={{fontSize:'10px', color:'var(--muted)'}}>{s.model || '—'}</span>
                              <span className="mono" style={{fontSize:'10px', color:'var(--muted)', marginLeft:'auto'}}>{s.kind === 'anthropic' ? 'Anthropic-compat' : 'OpenAI-compat'}</span>
                              <button className="btn-ghost" onClick={()=> setShowAdvanced(prev=> ({...prev, [p.id]: !prev[p.id]}))} style={{fontSize:'10px', padding:'4px 8px', borderRadius:'999px'}}>{isAdvanced ? 'Hide' : 'Edit'}</button>
                            </div>
                            {isAdvanced && (
                              <div style={{display:'flex', flexDirection:'column', gap:'8px', padding:'10px', background:'var(--panel)', border:'1px solid var(--line)', borderRadius:'8px'}}>
                                <input type="password" placeholder="API key (optional) • sk-..." value={customKeys[p.id]||''} onChange={e=> setCustomKeys(prev=>({...prev,[p.id]:e.target.value}) )} style={{flex:1}} />
                                <div style={{display:'flex', gap:'8px', alignItems:'center', flexWrap:'wrap'}}>
                                  <select value={s.kind || 'openai'} onChange={e=> handleProviderSettingsChange(p.id, 'kind', e.target.value)} style={{width:'140px'}}>
                                    <option value="openai">OpenAI compat</option>
                                    <option value="anthropic">Anthropic compat</option>
                                  </select>
                                  <input placeholder="Base URL" value={s.base_url || ''} onChange={e=> handleProviderSettingsChange(p.id, 'base_url', e.target.value)} style={{flex:1, minWidth:'140px'}} />
                                  <div style={{display:'flex', gap:'6px', flex:1, minWidth:'140px'}}>
                                    <input placeholder="Model" value={s.model || ''} onChange={e=> handleProviderSettingsChange(p.id, 'model', e.target.value)} style={{flex:1}} />
                                    {p.id==='kilo' && <button className="btn-ghost" onClick={()=> fetchKiloModels(p.id)} style={{whiteSpace:'nowrap', borderRadius:'999px', fontSize:'11px', padding:'6px 10px'}}>Browse</button>}
                                  </div>
                                  <button onClick={async()=>{ const k = customKeys[p.id]?.trim(); const set = {baseUrl: s.base_url?.trim()||null, model: s.model?.trim()||null, kind: s.kind?.trim()||null}; if(k){ await invoke('set_api_key',{providerId:p.id, apiKey:k}); } await invoke('set_provider_settings',{providerId:p.id, ...set}); setCustomKeys(prev=>{const n={...prev}; delete n[p.id]; return n;}); onRefresh?.(); }} style={{borderRadius:'999px', background:'var(--text)', color:'var(--bg)'}}>Save</button>
                                </div>
                              </div>
                            )}
                            <button className="btn-ghost" onClick={async()=>{ if(confirm(`Remove ${p.id}?`)){ await invoke('remove_api_key',{providerId:p.id}); await invoke('set_provider_settings',{providerId:p.id, baseUrl:null, model:null, kind:null}); onRefresh?.(); loadConfig(); } }} style={{color:'#ff6b6b', borderRadius:'999px', alignSelf:'flex-start'}}>Remove</button>
                          </div>
                        );
                      })}
                    </div>
                  )}
                  <div className="mono" style={{fontSize:'11px', color:'var(--muted)', background:'var(--panel)', border:'1px solid var(--line)', padding:'10px 12px', borderRadius:'8px'}}>
                    How to add: 1) Get API key from <span style={{color:'var(--text)'}}>x.ai/api</span> or <span style={{color:'var(--text)'}}>platform.openai.com</span> or <span style={{color:'var(--text)'}}>console.anthropic.com</span> → 2) For custom endpoints, set Base URL to <span style={{color:'var(--text)'}}>/v1</span> and Model name → Save → Select provider in header or set as default. No mock data — real keys only.
                  </div>
                </div>
              )}

              {activeTab==='mcp' && (
                <div style={{display:'flex', flexDirection:'column', gap:'16px'}}>
                  <div style={{padding:'16px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)'}}>
                    <div style={{fontWeight:600, fontSize:'13px', marginBottom:'12px'}}>Add MCP server</div>
                    <div style={{display:'flex', flexDirection:'column', gap:'10px'}}>
                      <input placeholder="Server name • filesystem" value={newMcp.name} onChange={e=>setNewMcp({...newMcp,name:e.target.value})} />
                      <select value={newMcp.transport} onChange={e=>setNewMcp({...newMcp,transport:e.target.value})}><option value="stdio">STDIO — Local</option><option value="http">HTTP — Remote</option></select>
                      {newMcp.transport==='stdio' ? (<>
                        <input placeholder="Command • npx" value={newMcp.command} onChange={e=>setNewMcp({...newMcp,command:e.target.value})} />
                        <input placeholder="Args • -y @modelcontextprotocol/server-filesystem /tmp" value={newMcp.args} onChange={e=>setNewMcp({...newMcp,args:e.target.value})} />
                      </>) : (
                        <input placeholder="URL • http://localhost:3000/mcp" value={newMcp.url} onChange={e=>setNewMcp({...newMcp,url:e.target.value})} />
                      )}
                      <button onClick={addMcpServer} disabled={saving==='mcp' || !newMcp.name.trim() || (newMcp.transport==='stdio' && !newMcp.command.trim()) || (newMcp.transport==='http' && !newMcp.url?.trim())} style={{alignSelf:'flex-start', borderRadius:'999px'}}>{saving==='mcp' ? 'Adding…' : 'Add server'}</button>
                    </div>
                  </div>
                  {mcpServers.length===0 ? <div className="mono" style={{textAlign:'center', padding:'20px', color:'var(--muted)', fontSize:'12px'}}>No MCP servers configured</div> : (
                    <div style={{display:'flex', flexDirection:'column', gap:'8px'}}>
                      {mcpServers.map(s=>(
                        <div key={s.name} style={{padding:'12px', border:'1px solid var(--line)', borderRadius:'12px', display:'flex', alignItems:'center', gap:'12px', background:'var(--bg)'}}>
                          <div style={{flex:1, minWidth:0}}>
                            <div style={{display:'flex', gap:'8px', alignItems:'center'}}><span style={{fontWeight:600, fontSize:'13px'}}>{s.name}</span><span className="badge mono">{s.transport}</span></div>
                            <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginTop:'4px', whiteSpace:'nowrap', overflow:'hidden', textOverflow:'ellipsis'}}>{s.transport==='stdio' ? `${s.command} ${s.args}` : s.url}</div>
                          </div>
                          <button className="btn-ghost" onClick={()=>removeMcpServer(s.name)} style={{color:'#ff6b6b', borderRadius:'999px'}}>Remove</button>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}

              {activeTab==='skills' && (
                <div style={{display:'flex', flexDirection:'column', gap:'16px'}}>
                  <div style={{display:'flex', alignItems:'center', gap:'8px'}}>
                    <input placeholder="Search skills • name or description" value={skillSearch} onChange={e=> setSkillSearch(e.target.value)} style={{flex:1}} />
                    <button className="btn-ghost" onClick={loadSkills} style={{borderRadius:'999px', fontSize:'11px', padding:'6px 10px'}}>Refresh</button>
                    <span className="mono" style={{fontSize:'11px', color:'var(--muted)', border:'1px solid var(--line)', padding:'4px 8px', borderRadius:999}}>{skills.length} skills</span>
                  </div>

                  {skills.length===0 ? <div className="mono" style={{textAlign:'center', padding:'20px', color:'var(--muted)', fontSize:'12px'}}>No skills installed — add one below or fetch from hub</div> : (
                    <div style={{display:'flex', flexDirection:'column', gap:'8px', maxHeight:'220px', overflowY:'auto'}}>
                      {skills.filter(s=> !skillSearch.trim() || s.name.toLowerCase().includes(skillSearch.toLowerCase()) || s.description.toLowerCase().includes(skillSearch.toLowerCase())).map(s=>(
                        <div key={s.path} style={{padding:'12px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)', display:'flex', flexDirection:'column', gap:'6px'}}>
                          <div style={{display:'flex', gap:'8px', alignItems:'center', flexWrap:'wrap'}}>
                            <span style={{fontWeight:600, fontSize:'13px'}}>{s.name}</span>
                            <span className="badge mono">{s.scope}</span>
                            {s.enabled ? <span className="mono" style={{fontSize:'10px', color:'#16a34a', border:'1px solid rgba(22,163,74,0.3)', padding:'1px 6px', borderRadius:999}}>enabled</span> : <span className="mono" style={{fontSize:'10px', color:'var(--faint)', border:'1px solid var(--line)', padding:'1px 6px', borderRadius:999}}>disabled</span>}
                            <button className="btn-ghost" onClick={()=> handleRemoveSkill(s.name)} style={{marginLeft:'auto', color:'#ff6b6b', borderRadius:'999px', fontSize:'11px', padding:'4px 8px'}}>Remove</button>
                          </div>
                          <div className="mono" style={{fontSize:'11px', color:'var(--muted)', lineHeight:1.5}}>{s.description || 'No description — add one in SKILL.md frontmatter'}</div>
                          <div className="mono" style={{fontSize:'10px', color:'var(--faint)', wordBreak:'break-all'}}>{s.path}</div>
                        </div>
                      ))}
                    </div>
                  )}

                  <div style={{height:'1px', background:'var(--line)'}} />

                  <div style={{padding:'16px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)'}}>
                    <div style={{fontWeight:600, fontSize:'13px', marginBottom:'4px'}}>Install local skill</div>
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'10px'}}>Paste SKILL.md content — frontmatter optional. Will be saved to <span style={{color:'var(--text)'}}>~/.maverick/skills/&lt;name&gt;/SKILL.md</span></div>
                    <div style={{display:'flex', flexDirection:'column', gap:'8px'}}>
                      <input placeholder="Skill name • e.g. my-helper" value={newSkill.name} onChange={e=> setNewSkill({...newSkill, name: e.target.value})} />
                      <textarea placeholder="SKILL.md content • ---&#10;name: my-helper&#10;description: Helps with ...&#10;---&#10;# Instructions&#10;..." value={newSkill.content} onChange={e=> setNewSkill({...newSkill, content: e.target.value})} rows={6} style={{fontFamily:'var(--font-mono)', fontSize:'12px', minHeight:'120px'}} />
                      <button onClick={handleInstallSkill} disabled={saving==='skill' || !newSkill.name.trim() || !newSkill.content.trim()} style={{alignSelf:'flex-start', borderRadius:'999px'}}>{saving==='skill' ? 'Installing…' : 'Install skill'}</button>
                    </div>
                  </div>

                  <div style={{padding:'16px', border:'1px dashed var(--line)', borderRadius:'12px', background:'var(--panel)'}}>
                    <div style={{fontWeight:600, fontSize:'13px', marginBottom:'4px'}}>Fetch from hub / marketplace</div>
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'10px'}}>Supports <span style={{color:'var(--text)'}}>agentskills.io</span> and any raw SKILL.md URL. Full marketplace browse coming soon — uses same hub cache as `Server` scope.</div>
                    <div style={{display:'flex', flexDirection:'column', gap:'8px'}}>
                      <div style={{display:'flex', gap:'8px'}}>
                        <input placeholder="Hub URL • https://agentskills.io" value={hubFetch.url} onChange={e=> setHubFetch({...hubFetch, url: e.target.value})} style={{flex:1}} />
                      </div>
                      <div style={{display:'flex', gap:'8px', flexWrap:'wrap'}}>
                        <input placeholder="Owner • e.g. maverick" value={hubFetch.owner} onChange={e=> setHubFetch({...hubFetch, owner: e.target.value})} style={{flex:1, minWidth:'120px'}} />
                        <input placeholder="Skill • e.g. commit" value={hubFetch.name} onChange={e=> setHubFetch({...hubFetch, name: e.target.value})} style={{flex:1, minWidth:'120px'}} />
                        <input placeholder="Version (optional)" value={hubFetch.version} onChange={e=> setHubFetch({...hubFetch, version: e.target.value})} style={{width:'130px'}} />
                        <button onClick={handleFetchHub} disabled={saving==='hub' || !hubFetch.owner.trim() || !hubFetch.name.trim()} style={{borderRadius:'999px', whiteSpace:'nowrap'}}>{saving==='hub' ? 'Fetching…' : 'Fetch'}</button>
                      </div>
                      <div className="mono" style={{fontSize:'10px', color:'var(--muted)', background:'var(--bg)', padding:'6px 8px', borderRadius:'6px', border:'1px solid var(--line)'}}>
                        Example: <span style={{color:'var(--text)'}}>owner=maverick name=commit</span> fetches <span style={{color:'var(--text)'}}>https://agentskills.io/maverick/commit/SKILL.md</span> → cached as <span style={{color:'var(--text)'}}>~/.maverick/hub/skills/maverick/commit/SKILL.md</span> (Server scope). Raw URL also works — paste full URL into Hub URL and set owner/name dummy.
                      </div>
                    </div>
                  </div>

                  <div className="mono" style={{fontSize:'11px', color:'var(--muted)', background:'var(--panel)', border:'1px solid var(--line)', padding:'10px 12px', borderRadius:'8px'}}>
                    Skills are loaded into <span style={{color:'var(--text)'}}>ToolBridge</span> as the `skill` tool — the model sees them as <span style={{color:'var(--text)'}}>available_skills</span> and can call `skill(name: "...")` to inject instructions. Add a skill, then prompt “use skill X” in chat. Marketplace (full browse/install from `xai-org/plugin-marketplace`) is next — hub cache already uses `Server` scope dedup.
                  </div>
                </div>
              )}

              {activeTab==='ui' && (
                <div style={{display:'flex', flexDirection:'column', gap:'16px'}}>
                  <div style={{padding:'16px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)'}}>
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'12px'}}>Theme</div>
                    <div style={{display:'flex', gap:'8px'}}>
                      {(['dark','light','system'] as const).map(t=>(
                        <button key={t} onClick={()=>{ const n={...uiConfig, theme:t}; setUiConfig(n); invoke('set_ui_config',{ui:n}); }} style={{flex:1, background: uiConfig.theme===t ? 'var(--text)' : 'transparent', color: uiConfig.theme===t ? 'var(--bg)' : 'var(--muted)', borderRadius:'999px'}}>{t.charAt(0).toUpperCase()+t.slice(1)}</button>
                      ))}
                    </div>
                  </div>
                  <div style={{padding:'16px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)'}}>
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'12px'}}>Behavior</div>
                    <div style={{display:'flex', flexDirection:'column', gap:'12px'}}>
                      {[
                        ['show_tool_calls','Show tool calls inline'],
                        ['auto_scroll','Auto-scroll to new messages'],
                        ['compact_mode','Compact mode'],
                      ].map(([k,label])=>(
                        <label key={k} style={{display:'flex', alignItems:'center', gap:'10px', cursor:'pointer', fontSize:'13px'}}>
                          <input type="checkbox" checked={(uiConfig as any)[k]} onChange={e=>handleUiChange(k,e.target.checked)} style={{width:'16px', height:'16px', accentColor:'var(--text)'}} />
                          <span>{label}</span>
                        </label>
                      ))}
                    </div>
                  </div>
                </div>
              )}
            </>
          )}
        </div>

        <div style={{padding:'16px', borderTop:'1px solid var(--line)', display:'flex', justifyContent:'flex-end'}}>
          <button onClick={onClose} style={{borderRadius:'999px', background:'var(--text)', color:'var(--bg)', borderColor:'var(--text)'}}>Done</button>
        </div>
      </div>

      {/* Kilo model picker */}
      {showKiloPicker && (
        <div style={{position:'fixed', inset:0, background:'rgba(0,0,0,0.6)', backdropFilter:'blur(8px)', display:'flex', alignItems:'center', justifyContent:'center', zIndex:200, padding:'16px'}} onClick={()=> setShowKiloPicker(null)}>
          <div onClick={e=>e.stopPropagation()} style={{width:'100%', maxWidth:'560px', maxHeight:'78vh', display:'flex', flexDirection:'column', background:'var(--panel)', border:'1px solid var(--line)', borderRadius:'16px', overflow:'hidden'}}>
            <div style={{padding:'14px 16px', borderBottom:'1px solid var(--line)', display:'flex', justifyContent:'space-between', alignItems:'center'}}>
              <div><div style={{fontWeight:600, fontSize:'13px'}}>Kilo models</div><div className="mono" style={{fontSize:'11px', color:'var(--muted)'}}>{kiloModels.length} available • GET https://api.kilo.ai/api/gateway/models</div></div>
              <button className="btn-ghost btn-ico" onClick={()=> setShowKiloPicker(null)} style={{borderRadius:'999px'}}>✕</button>
            </div>
            <div style={{padding:'12px 16px', borderBottom:'1px solid var(--line)', display:'flex', flexDirection:'column', gap:'8px'}}>
              <input placeholder="Search model • anthropic/claude, gpt-5, gemini…" value={kiloSearch} onChange={e=> setKiloSearch(e.target.value)} style={{width:'100%'}} autoFocus />
              <div style={{display:'flex', gap:'6px'}}>
                {(['all','free','paid'] as const).map(f=>(
                  <button key={f} onClick={()=> setKiloFilter(f)} style={{flex:1, padding:'6px', borderRadius:'999px', background: kiloFilter===f ? 'var(--text)' : 'transparent', color: kiloFilter===f ? 'var(--bg)' : 'var(--muted)', border:'1px solid var(--line)', fontSize:'12px', textTransform:'capitalize'}}>{f}</button>
                ))}
                <span className="mono" style={{marginLeft:'auto', fontSize:'11px', color:'var(--muted)', alignSelf:'center'}}>{filteredKilo.length} shown</span>
              </div>
            </div>
            <div style={{flex:1, overflowY:'auto', padding:'8px'}}>
              {kiloLoading ? <div className="mono" style={{textAlign:'center', padding:'24px', color:'var(--muted)'}}>Loading…</div>
               : kiloError ? <div className="mono" style={{textAlign:'center', padding:'24px', color:'#ff6b6b'}}>{kiloError}</div>
               : filteredKilo.length===0 ? <div className="mono" style={{textAlign:'center', padding:'24px', color:'var(--muted)'}}>No matches</div>
               : filteredKilo.slice(0,120).map(m=>(
                <button key={m.id} onClick={()=>{
                  if(showKiloPicker==='new'){
                    setNewCustom(prev=> ({...prev, id:'kilo', base_url:'https://api.kilo.ai/api/gateway', model:m.id, kind:'openai'}));
                  } else {
                    handleProviderSettingsChange(showKiloPicker!, 'model', m.id);
                  }
                  setShowKiloPicker(null);
                }} style={{width:'100%', textAlign:'left', display:'flex', flexDirection:'column', gap:'2px', padding:'10px 12px', marginBottom:'6px', background:'var(--bg)', border:'1px solid var(--line)', borderRadius:'12px'}}>
                  <div style={{display:'flex', gap:'8px', alignItems:'center', flexWrap:'wrap'}}>
                    <span style={{fontWeight:600, fontSize:'12px', wordBreak:'break-all'}}>{m.id}</span>
                    {m.is_free ? <span className="mono" style={{fontSize:'10px', color:'#16a34a', border:'1px solid rgba(22,163,74,0.3)', padding:'1px 6px', borderRadius:999}}>free</span> : <span className="mono" style={{fontSize:'10px', color:'var(--muted)', border:'1px solid var(--line)', padding:'1px 6px', borderRadius:999}}>paid</span>}
                    {m.context_length ? <span className="mono" style={{fontSize:'10px', color:'var(--muted)'}}>{Math.round(m.context_length/1000)}k</span> : null}
                  </div>
                  <div className="mono" style={{fontSize:'11px', color:'var(--muted)', lineHeight:1.4}}>{m.name}</div>
                </button>
              ))}
              {filteredKilo.length>120 && <div className="mono" style={{textAlign:'center', fontSize:'11px', color:'var(--muted)', padding:'8px'}}>Showing 120 of {filteredKilo.length} — refine search</div>}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
