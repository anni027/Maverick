import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import Chat from './components/Chat';
import SessionSidebar from './components/SessionSidebar';
import ModelPresetBadge from './components/ModelPresetBadge';
import Settings from './components/Settings';
import ApertureLogo from './components/ApertureLogo';
import { PanelIcon, SettingsIcon, PlusIcon } from './components/icons';
import { ProviderInfo, UiConfig, DEFAULT_UI_CONFIG } from './types';
import { useTheme, type ThemeMode } from './hooks/useTheme';

export default function App() {
  // No placeholder id: a fake default would make `init_session` create a real
  // `demo-session` directory whenever startup partially failed.
  const [sessionId, setSessionId] = useState('');
  const [sessions, setSessions] = useState<string[]>([]);
  const [providers, setProviders] = useState<ProviderInfo[]>([]);
  const [selectedProvider, setSelectedProvider] = useState('');
  const [tools, setTools] = useState<string[]>([]);
  const [showSidebar, setShowSidebar] = useState(true);
  const [showSettings, setShowSettings] = useState(false);
  const [isInitialized, setIsInitialized] = useState(false);
  const [initError, setInitError] = useState<string | null>(null);
  const [initStep, setInitStep] = useState('Starting');
  // UI preferences (tool bubbles / auto-scroll / compact / theme). Loaded at
  // startup and re-read whenever Settings saves one — previously these toggles
  // were written to config but never read back or applied anywhere.
  const [uiConfig, setUiConfig] = useState<UiConfig>(DEFAULT_UI_CONFIG);
  const loadUiConfig = async () => {
    try {
      const ui = await invoke<Partial<UiConfig>>('get_ui_config');
      setUiConfig({ ...DEFAULT_UI_CONFIG, ...ui });
    } catch { /* keep defaults */ }
  };

  // Theme: UiConfig.theme (config.toml) is the source of truth; push it into
  // ThemeProvider, which applies the `dark` class + localStorage mirror.
  const { setMode } = useTheme();
  useEffect(() => {
    const t = uiConfig.theme;
    setMode(t === 'light' || t === 'dark' || t === 'system' ? (t as ThemeMode) : 'dark');
  }, [uiConfig.theme, setMode]);

  useEffect(() => { initialize(); }, []);

  const withTimeout = <T,>(p: Promise<T>, ms: number, label: string): Promise<T> =>
    Promise.race([p, new Promise<never>((_, r) => setTimeout(() => r(new Error(`${label} timed out after ${ms}ms`)), ms))]);

  const initialize = async () => {
    setInitError(null);
    const fallback = setTimeout(() => { setInitError(prev => prev || 'Initialization timed out — showing UI'); setIsInitialized(true); }, 10000);
    try {
      setInitStep('Connecting');
      listen('agent-event', (e: any) => console.log('agent-event', e.payload)).catch(()=>{});
      loadUiConfig();
      setInitStep('Loading sessions');
      let target = sessionId;
      let chosenProvider = selectedProvider;
      try {
        const l = await withTimeout(invoke<string[]>('list_sessions'), 5000, 'list_sessions');
        setSessions(l);
        // First existing session, or a fresh id for a first-ever run.
        target = l.length ? l[0] : `session-${Date.now()}`;
        setSessionId(target);
      } catch(e){
        setInitError(`Sessions: ${String(e)}`);
        if (!target) {
          target = `session-${Date.now()}`;
          setSessionId(target);
        }
      }
      setInitStep('Loading providers');
      try {
        const [pl, defProvider] = await Promise.all([
          withTimeout(invoke<ProviderInfo[]>('list_providers'), 5000, 'list_providers'),
          withTimeout(invoke<string | null>('get_default_provider'), 5000, 'get_default_provider').catch(() => null),
        ]);
        const list = pl;
        setProviders(list);
        if (defProvider && list.some(p => p.id === defProvider)) {
          chosenProvider = defProvider;
        } else if (list.length) {
          chosenProvider = list[0].id;
        } else {
          chosenProvider = '';
        }
        setSelectedProvider(chosenProvider);
        if (list.length === 0) {
          setInitError('No provider configured — open Settings to add an API key (xAI/OpenAI/Anthropic)');
          setShowSettings(true);
        }
      } catch(e){ setInitError(`Providers: ${String(e)}`)}
      setInitStep('Loading tools');
      try { const tl = await withTimeout(invoke<string[]>('list_tools'),5000,'list_tools'); setTools(tl.filter(t=> t !== 'test_tool')) } catch(e){ setInitError(`Tools: ${String(e)}`)}
      setInitStep('Opening session');
      try { await withTimeout(invoke('init_session', { sessionId: target, providerId: chosenProvider }), 10000, 'init_session'); } catch(e){
        setInitError(`${String(e)}`);
        // Retry with a brand-new session instead of a shared placeholder —
        // the placeholder used to leave a phantom `demo-session` directory in
        // the sidebar every time startup hiccuped.
        try {
          const fresh = `session-${Date.now()}`;
          await withTimeout(invoke('init_session',{sessionId:fresh, providerId: chosenProvider}),8000,'retry');
          setSessionId(fresh);
          setSessions(prev => (prev.includes(fresh) ? prev : [...prev, fresh]));
          setInitError(null);
        }catch{}
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

  // ModelPresetBadge dispatches `model-select` events; route them through the
  // existing provider-change flow so a badge click does the same hot-swap as
  // the old `<select>` onChange.
  useEffect(() => {
    const onModelSelect = (e: Event) => {
      const detail = (e as CustomEvent).detail;
      if (detail?.id) handleProviderChange(detail.id);
    };
    window.addEventListener('model-select', onModelSelect);
    return () => window.removeEventListener('model-select', onModelSelect);
  }, [handleProviderChange]);

  const refreshProviders = async () => {
    try {
      const pl = await invoke<ProviderInfo[]>('list_providers');
      const list = pl;
      setProviders(list);
      if (list.length > 0 && !list.some(p => p.id === selectedProvider)) {
        await handleProviderChange(list[0].id);
      }
    } catch(e){ console.error('refreshProviders failed', e); }
    // Settings also uses this callback after saving UI preferences.
    loadUiConfig();
  };

  const handleAddMcp = async (name:string, command:string, args:string[]) => {
    // Let the caller surface handshake failures — swallowing here made a failed
    // add look like a success (dialog closed, tool list unchanged).
    await invoke('add_mcp',{name,command,args});
    const tl=await invoke<string[]>('list_tools');
    setTools(tl.filter(t=> t!=='test_tool'));
  };

  if (!isInitialized) {
    return (
      <div style={{height:'100vh', display:'flex', alignItems:'center', justifyContent:'center', background:'var(--bg)', padding:'32px'}}>
        <div style={{width:'100%', maxWidth:'360px', textAlign:'center'}}>
          <div style={{display:'inline-flex', alignItems:'center', gap:'10px', marginBottom:'20px'}}>
            <ApertureLogo size={30} animated />
            <span style={{fontFamily:'var(--font-head)', fontWeight:650, fontSize:'16px', letterSpacing:'-0.02em'}}>Maverick</span>
          </div>
          <div className="panel" style={{padding:'16px', textAlign:'left'}}>
            <div className="mono" style={{fontSize:'11px', color:'var(--muted)', display:'flex', justifyContent:'space-between'}}>
              <span>{initStep}</span><span style={{color:'var(--accent)'}}>Syncing</span>
            </div>
            <div style={{height:'2px', background:'var(--line)', marginTop:'12px', borderRadius:999, overflow:'hidden'}}>
              <div style={{height:'100%', width:'42%', background:'var(--accent)', animation:'shim 1s ease-in-out infinite'}} />
            </div>
            {initError && <div className="mono" style={{marginTop:'12px', fontSize:'12px', color:'var(--error)', background:'var(--error-bg)', border:'1px solid var(--error-border)', padding:'8px 10px', borderRadius:'8px'}}>{initError}</div>}
          </div>
        </div>
      </div>
    );
  }

  const visibleProviders = providers;

  return (
    <div style={{display:'flex', height:'100vh', background:'var(--bg)', overflow:'hidden'}}>
      {showSidebar && (
        <div style={{width:'260px', minWidth:'260px', display:'flex', flexDirection:'column', background:'var(--panel-2)', borderRight:'1px solid var(--line)'}}>
          <SessionSidebar
            sessions={sessions}
            currentSession={sessionId}
            onSelect={handleSelectSession}
            onNew={handleNewSession}
            onDelete={handleDeleteSession}
            onRename={handleRenameSession}
            onOpenSettings={()=>setShowSettings(true)}
            onToggleSidebar={()=>setShowSidebar(false)}
          />
        </div>
      )}

      <div style={{flex:1, display:'flex', flexDirection:'column', minWidth:0, background:'var(--bg)'}}>
        <header style={{
          height:'52px', display:'flex', alignItems:'center', justifyContent:'space-between', padding:'0 16px',
          borderBottom:'1px solid var(--line)', background:'var(--header-bg)', backdropFilter:'blur(12px)', flexShrink:0
        }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
            {!showSidebar && (
              <button
                className="btn-ghost btn-ico"
                onClick={()=>setShowSidebar(true)}
                aria-label="Open sidebar"
                title="Open sidebar"
                style={{ padding: '6px', borderRadius: '8px', color: 'var(--muted)' }}
              >
                <PanelIcon size={16} />
              </button>
            )}
            <ModelPresetBadge providers={visibleProviders.length?visibleProviders:providers} selected={selectedProvider} onOpenSettings={()=>setShowSettings(true)} />
          </div>

          <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
            {!showSidebar && (
              <button
                className="btn-ghost btn-ico"
                onClick={handleNewSession}
                aria-label="New chat"
                title="New chat"
                style={{ padding: '6px', borderRadius: '8px', color: 'var(--muted)' }}
              >
                <PlusIcon size={16} />
              </button>
            )}
            <button
              className="btn-ghost btn-ico"
              onClick={()=>setShowSettings(true)}
              aria-label="Settings"
              title="Settings"
              style={{ padding: '6px', borderRadius: '8px', color: 'var(--muted)' }}
            >
              <SettingsIcon size={16} />
            </button>
          </div>
        </header>

        <div style={{flex:1, display:'flex', minWidth:0, overflow:'hidden', position:'relative'}}>
          <Chat
            sessionId={sessionId}
            onAddMcp={handleAddMcp}
            availableTools={tools}
            ui={uiConfig}
            providers={visibleProviders.length ? visibleProviders : providers}
            selectedProvider={selectedProvider}
            onProviderChange={handleProviderChange}
          />
        </div>
      </div>


      <Settings isOpen={showSettings} onClose={()=>setShowSettings(false)} providers={visibleProviders.length?visibleProviders:providers} currentProvider={selectedProvider} onProviderChange={handleProviderChange} onRefresh={refreshProviders} />
    </div>
  );
}
