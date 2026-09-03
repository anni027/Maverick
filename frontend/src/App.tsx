import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import Chat from './components/Chat';
import SessionSidebar from './components/SessionSidebar';
import ProviderSelector from './components/ProviderSelector';
import Settings from './components/Settings';
import { ProviderInfo } from './types';

function Mark({ size = 24 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" aria-hidden>
      <rect x="2" y="2" width="20" height="20" rx="5" fill="#E30613" />
      <path d="M6.5 16 V8.5 L10.5 13.5 L14.5 8.5 V16 M17.5 8.5 V16" stroke="white" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round"/>
    </svg>
  );
}

export default function App() {
  const [sessionId, setSessionId] = useState('demo-session');
  const [sessions, setSessions] = useState<string[]>([]);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [selectedProvider, setSelectedProvider] = useState('mock');
  const [tools, setTools] = useState<string[]>([]);
  const [showSidebar, setShowSidebar] = useState(true);
  const [showSettings, setShowSettings] = useState(false);
  const [isInitialized, setIsInitialized] = useState(false);
  const [initError, setInitError] = useState<string | null>(null);
  const [initStep, setInitStep] = useState('Starting');

  useEffect(() => { initialize(); }, []);

  const withTimeout = <T,>(p: Promise<T>, ms: number, label: string): Promise<T> =>
    Promise.race([p, new Promise<never>((_, r) => setTimeout(() => r(new Error(`${label} timed out after ${ms}ms`)), ms))]);

  const initialize = async () => {
    setInitError(null);
    const fallback = setTimeout(() => { setInitError(prev => prev || 'Initialization timed out — showing UI'); setIsInitialized(true); }, 10000);
    try {
      setInitStep('Connecting');
      listen('agent-event', (e: any) => console.log('agent-event', e.payload)).catch(()=>{});
      setInitStep('Loading sessions');
      let target = sessionId;
      let chosenProvider = selectedProvider;
      try {
        const l = await withTimeout(invoke<string[]>('list_sessions'), 5000, 'list_sessions');
        setSessions(l); if(l.length && !l.includes(sessionId)){ target=l[0]; setSessionId(target); }
      } catch(e){ setInitError(`Sessions: ${String(e)}`)}
      setInitStep('Loading providers');
      try {
        const [pl, defProvider] = await Promise.all([
          withTimeout(invoke<ProviderInfo[]>('list_providers'), 5000, 'list_providers'),
          withTimeout(invoke<string | null>('get_default_provider'), 5000, 'get_default_provider').catch(() => null),
        ]);
        const visible = pl.filter(p => p.id !== 'mock');
        const list = visible.length ? visible : pl;
        setProviders(list);
        if (defProvider && list.some(p => p.id === defProvider)) {
          chosenProvider = defProvider;
        } else if (visible.length) {
          chosenProvider = visible[0].id;
        } else if (list.length) {
          chosenProvider = list[0].id;
        }
        setSelectedProvider(chosenProvider);
      } catch(e){ setInitError(`Providers: ${String(e)}`)}
      setInitStep('Loading tools');
      try { const tl = await withTimeout(invoke<string[]>('list_tools'),5000,'list_tools'); setTools(tl.filter(t=> t !== 'test_tool')) } catch(e){ setInitError(`Tools: ${String(e)}`)}
      setInitStep('Opening session');
      try { await withTimeout(invoke('init_session', { sessionId: target, providerId: chosenProvider }), 10000, 'init_session'); } catch(e){
        setInitError(`${String(e)}`);
        try{ await withTimeout(invoke('init_session',{sessionId:'demo-session', providerId: chosenProvider}),8000,'retry'); setSessionId('demo-session'); setInitError(null)}catch{}
      }
      setInitStep('Ready');
    } catch(e){ setInitError(String(e)); } finally { clearTimeout(fallback); setTimeout(()=>setIsInitialized(true), 200); }
  };

  const handleSelectSession = async (id:string) => {
    setSessionId(id);
    try{ await invoke('init_session',{sessionId:id, providerId: selectedProvider}); }catch(e){ console.error(e)}
  };
  const handleNewSession = async () => {
    const nid=`session-${Date.now()}`;
    try{ await invoke('init_session',{sessionId:nid, providerId: selectedProvider}); const l=await invoke<string[]>('list_sessions'); setSessions(l); setSessionId(nid);}catch(e){console.error(e)}
  };
  const handleDeleteSession = async (id: string) => {
    try {
      await invoke('delete_session', { sessionId: id });
      const updated = sessions.filter(s => s !== id);
      setSessions(updated);
      if (sessionId === id) {
        if (updated.length > 0) {
          await handleSelectSession(updated[0]);
        } else {
          await handleNewSession();
        }
      }
    } catch (e) {
      console.error('Failed to delete session', e);
    }
  };
  const handleRenameSession = async (oldId: string, newId: string) => {
    const trimmed = newId.trim();
    if (!trimmed || trimmed === oldId) return;
    try {
      await invoke('rename_session', { oldId, newId: trimmed });
      const updated = sessions.map(s => (s === oldId ? trimmed : s));
      setSessions(updated);
      if (sessionId === oldId) {
        setSessionId(trimmed);
      }
    } catch (e) {
      console.error('Failed to rename session', e);
    }
  };
  const handleProviderChange = async (id: string) => {
    setSelectedProvider(id);
    try { await invoke('set_default_provider', { providerId: id }); } catch(e){ console.error('set_default_provider failed', e); }
    // Re-init current session with new provider so next message uses it
    try { await invoke('init_session', { sessionId, providerId: id }); } catch(e){ console.error('re-init with new provider failed', e); }
  };

  const refreshProviders = async () => {
    try {
      const pl = await invoke<ProviderInfo[]>('list_providers');
      const visible = pl.filter(p=> p.id !== 'mock');
      const list = visible.length ? visible : pl;
      setProviders(list);
      if (visible.length > 0 && (selectedProvider === 'mock' || !visible.some(p => p.id === selectedProvider))) {
        await handleProviderChange(visible[0].id);
      } else if (list.length > 0 && !list.some(p => p.id === selectedProvider)) {
        await handleProviderChange(list[0].id);
      }
    } catch(e){ console.error('refreshProviders failed', e); }
  };

  const handleAddMcp = async (name:string, command:string, args:string[]) => {
    try{ await invoke('add_mcp',{name,command,args}); const tl=await invoke<string[]>('list_tools'); setTools(tl.filter(t=> t!=='test_tool'))}catch(e){console.error(e)}
  };

  if (!isInitialized) {
    return (
      <div style={{height:'100vh', display:'flex', alignItems:'center', justifyContent:'center', background:'var(--bg)', padding:'32px'}}>
        <div style={{width:'100%', maxWidth:'360px', textAlign:'center'}}>
          <div style={{display:'inline-flex', alignItems:'center', gap:'10px', marginBottom:'20px'}}>
            <Mark size={32} />
            <span style={{fontWeight:650, fontSize:'16px', letterSpacing:'-0.02em'}}>Maverick</span>
            <span className="mono" style={{fontSize:'11px', color:'var(--muted)', letterSpacing:'0.06em'}}>ChatGPT • Agentic</span>
          </div>
          <div className="panel" style={{padding:'16px', textAlign:'left'}}>
            <div className="mono" style={{fontSize:'11px', color:'var(--muted)', display:'flex', justifyContent:'space-between'}}>
              <span>{initStep}</span><span style={{color:'var(--rosso)'}}>Syncing</span>
            </div>
            <div style={{height:'2px', background:'var(--line)', marginTop:'12px', borderRadius:999, overflow:'hidden'}}>
              <div style={{height:'100%', width:'42%', background:'var(--rosso)', animation:'shim 1s ease-in-out infinite'}} />
            </div>
            {initError && <div className="mono" style={{marginTop:'12px', fontSize:'12px', color:'#ff6b6b', background:'rgba(227,6,19,0.08)', border:'1px solid rgba(227,6,19,0.2)', padding:'8px 10px', borderRadius:'8px'}}>{initError}</div>}
          </div>
        </div>
      </div>
    );
  }

  const visibleProviders = providers.filter(p=> p.id!=='mock');

  return (
    <div style={{display:'flex', height:'100vh', background:'var(--bg)', overflow:'hidden'}}>
      {showSidebar && (
        <div style={{width:'260px', minWidth:'260px', display:'flex', flexDirection:'column', background:'#0F0F0F', borderRight:'1px solid var(--line)'}}>
          <SessionSidebar
            sessions={sessions}
            currentSession={sessionId}
            onSelect={handleSelectSession}
            onNew={handleNewSession}
            onDelete={handleDeleteSession}
            onRename={handleRenameSession}
          />
        </div>
      )}

      <div style={{flex:1, display:'flex', flexDirection:'column', minWidth:0, background:'var(--bg)'}}>
        <header style={{height:'56px', display:'flex', alignItems:'center', gap:'12px', padding:'0 16px', borderBottom:'1px solid var(--line)', background:'var(--bg)', flexShrink:0}}>
          <button className="btn-ghost btn-ico" onClick={()=>setShowSidebar(v=>!v)} aria-label="Toggle sidebar">
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4"><path d="M3 3.5 H13 V12.5 H3 Z"/><path d="M5.5 3.5 V12.5"/></svg>
          </button>
          <div style={{display:'flex', alignItems:'center', gap:'10px'}}>
            <Mark size={22} />
            <div style={{lineHeight:1}}>
              <div style={{fontWeight:650, fontSize:'14px', letterSpacing:'-0.02em'}}>Maverick</div>
              <div className="mono" style={{fontSize:'10px', color:'var(--muted)'}}>ChatGPT familiar • Agent intelligence</div>
            </div>
          </div>
          <div style={{height:'20px', width:'1px', background:'var(--line)', margin:'0 4px'}} />
          <ProviderSelector providers={visibleProviders.length?visibleProviders:providers} selected={selectedProvider} onChange={handleProviderChange} onOpenSettings={()=>setShowSettings(true)} />
          <div style={{flex:1}} />
          <div className="mono" style={{fontSize:'11px', color:'var(--muted)', border:'1px solid var(--line)', padding:'6px 10px', borderRadius:999}}>
            {tools.length} tools
          </div>
          <button className="btn-ghost btn-ico" onClick={()=>setShowSettings(true)} aria-label="Settings">
            <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3"><circle cx="8" cy="8" r="2.8"/><path d="M8 1.8 V3.2 M8 12.8 V14.2 M1.8 8 H3.2 M12.8 8 H14.2"/><path d="M3.6 3.6 L4.6 4.6 M11.4 11.4 L12.4 12.4 M12.4 3.6 L11.4 4.6 M4.6 11.4 L3.6 12.4" opacity="0.6"/></svg>
          </button>
        </header>

        <div style={{flex:1, display:'flex', justifyContent:'center', overflow:'hidden'}}>
          <div style={{width:'100%', maxWidth:'760px', display:'flex', flexDirection:'column', flex:1, minWidth:0}}>
            <Chat sessionId={sessionId} onNewSession={handleNewSession} onAddMcp={handleAddMcp} availableTools={tools} />
          </div>
        </div>
      </div>

      <Settings isOpen={showSettings} onClose={()=>setShowSettings(false)} providers={visibleProviders.length?visibleProviders:providers} currentProvider={selectedProvider} onProviderChange={handleProviderChange} onRefresh={refreshProviders} />
    </div>
  );
}
