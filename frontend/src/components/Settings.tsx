// Settings — minimal, English, no mock, no generic power badge
import React, { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import type { InteractionConfig, MemoryConfig, MemoryScope, MemoryStats, ModelPreset } from '../types';
import { DEFAULT_INTERACTION_CONFIG, DEFAULT_MEMORY_CONFIG } from '../types';

interface SettingsProps {
  isOpen: boolean; onClose: () => void;
  /** Visible session — scopes the memory editor + workspace default display. */
  sessionId?: string;
  providers: Array<{ id: string; name: string; model: string }>;
  currentProvider: string; onProviderChange: (id: string) => void;
  onRefresh?: () => void;
  /** Saved model presets (shared with the composer menu + header badge). */
  presets?: ModelPreset[];
  activePresetId?: string;
  onApplyPreset?: (p: ModelPreset) => void;
  onDeletePreset?: (id: string) => void;
  onSavePreset?: () => void;
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

/** Row shape returned by `list_kilo_models`. */
type KiloModelRow = {
  id: string;
  name: string;
  context_length?: number;
  is_free?: boolean;
  supported_parameters?: string[];
  /** Effort tiers from `opencode.variants`, ascending (`none`…`max`). */
  efforts?: string[];
};

/**
 * Split a shell-style argument string into argv, honouring single/double
 * quotes. A plain `.split(' ')` broke any argument containing a space
 * (`--flag "a b"` arrived as two truncated argv entries).
 */
function splitArgs(input: string): string[] {
  const out: string[] = [];
  let cur = '';
  let started = false;
  let quote: string | null = null;
  const flush = () => {
    if (started) { out.push(cur); cur = ''; started = false; }
  };
  for (const ch of input) {
    if (quote) {
      if (ch === quote) quote = null;
      else cur += ch;
      started = true;
      continue;
    }
    if (ch === '"' || ch === "'") { quote = ch; started = true; continue; }
    if (/\s/.test(ch)) { flush(); continue; }
    cur += ch;
    started = true;
  }
  flush();
  return out;
}

export default function Settings({ isOpen, onClose, sessionId, providers, currentProvider: _cp, onProviderChange, onRefresh, presets = [], activePresetId, onApplyPreset, onDeletePreset, onSavePreset }: SettingsProps) {
  const [apiKeys, setApiKeys] = useState<Record<string,string>>({});
  const [providerSettings, setProviderSettings] = useState<Record<string, {base_url?: string, model?: string, kind?: string}>>({});
  const [mcpServers, setMcpServers] = useState<Array<{name:string;transport:string;command?:string;args:string;url?:string;enabled:boolean}>>([]);
  const [mcpStatus, setMcpStatus] = useState<Array<{name:string;transport:string;tool_count:number;status:string;tools:string[]}>>([]);
  const [marketplace, setMarketplace] = useState<Array<{name:string;description:string;transport:string;command?:string;args:string[];url?:string;category:string;install_count?:number}>>([]);
  const [uiConfig, setUiConfig] = useState({ theme:'dark', show_tool_calls:true, auto_scroll:true, compact_mode:false });
  // Phase 3 context management (auto-compact checkpoint + per-tool output budgets).
  const [contextConfig, setContextConfig] = useState<{ auto_compact_enabled:boolean; auto_compact_threshold_percent:number; tail_keep_items:number; tool_output_budgets: Record<string, number> }>({ auto_compact_enabled:true, auto_compact_threshold_percent:85, tail_keep_items:24, tool_output_budgets:{} });
  // Turn/segment budgets + spend guardrails (§5.6-B/F).
  const [budgetConfig, setBudgetConfig] = useState<{ max_turns:number; max_segments:number; auto_continue:boolean; spend_cap_usd:number|null; avg_tokens_per_turn:number }>({ max_turns:40, max_segments:3, auto_continue:true, spend_cap_usd:null, avg_tokens_per_turn:2000 });
  // Clarifying questions (ask_user tool).
  const [interactionConfig, setInteractionConfig] = useState<InteractionConfig>(DEFAULT_INTERACTION_CONFIG);
  // Unified cross-chat memory (global + workspace MEMORY.md).
  const [memoryConfig, setMemoryConfig] = useState<MemoryConfig>(DEFAULT_MEMORY_CONFIG);
  const [memScope, setMemScope] = useState<'global'|'workspace'>('global');
  const [memText, setMemText] = useState('');
  const [memDirty, setMemDirty] = useState(false);
  const [memStats, setMemStats] = useState<MemoryStats|null>(null);
  // Default workspace (global fallback for sessions without their own).
  const [defaultWs, setDefaultWs] = useState<string|null>(null);
  const [wsDraft, setWsDraft] = useState('');
  const [defaultProvider, setDefaultProvider] = useState('');
  const [activeTab, setActiveTab] = useState<'providers'|'mcp'|'skills'|'ui'|'context'|'memory'>('providers');
  const [skills, setSkills] = useState<Array<{name:string, description:string, path:string, scope:string, display_name?: string, enabled:boolean}>>([]);
  const [newSkill, setNewSkill] = useState({ name:'', content:'' });
  const [hubFetch, setHubFetch] = useState({ owner:'', name:'', version:'', url:'https://agentskills.io' });
  const [skillSearch, setSkillSearch] = useState('');
  // Skills marketplace (GitHub-backed catalog).
  const [mpSources, setMpSources] = useState<Array<{id:string; display_name:string; owner:string; repo:string; branch:string; skills_path:string}>>([]);
  const [mpSourceId, setMpSourceId] = useState('');
  const [mpSkills, setMpSkills] = useState<Array<{source_id:string; dir:string; name:string; description:string; installed:boolean}>>([]);
  const [mpSearch, setMpSearch] = useState('');
  const [mpLoading, setMpLoading] = useState(false);
  const [newSource, setNewSource] = useState({ id:'', display_name:'', owner:'', repo:'', branch:'main', skills_path:'skills' });
  const [loading, setLoading] = useState(true);
  const [newMcp, setNewMcp] = useState({ name:'', transport:'stdio', command:'', args:'', url:'' });
  const [newCustom, setNewCustom] = useState({ id:'', base_url:'', model:'', api_key:'', kind:'openai' as 'openai'|'anthropic' });
  const [saving, setSaving] = useState<string|null>(null);
  const [showAdvanced, setShowAdvanced] = useState<Record<string, boolean>>({});
  const [customKeys, setCustomKeys] = useState<Record<string, string>>({});
  const [kiloModels, setKiloModels] = useState<KiloModelRow[]>([]);
  const [kiloLoading, setKiloLoading] = useState(false);
  const [kiloError, setKiloError] = useState<string|null>(null);
  const [showKiloPicker, setShowKiloPicker] = useState<string|null>(null); // providerId or 'new'
  const [kiloSearch, setKiloSearch] = useState('');
  const [kiloFilter, setKiloFilter] = useState<'all'|'free'|'paid'>('all');
  // Resizable panel: custom corner handle (always visible, unlike the native
  // `resize` grip). Size persists across opens via localStorage.
  const [panelSize, setPanelSize] = useState<{w:number;h:number}|null>(() => {
    try {
      const raw = localStorage.getItem('maverick.settingsSize');
      if (!raw) return null;
      const p = JSON.parse(raw);
      if (typeof p?.w === 'number' && typeof p?.h === 'number' && p.w >= 340 && p.h >= 360) return p;
    } catch { /* corrupted — fall back to defaults */ }
    return null;
  });
  const panelRef = useRef<HTMLDivElement>(null);
  const panelSizeRef = useRef(panelSize);
  panelSizeRef.current = panelSize;
  const startPanelResize = (e: React.PointerEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const startX = e.clientX, startY = e.clientY;
    const rect = panelRef.current?.getBoundingClientRect();
    const startW = panelSizeRef.current?.w ?? rect?.width ?? 800;
    const startH = panelSizeRef.current?.h ?? rect?.height ?? 600;
    const onMove = (ev: PointerEvent) => {
      const w = Math.min(window.innerWidth - 32, Math.max(340, Math.round(startW + ev.clientX - startX)));
      const h = Math.min(window.innerHeight - 32, Math.max(360, Math.round(startH + ev.clientY - startY)));
      setPanelSize({ w, h });
    };
    const onUp = () => {
      document.removeEventListener('pointermove', onMove);
      document.removeEventListener('pointerup', onUp);
      document.body.style.cursor = '';
      try {
        const cur = panelSizeRef.current;
        if (cur) localStorage.setItem('maverick.settingsSize', JSON.stringify(cur));
      } catch { /* private mode — session-only size */ }
    };
    document.addEventListener('pointermove', onMove);
    document.addEventListener('pointerup', onUp);
    document.body.style.cursor = 'nwse-resize';
  };

  useEffect(()=>{ if(isOpen) { loadConfig(); loadSkills(); loadMcpStatus(); loadMarketplace(); loadMpSources(); loadMemoryStats(); loadDefaultWs(); } },[isOpen]);
  // Reload the editor whenever the file scope flips.
  useEffect(()=>{ if(isOpen) loadMemoryText(memScope); },[isOpen, memScope]);
  // Browse the selected marketplace source (first browse auto-fetches).
  useEffect(()=>{ if(isOpen && mpSourceId) loadMpSkills(false); },[mpSourceId]);
  const loadSkills = async () => {
    try {
      const list = await invoke<Array<{name:string, description:string, path:string, scope:string, display_name?:string, enabled:boolean}>>('list_skills');
      setSkills(list);
    } catch(e){ console.error('list_skills failed', e); }
  };
  const loadMcpStatus = async () => {
    try {
      const st = await invoke<Array<{name:string,transport:string,tool_count:number,status:string,tools:string[]}>>('list_mcp_status');
      setMcpStatus(st);
    } catch(e){ console.error('list_mcp_status failed', e); }
  };
  const loadMarketplace = async () => {
    try {
      const mp = await invoke<Array<{name:string,description:string,transport:string,command?:string,args:string[],url?:string,category:string,install_count?:number}>>('scan_marketplace');
      setMarketplace(mp);
    } catch(e){ console.error('scan_marketplace failed', e); }
  };

  const fetchKiloModels = async (target: string) => {
    setShowKiloPicker(target); setKiloLoading(true); setKiloError(null);
    try {
      const list = await invoke<KiloModelRow[]>('list_kilo_models', {
        providerId: target === 'new' ? null : target,
        baseUrl: target === 'new' ? (newCustom.base_url.trim() || 'https://api.kilo.ai/api/gateway') : null,
        apiKey: target === 'new' ? (newCustom.api_key.trim() || null) : null,
      });
      setKiloModels(list);
    } catch(e){ setKiloError(String(e)); }
    finally{ setKiloLoading(false); }
  };

  const selectKiloModel = async (modelId: string) => {
    if (showKiloPicker === 'new') {
      setNewCustom(prev => ({ ...prev, id: 'kilo', base_url: prev.base_url.trim() || 'https://api.kilo.ai/api/gateway', model: modelId, kind: 'openai' }));
      setShowKiloPicker(null);
      return;
    }

    const providerId = showKiloPicker;
    if (!providerId) return;
    const settings = providerSettings[providerId] || {};
    setSaving(providerId);
    try {
      const key = customKeys[providerId]?.trim();
      if (key) await invoke('set_api_key', { providerId, apiKey: key });
      await invoke('set_provider_settings', {
        providerId,
        baseUrl: settings.base_url?.trim() || null,
        model: modelId,
        kind: settings.kind?.trim() || null,
      });
      setProviderSettings(prev => ({ ...prev, [providerId]: { ...(prev[providerId] || {}), model: modelId } }));
      setCustomKeys(prev => { const next = { ...prev }; delete next[providerId]; return next; });
      onRefresh?.();
      setShowKiloPicker(null);
    } catch (e) {
      setKiloError(String(e));
    } finally {
      setSaving(null);
    }
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
      if(c.context) setContextConfig({
        auto_compact_enabled: c.context.auto_compact_enabled ?? true,
        auto_compact_threshold_percent: c.context.auto_compact_threshold_percent ?? 85,
        tail_keep_items: c.context.tail_keep_items ?? 24,
        tool_output_budgets: c.context.tool_output_budgets || {},
      });
      if(c.budget) setBudgetConfig({
        max_turns: c.budget.max_turns ?? 40,
        max_segments: c.budget.max_segments ?? 3,
        auto_continue: c.budget.auto_continue ?? true,
        spend_cap_usd: c.budget.spend_cap_usd ?? null,
        avg_tokens_per_turn: c.budget.avg_tokens_per_turn ?? 2000,
      });
      if(c.interaction) setInteractionConfig({
        ask_user_enabled: c.interaction.ask_user_enabled ?? true,
      });
      if(c.memory) setMemoryConfig({
        enabled: c.memory.enabled ?? true,
        auto_extract: c.memory.auto_extract ?? true,
        scope: (c.memory.scope as MemoryScope) ?? 'both',
        extract_model: c.memory.extract_model ?? DEFAULT_MEMORY_CONFIG.extract_model,
        max_chars: c.memory.max_chars ?? DEFAULT_MEMORY_CONFIG.max_chars,
      });
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
      await invoke('add_mcp_server_full',{name:newMcp.name, transport:newMcp.transport, command: newMcp.transport==='stdio'?newMcp.command:undefined, args:splitArgs(newMcp.args), url: newMcp.transport==='http'?newMcp.url:undefined});
      loadConfig(); loadMcpStatus(); setNewMcp({name:'',transport:'stdio',command:'',args:'',url:''});
    }catch(e){ alert(String(e))} finally{ setSaving(null)}
  };
  const removeMcpServer=async(name:string)=>{
    if(!confirm(`Remove MCP server "${name}"?`)) return;
    try{ await invoke('remove_mcp',{name}); loadConfig(); loadMcpStatus(); }catch(e){ alert(String(e))}
  };
  const installMarketplace = async (entry: any)=>{
    setSaving('mp-'+entry.name);
    try{
      await invoke('add_mcp_server_full',{name: entry.name, transport: entry.transport, command: entry.command, args: entry.args || [], url: entry.url});
      loadConfig(); loadMcpStatus();
    }catch(e){ alert(String(e))} finally{ setSaving(null)}
  };
  const handleUiChange=async(k:string,v:boolean)=>{
    const nc={...uiConfig,[k]:v}; setUiConfig(nc);
    try{ await invoke('set_ui_config',{ui:nc}); }catch(e){ console.error(e)}
    // The app shell reads these — tell it to re-read, or the toggle only
    // takes effect after a restart.
    onRefresh?.();
  };
  // Phase 3 context config: apply a patch, persist, and return the next value
  // (avoids stale-closure reads when several fields change in one gesture).
  const patchContext = (patch: Partial<typeof contextConfig>) => {
    const next = { ...contextConfig, ...patch };
    setContextConfig(next);
    invoke('set_context_config',{context:next}).catch(e=>alert(String(e)));
    return next;
  };
  // Turn/segment budgets: same patch-and-persist pattern as context.
  const patchBudget = (patch: Partial<typeof budgetConfig>) => {
    const next = { ...budgetConfig, ...patch };
    setBudgetConfig(next);
    invoke('set_budget_config',{budget:next}).catch(e=>alert(String(e)));
    return next;
  };
  // Clarifying questions: same patch-and-persist pattern.
  const patchInteraction = (patch: Partial<InteractionConfig>) => {
    const next = { ...interactionConfig, ...patch };
    setInteractionConfig(next);
    invoke('set_interaction_config',{interaction:next}).catch(e=>alert(String(e)));
    return next;
  };
  // Unified memory: same patch-and-persist pattern.
  const patchMemory = (patch: Partial<MemoryConfig>) => {
    const next = { ...memoryConfig, ...patch };
    setMemoryConfig(next);
    invoke('set_memory_config',{memory:next}).catch(e=>alert(String(e)));
    return next;
  };
  const loadMemoryText = async (scope: 'global'|'workspace') => {
    try{
      const text = await invoke<string>('get_memory_text',{scope, sessionId: sessionId ?? null});
      setMemText(text); setMemDirty(false);
    }catch(e){ alert(String(e)) }
  };
  const loadMemoryStats = async () => {
    try{
      const s = await invoke<MemoryStats>('get_memory_stats',{sessionId: sessionId ?? null});
      setMemStats(s);
    }catch(e){ console.error('get_memory_stats failed', e) }
  };
  const saveMemoryText = async () => {
    setSaving('memory');
    try{
      await invoke('save_memory_text',{scope:memScope, text:memText, sessionId: sessionId ?? null});
      setMemDirty(false); loadMemoryStats();
    }catch(e){ alert(String(e)) } finally{ setSaving(null) }
  };
  const clearMemory = async (scope: 'global'|'workspace'|'both') => {
    if(!confirm(`Delete ${scope === 'both' ? 'ALL' : scope} memor${scope === 'both' ? 'ies' : 'y'}? This cannot be undone.`)) return;
    setSaving('memory-clear');
    try{
      await invoke('clear_memory',{scope, sessionId: sessionId ?? null});
      loadMemoryText(memScope); loadMemoryStats();
    }catch(e){ alert(String(e)) } finally{ setSaving(null) }
  };
  const loadDefaultWs = async () => {
    try{
      const d = await invoke<string|null>('get_default_workspace');
      setDefaultWs(d);
      setWsDraft(d ?? '');
    }catch(e){ console.error('get_default_workspace failed', e) }
  };
  const saveDefaultWs = async (path: string | null) => {
    setSaving('workspace');
    try{
      const d = await invoke<string|null>('set_default_workspace',{path});
      setDefaultWs(d);
      setWsDraft(d ?? '');
    }catch(e){ alert(String(e)) } finally{ setSaving(null) }
  };
  const browseDefaultWs = async () => {
    try{
      const picked = await open({ directory: true, multiple: false, title: 'Choose default workspace' });
      if (typeof picked === 'string' && picked) saveDefaultWs(picked);
    }catch(e){ alert(String(e)) }
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
  const loadMpSources = async()=>{
    try{
      const list = await invoke<Array<{id:string; display_name:string; owner:string; repo:string; branch:string; skills_path:string}>>('list_marketplace_sources');
      setMpSources(list);
      setMpSourceId(prev=> list.some(s=>s.id===prev) ? prev : (list[0]?.id || ''));
    }catch(e){ console.error('list_marketplace_sources failed', e); }
  };
  const loadMpSkills = async(refresh=false)=>{
    if(!mpSourceId) return;
    setMpLoading(true);
    try{
      const list = await invoke<Array<{source_id:string; dir:string; name:string; description:string; installed:boolean}>>('list_marketplace_skills',{sourceId: mpSourceId, refresh});
      setMpSkills(list);
    }catch(e){ alert(String(e)) } finally{ setMpLoading(false) }
  };
  const searchMpSkills = async()=>{
    if(!mpSourceId) return;
    const q = mpSearch.trim();
    setMpLoading(true);
    try{
      const list = q
        ? await invoke<Array<{source_id:string; dir:string; name:string; description:string; installed:boolean}>>('search_marketplace_skills',{sourceId: mpSourceId, query: q})
        : await invoke<Array<{source_id:string; dir:string; name:string; description:string; installed:boolean}>>('list_marketplace_skills',{sourceId: mpSourceId, refresh: false});
      setMpSkills(list);
    }catch(e){ alert(String(e)) } finally{ setMpLoading(false) }
  };
  const installMpSkill = async(skill: {source_id:string; dir:string; name:string})=>{
    setSaving('mp-skill-'+skill.dir);
    try{
      await invoke('install_marketplace_skill',{sourceId: skill.source_id, dir: skill.dir});
      loadSkills();
      setMpSkills(prev=> prev.map(s=> s.dir===skill.dir ? {...s, installed:true} : s));
      onRefresh?.();
    }catch(e){ alert(String(e)) } finally{ setSaving(null) }
  };
  const addMpSource = async()=>{
    if(!newSource.id.trim() || !newSource.owner.trim() || !newSource.repo.trim()){ alert('Source id, owner and repo are required'); return; }
    setSaving('mp-source');
    try{
      const list = await invoke<Array<{id:string; display_name:string; owner:string; repo:string; branch:string; skills_path:string}>>('add_marketplace_source',{
        id: newSource.id, displayName: newSource.display_name, owner: newSource.owner, repo: newSource.repo,
        branch: newSource.branch.trim() || null, skillsPath: newSource.skills_path.trim() || null,
      });
      setMpSources(list);
      // Backend sanitizes the id the same way (lowercase, non [a-z0-9_-] → '-').
      const predicted = newSource.id.trim().toLowerCase().replace(/[^a-z0-9_-]/g,'-').replace(/^-+|-+$/g,'');
      setMpSourceId(list.some(s=>s.id===predicted) ? predicted : (list[0]?.id || ''));
      setNewSource({id:'', display_name:'', owner:'', repo:'', branch:'main', skills_path:'skills'});
    }catch(e){ alert(String(e)) } finally{ setSaving(null) }
  };
  const removeMpSource = async(id:string)=>{
    if(!confirm(`Remove marketplace source "${id}"? Installed skills stay installed.`)) return;
    try{
      const list = await invoke<Array<{id:string; display_name:string; owner:string; repo:string; branch:string; skills_path:string}>>('remove_marketplace_source',{id});
      setMpSources(list);
      setMpSourceId(prev=> prev===id ? (list[0]?.id || '') : prev);
      if(!list.length) setMpSkills([]);
    }catch(e){ alert(String(e)) }
  };

  if(!isOpen) return null;

  return (
    <div style={{position:'fixed', inset:0, background:'rgba(0,0,0,0.45)', backdropFilter:'blur(8px)', display:'flex', alignItems:'center', justifyContent:'center', zIndex:100, padding:'16px'}}>
      <div ref={panelRef} onClick={e=>e.stopPropagation()} style={{width: panelSize ? `${panelSize.w}px` : '100%', maxWidth:'min(960px, calc(100vw - 32px))', minWidth:'340px', height: panelSize ? `${panelSize.h}px` : undefined, maxHeight:'calc(100vh - 32px)', minHeight:'360px', position:'relative', display:'flex', flexDirection:'column', background:'var(--panel)', border:'1px solid var(--line)', borderRadius:'16px', overflow:'hidden'}}>
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
          {(['providers','mcp','skills','ui','context','memory'] as const).map(tab=>(
            <button key={tab} onClick={()=>setActiveTab(tab)} style={{
              flex:1, padding:'8px', borderRadius:'999px', border:'none',
              background: activeTab===tab ? 'var(--text)' : 'transparent',
              color: activeTab===tab ? 'var(--bg)' : 'var(--muted)',
              fontSize:'12px', margin:'8px 0'
            }}>
              {tab === 'providers' ? 'Providers' : tab === 'mcp' ? 'MCP' : tab === 'skills' ? 'Skills' : tab === 'context' ? 'Context' : tab === 'memory' ? 'Memory' : 'Interface'}
            </button>
          ))}
        </div>

        <div style={{flex:1, minHeight:0, overflow:'auto', padding:'20px'}}>
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
                  <div>
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'6px'}}>Model presets — provider + model + effort</div>
                    {presets.length === 0 ? (
                      <div className="mono" style={{fontSize:'11px', color:'var(--muted)'}}>None yet — save combos from the composer model menu or the header badge.</div>
                    ) : (
                      <div style={{display:'flex', flexDirection:'column', gap:'6px'}}>
                        {presets.map(p => {
                          const active = p.id === activePresetId;
                          return (
                            <div key={p.id} style={{display:'flex', alignItems:'center', gap:'8px', padding:'8px 10px', border:'1px solid var(--line)', borderRadius:'10px', background:'var(--bg)'}}>
                              <div style={{flex:1, minWidth:0}}>
                                <div style={{display:'flex', gap:'6px', alignItems:'center'}}>
                                  <span style={{fontWeight:600, fontSize:'12px', overflow:'hidden', textOverflow:'ellipsis', whiteSpace:'nowrap'}}>{p.name}</span>
                                  {active && <span className="mono" style={{fontSize:'10px', color:'var(--ok-text)', border:'1px solid var(--ok-border)', padding:'1px 6px', borderRadius:999}}>active</span>}
                                </div>
                                <div className="mono" style={{fontSize:'11px', color:'var(--muted)', overflow:'hidden', textOverflow:'ellipsis', whiteSpace:'nowrap'}}>{p.provider_id} · {p.model}{p.effort ? ` · ${p.effort}` : ''}</div>
                              </div>
                              <button className="btn-ghost" onClick={()=> onApplyPreset?.(p)} style={{fontSize:'11px', padding:'4px 10px', borderRadius:'999px', border:'1px solid var(--line)'}}>Use</button>
                              <button className="btn-ghost" onClick={()=> onDeletePreset?.(p.id)} aria-label={`Delete preset ${p.name}`} style={{fontSize:'13px', padding:'2px 8px', borderRadius:'999px', color:'var(--danger)'}}>×</button>
                            </div>
                          );
                        })}
                      </div>
                    )}
                    <button className="btn-ghost" onClick={()=> onSavePreset?.()} style={{marginTop:'8px', fontSize:'11px', padding:'6px 10px', borderRadius:'999px', border:'1px dashed var(--line)'}}>Save current as preset…</button>
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
                          {isConfigured && <span className="mono" style={{marginLeft:'auto', fontSize:'10px', color:'var(--ok-text)', border:'1px solid var(--ok-border)', padding:'2px 6px', borderRadius:999}}>configured</span>}
                          <button className="btn-ghost" onClick={()=> setShowAdvanced(prev=> ({...prev, [p.id]: !prev[p.id]}))} style={{marginLeft: isConfigured ? '0' : 'auto', fontSize:'10px', padding:'4px 8px', borderRadius:'999px'}}>
                            {isAdvanced ? 'Hide' : 'Base URL / Model'}
                          </button>
                        </div>
                        <div style={{display:'flex', gap:'8px', marginBottom: isAdvanced ? '8px' : '0'}}>
                          <input type="password" placeholder={isConfigured ? '•••••••• (saved) — enter new to replace' : 'sk-...'} value={apiKeys[p.id]||''} onChange={e=>handleApiKeyChange(p.id,e.target.value)} style={{flex:1}} />
                          <button onClick={()=>saveProviderConfig(p.id)} disabled={saving===p.id} style={{borderRadius:'999px', background: isConfigured ? 'var(--panel)' : 'var(--text)', color: isConfigured ? 'var(--text)' : 'var(--bg)'}}>{saving===p.id ? '…' : isConfigured ? 'Save' : 'Save'}</button>
                          {isConfigured && <button className="btn-ghost" onClick={()=>removeApiKey(p.id)} style={{color:'var(--danger)', borderRadius:'999px'}}>Remove</button>}
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
                           <button className="btn-ghost" onClick={()=> fetchKiloModels('new')} style={{whiteSpace:'nowrap', borderRadius:'999px'}}>Browse models</button>
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
                              <span className="mono" style={{fontSize:'10px', color:'var(--muted)'}}>{s.model || p.model || '—'}</span>
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
                            <button className="btn-ghost" onClick={async()=>{ if(confirm(`Remove ${p.id}?`)){ await invoke('remove_api_key',{providerId:p.id}); await invoke('set_provider_settings',{providerId:p.id, baseUrl:null, model:null, kind:null}); onRefresh?.(); loadConfig(); } }} style={{color:'var(--danger)', borderRadius:'999px', alignSelf:'flex-start'}}>Remove</button>
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
                  {mcpServers.length===0 ? <div className="mono" style={{textAlign:'center', padding:'16px', color:'var(--muted)', fontSize:'12px'}}>No MCP servers configured</div> : (
                    <div style={{display:'flex', flexDirection:'column', gap:'8px'}}>
                      {mcpServers.map(s=>{
                        const st = mcpStatus.find(x=> x.name===s.name);
                        return (
                        <div key={s.name} style={{padding:'12px', border:'1px solid var(--line)', borderRadius:'12px', display:'flex', flexDirection:'column', gap:'8px', background:'var(--bg)'}}>
                          <div style={{display:'flex', alignItems:'center', gap:'12px'}}>
                            <div style={{flex:1, minWidth:0}}>
                              <div style={{display:'flex', gap:'8px', alignItems:'center', flexWrap:'wrap'}}><span style={{fontWeight:600, fontSize:'13px'}}>{s.name}</span><span className="badge mono">{s.transport}</span>{st && <span className="mono" style={{fontSize:'10px', padding:'2px 6px', borderRadius:999, border:'1px solid var(--line)', color: st.status==='Ready'?'var(--ok-text)': st.status==='Placeholder'?'var(--muted)':'var(--danger)'}}>{st.status} • {st.tool_count} tools</span>}</div>
                              <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginTop:'4px', whiteSpace:'nowrap', overflow:'hidden', textOverflow:'ellipsis'}}>{s.transport==='stdio' ? `${s.command} ${s.args}` : s.url}</div>
                            </div>
                            <button className="btn-ghost" onClick={()=>removeMcpServer(s.name)} style={{color:'var(--danger)', borderRadius:'999px'}}>Remove</button>
                          </div>
                          {st && st.tools.length>0 && <div className="mono" style={{fontSize:'10px', color:'var(--muted)', background:'var(--panel)', border:'1px solid var(--line)', padding:'6px 8px', borderRadius:'8px', wordBreak:'break-all'}}>{st.tools.slice(0,8).join(' • ')}{st.tools.length>8 ? ` +${st.tools.length-8} more` : ''}</div>}
                        </div>
                      )})}
                    </div>
                  )}
                  <div style={{height:'1px', background:'var(--line)'}} />
                  <div style={{padding:'16px', border:'1px dashed var(--line)', borderRadius:'12px', background:'var(--panel)'}}>
                    <div style={{display:'flex', alignItems:'center', gap:'8px', marginBottom:'10px'}}><span style={{fontWeight:600, fontSize:'13px'}}>Marketplace</span><span className="badge mono">{marketplace.length} servers</span><button className="btn-ghost" onClick={loadMarketplace} style={{marginLeft:'auto', fontSize:'11px', padding:'4px 8px', borderRadius:'999px'}}>Refresh</button></div>
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'12px'}}>Full marketplace via <span style={{color:'var(--text)'}}>xai-org/plugin-marketplace</span> scaffold — one-click install. Hub cache is <span style={{color:'var(--text)'}}>Server</span> scope.</div>
                    <div style={{display:'flex', flexDirection:'column', gap:'8px', maxHeight:'280px', overflowY:'auto'}}>
                      {marketplace.map(entry=>(
                        <div key={entry.name} style={{padding:'12px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)', display:'flex', gap:'12px', alignItems:'center'}}>
                          <div style={{flex:1, minWidth:0}}>
                            <div style={{display:'flex', gap:'8px', alignItems:'center', flexWrap:'wrap'}}><span style={{fontWeight:600, fontSize:'13px'}}>{entry.name}</span><span className="badge mono">{entry.category}</span><span className="mono" style={{fontSize:'10px', color:'var(--muted)'}}>{entry.transport}</span>{entry.install_count && <span className="mono" style={{fontSize:'10px', color:'var(--faint)'}}>{entry.install_count.toLocaleString()} installs</span>}</div>
                            <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginTop:'4px', lineHeight:1.4}}>{entry.description}</div>
                            <div className="mono" style={{fontSize:'10px', color:'var(--faint)', marginTop:'2px'}}>{entry.transport==='stdio' ? `${entry.command} ${(entry.args||[]).join(' ')}` : entry.url}</div>
                          </div>
                          <button onClick={()=> installMarketplace(entry)} disabled={saving==='mp-'+entry.name || mcpServers.some(s=> s.name===entry.name)} style={{borderRadius:'999px', whiteSpace:'nowrap', fontSize:'12px', padding:'8px 14px'}}>{mcpServers.some(s=> s.name===entry.name) ? 'Installed' : saving==='mp-'+entry.name ? '…' : 'Install'}</button>
                        </div>
                      ))}
                    </div>
                  </div>
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
                            {s.enabled ? <span className="mono" style={{fontSize:'10px', color:'var(--ok-text)', border:'1px solid var(--ok-border)', padding:'1px 6px', borderRadius:999}}>enabled</span> : <span className="mono" style={{fontSize:'10px', color:'var(--faint)', border:'1px solid var(--line)', padding:'1px 6px', borderRadius:999}}>disabled</span>}
                            <button className="btn-ghost" onClick={()=> handleRemoveSkill(s.name)} style={{marginLeft:'auto', color:'var(--danger)', borderRadius:'999px', fontSize:'11px', padding:'4px 8px'}}>Remove</button>
                          </div>
                          <div className="mono" style={{fontSize:'11px', color:'var(--muted)', lineHeight:1.5}}>{s.description || 'No description — add one in SKILL.md frontmatter'}</div>
                          <div className="mono" style={{fontSize:'10px', color:'var(--faint)', wordBreak:'break-all'}}>{s.path}</div>
                        </div>
                      ))}
                    </div>
                  )}

                  <div style={{height:'1px', background:'var(--line)'}} />

                  <div style={{padding:'16px', border:'1px dashed var(--line)', borderRadius:'12px', background:'var(--panel)'}}>
                    <div style={{display:'flex', alignItems:'center', gap:'8px', marginBottom:'4px'}}>
                      <span style={{fontWeight:600, fontSize:'13px'}}>Marketplace</span>
                      <span className="badge mono">{mpSkills.length} skills</span>
                      <button className="btn-ghost" onClick={()=>loadMpSkills(true)} disabled={mpLoading || !mpSourceId} style={{marginLeft:'auto', fontSize:'11px', padding:'4px 8px', borderRadius:'999px'}}>Refresh</button>
                    </div>
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'10px'}}>Browse GitHub skill repos, install with one click. Catalogs are cached locally — installs land in <span style={{color:'var(--text)'}}>~/.maverick/hub/skills/marketplace/</span>. Refresh hits the GitHub API (60 req/hr unauthenticated).</div>
                    <div style={{display:'flex', gap:'8px', marginBottom:'8px', flexWrap:'wrap', alignItems:'center'}}>
                      <select value={mpSourceId} onChange={e=>setMpSourceId(e.target.value)} style={{flex:1, minWidth:'180px'}}>
                        {mpSources.length===0 && <option value="">No sources</option>}
                        {mpSources.map(s=> <option key={s.id} value={s.id}>{s.display_name || s.id} • {s.owner}/{s.repo}</option>)}
                      </select>
                      {mpSourceId && <button className="btn-ghost" onClick={()=>removeMpSource(mpSourceId)} style={{color:'var(--danger)', borderRadius:'999px', fontSize:'11px', padding:'4px 8px', whiteSpace:'nowrap'}}>Remove source</button>}
                    </div>
                    <div style={{display:'flex', gap:'8px', marginBottom:'10px'}}>
                      <input placeholder="Search marketplace • name or description" value={mpSearch} onChange={e=>setMpSearch(e.target.value)} onKeyDown={e=>{ if(e.key==='Enter') searchMpSkills(); }} style={{flex:1}} />
                      <button className="btn-ghost" onClick={searchMpSkills} disabled={mpLoading || !mpSourceId} style={{borderRadius:'999px', fontSize:'11px', padding:'6px 10px', whiteSpace:'nowrap'}}>Search</button>
                    </div>
                    {mpLoading
                      ? <div className="mono" style={{textAlign:'center', padding:'20px', color:'var(--muted)', fontSize:'12px'}}>Loading catalog…</div>
                      : mpSkills.length===0
                        ? <div className="mono" style={{textAlign:'center', padding:'20px', color:'var(--muted)', fontSize:'12px'}}>{mpSourceId ? 'No skills cached — press Refresh to fetch the catalog' : 'Add a marketplace source below'}</div>
                        : (
                          <div style={{display:'flex', flexDirection:'column', gap:'8px', maxHeight:'240px', overflowY:'auto'}}>
                            {mpSkills.map(m=>(
                              <div key={m.dir} style={{padding:'12px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)', display:'flex', gap:'12px', alignItems:'center'}}>
                                <div style={{flex:1, minWidth:0}}>
                                  <div style={{display:'flex', gap:'8px', alignItems:'center', flexWrap:'wrap'}}>
                                    <span style={{fontWeight:600, fontSize:'13px'}}>{m.name}</span>
                                    {m.installed && <span className="mono" style={{fontSize:'10px', color:'var(--ok-text)', border:'1px solid var(--ok-border)', padding:'1px 6px', borderRadius:999}}>installed</span>}
                                  </div>
                                  <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginTop:'4px', lineHeight:1.4}}>{m.description || 'No description'}</div>
                                  <div className="mono" style={{fontSize:'10px', color:'var(--faint)', marginTop:'2px'}}>{m.dir}/SKILL.md</div>
                                </div>
                                <button onClick={()=>installMpSkill(m)} disabled={m.installed || saving==='mp-skill-'+m.dir} style={{borderRadius:'999px', whiteSpace:'nowrap', fontSize:'12px', padding:'8px 14px'}}>{m.installed ? 'Installed' : saving==='mp-skill-'+m.dir ? '…' : 'Install'}</button>
                              </div>
                            ))}
                          </div>
                        )}
                    <div style={{marginTop:'10px', paddingTop:'10px', borderTop:'1px solid var(--line)'}}>
                      <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'8px'}}>Add source • any GitHub repo with SKILL.md files</div>
                      <div style={{display:'flex', gap:'8px', flexWrap:'wrap'}}>
                        <input placeholder="id • e.g. my-skills" value={newSource.id} onChange={e=>setNewSource({...newSource, id: e.target.value})} style={{flex:1, minWidth:'110px'}} />
                        <input placeholder="Display name" value={newSource.display_name} onChange={e=>setNewSource({...newSource, display_name: e.target.value})} style={{flex:1, minWidth:'110px'}} />
                        <input placeholder="Owner" value={newSource.owner} onChange={e=>setNewSource({...newSource, owner: e.target.value})} style={{flex:1, minWidth:'90px'}} />
                        <input placeholder="Repo" value={newSource.repo} onChange={e=>setNewSource({...newSource, repo: e.target.value})} style={{flex:1, minWidth:'90px'}} />
                        <input placeholder="Branch • main" value={newSource.branch} onChange={e=>setNewSource({...newSource, branch: e.target.value})} style={{width:'100px'}} />
                        <input placeholder="Path • skills" value={newSource.skills_path} onChange={e=>setNewSource({...newSource, skills_path: e.target.value})} style={{width:'100px'}} />
                        <button onClick={addMpSource} disabled={saving==='mp-source'} style={{borderRadius:'999px', whiteSpace:'nowrap'}}>{saving==='mp-source' ? 'Adding…' : 'Add'}</button>
                      </div>
                    </div>
                  </div>

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
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'10px'}}>Supports <span style={{color:'var(--text)'}}>agentskills.io</span> and any raw SKILL.md URL. For browsing, use the <span style={{color:'var(--text)'}}>Marketplace</span> above — same hub cache, `Server` scope.</div>
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
                    Skills are loaded into <span style={{color:'var(--text)'}}>ToolBridge</span> as the `skill` tool — the model sees them as <span style={{color:'var(--text)'}}>available_skills</span> and can call `skill(name: "...")` to inject instructions. Add a skill, then prompt “use skill X” in chat. Marketplace installs fetch SKILL.md only — bundled `scripts/` and `references/` folders are not downloaded yet.
                  </div>
                </div>
              )}

              {activeTab==='ui' && (
                <div style={{display:'flex', flexDirection:'column', gap:'16px'}}>
                  <div style={{padding:'16px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)'}}>
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'12px'}}>Theme</div>
                    <div style={{display:'flex', gap:'8px'}}>
                      {(['dark','light','system'] as const).map(t=>(
                        <button key={t} onClick={async ()=>{ const n={...uiConfig, theme:t}; setUiConfig(n); try{ await invoke('set_ui_config',{ui:n}); }catch(e){ console.error(e); } onRefresh?.(); }} style={{flex:1, background: uiConfig.theme===t ? 'var(--text)' : 'transparent', color: uiConfig.theme===t ? 'var(--bg)' : 'var(--muted)', borderRadius:'999px'}}>{t.charAt(0).toUpperCase()+t.slice(1)}</button>
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
                  <div style={{padding:'16px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)'}}>
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'4px'}}>Default workspace</div>
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'12px'}}>
                      Fallback folder for chats without their own (per-chat folder via the header chip).
                      {defaultWs ? '' : ' Not set — uses ./workspace (or MAVERICK_WORKSPACE_DIR).'}
                    </div>
                    {defaultWs && (
                      <div className="mono" style={{fontSize:'12px', color:'var(--text)', wordBreak:'break-all', marginBottom:'10px'}}>{defaultWs}</div>
                    )}
                    <div style={{display:'flex', gap:'8px', flexWrap:'wrap', alignItems:'center'}}>
                      <input
                        value={wsDraft}
                        onChange={e=>setWsDraft(e.target.value)}
                        onKeyDown={e=>{ if(e.key==='Enter') saveDefaultWs(wsDraft.trim() || null); }}
                        placeholder="Paste a folder path…"
                        style={{flex:1, minWidth:'180px', fontSize:'12px'}}
                      />
                      <button className="btn-ghost" onClick={()=>saveDefaultWs(wsDraft.trim() || null)} disabled={saving==='workspace'} style={{borderRadius:999}}>Set</button>
                      <button className="btn-ghost" onClick={browseDefaultWs} disabled={saving==='workspace'} style={{borderRadius:999}}>Browse…</button>
                      {defaultWs && (
                        <button className="btn-ghost" onClick={()=>saveDefaultWs(null)} disabled={saving==='workspace'} style={{borderRadius:999}}>Clear</button>
                      )}
                    </div>
                  </div>
                </div>
              )}
              {activeTab==='context' && (
                <div style={{display:'flex', flexDirection:'column', gap:'16px'}}>
                  <div className="mono" style={{fontSize:'11px', color:'var(--muted)'}}>
                    Context management runs a checkpoint between auto-continue segments: older history is
                    folded into one summary item (system prompt + the newest items stay verbatim, open todos
                    are kept) so long tasks do not blow the window.
                  </div>

                  <div style={{padding:'16px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)'}}>
                    <label style={{display:'flex', alignItems:'center', gap:'10px', cursor:'pointer', fontSize:'13px'}}>
                      <input type="checkbox" checked={contextConfig.auto_compact_enabled}
                        onChange={e=>patchContext({ auto_compact_enabled:e.target.checked })}
                        style={{width:'16px', height:'16px', accentColor:'var(--text)'}} />
                      <span>Enable auto-compact checkpoints</span>
                    </label>
                    <div className="mono" style={{marginTop:'8px', fontSize:'11px', color:'var(--muted)'}}>
                      Off = history grows unbounded until the provider rejects the request.
                    </div>
                  </div>

                  <div style={{padding:'16px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)'}}>
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'12px'}}>Trigger</div>
                    <label style={{display:'flex', flexDirection:'column', gap:'6px', fontSize:'13px'}}>
                      <span>Compact at <b>{contextConfig.auto_compact_threshold_percent}%</b> of the context window</span>
                      <input type="range" min={50} max={100} step={5}
                        value={contextConfig.auto_compact_threshold_percent}
                        disabled={!contextConfig.auto_compact_enabled}
                        onChange={e=>patchContext({ auto_compact_threshold_percent: Number(e.target.value) })}
                        style={{width:'100%'}} />
                    </label>
                    <label style={{display:'flex', flexDirection:'column', gap:'6px', fontSize:'13px', marginTop:'12px'}}>
                      <span>Keep the newest <b>{contextConfig.tail_keep_items}</b> messages verbatim</span>
                      <input type="number" min={4} max={200} value={contextConfig.tail_keep_items}
                        disabled={!contextConfig.auto_compact_enabled}
                        onChange={e=>patchContext({ tail_keep_items: Number(e.target.value) })}
                        style={{width:'120px'}} />
                    </label>
                  </div>

                  <div style={{padding:'16px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)'}}>
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'12px'}}>Per-tool output budgets (bytes)</div>                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'10px'}}>
                      Tool results longer than the budget are truncated head+tail before entering history.
                      Default is 12288 bytes for any tool not listed here.
                    </div>
                    {['read_file','run_terminal_cmd','duckduckgo_search','web_fetch','grep'].map(tool=>(
                      <label key={tool} style={{display:'flex', alignItems:'center', gap:'10px', fontSize:'12px', marginBottom:'8px'}}>
                        <span className="mono" style={{width:'150px', color:'var(--muted)'}}>{tool}</span>
                        <input type="number" min={0} step={256}
                          placeholder="12288 (default)"
                          value={contextConfig.tool_output_budgets[tool] ?? ''}
                          onChange={e=>{
                            const v = e.target.value === '' ? undefined : Number(e.target.value);
                            const b = {...contextConfig.tool_output_budgets};
                            if(v === undefined || v <= 0) delete b[tool]; else b[tool] = v;
                            patchContext({ tool_output_budgets: b });
                          }} />
                      </label>
                    ))}
                  </div>

                  <div style={{padding:'16px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)'}}>
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'4px'}}>Turn &amp; segment budgets</div>
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'12px'}}>
                      A long task runs as N segments × up to max turns each, checkpointed by todos between segments.
                    </div>
                    <label style={{display:'flex', alignItems:'center', gap:'10px', cursor:'pointer', fontSize:'13px', marginBottom:'12px'}}>
                      <input type="checkbox" checked={budgetConfig.auto_continue}
                        onChange={e=>patchBudget({ auto_continue:e.target.checked })}
                        style={{width:'16px', height:'16px', accentColor:'var(--text)'}} />
                      <span>Auto-continue across segments</span>
                    </label>
                    <div style={{display:'flex', gap:'16px', flexWrap:'wrap'}}>
                      <label style={{display:'flex', flexDirection:'column', gap:'6px', fontSize:'12px'}}>
                        <span className="mono" style={{color:'var(--muted)'}}>Max turns / segment</span>
                        <input type="number" min={1} max={200} value={budgetConfig.max_turns}
                          onChange={e=>patchBudget({ max_turns: Math.max(1, Number(e.target.value) || 1) })}
                          style={{width:'110px'}} />
                      </label>
                      <label style={{display:'flex', flexDirection:'column', gap:'6px', fontSize:'12px'}}>
                        <span className="mono" style={{color:'var(--muted)'}}>Max segments / task</span>
                        <input type="number" min={1} max={10} value={budgetConfig.max_segments}
                          onChange={e=>patchBudget({ max_segments: Math.max(1, Number(e.target.value) || 1) })}
                          style={{width:'110px'}} />
                      </label>
                      <label style={{display:'flex', flexDirection:'column', gap:'6px', fontSize:'12px'}}>
                        <span className="mono" style={{color:'var(--muted)'}}>Spend cap (USD, blank = none)</span>
                        <input type="number" min={0} step={0.1} placeholder="none"
                          value={budgetConfig.spend_cap_usd ?? ''}
                          onChange={e=>patchBudget({ spend_cap_usd: e.target.value === '' ? null : Math.max(0, Number(e.target.value)) })}
                          style={{width:'130px'}} />
                      </label>
                    </div>
                    <div className="mono" style={{marginTop:'10px', fontSize:'11px', color:'var(--muted)'}}>
                      Crossing 80% of max segments × max turns × ~2000 tokens/turn emits a spend warning;
                      breaching the spend cap stops further segments.
                    </div>
                  </div>

                  <div style={{padding:'16px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)'}}>
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'4px'}}>Clarifying questions</div>
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'12px'}}>
                      When something is genuinely unclear, the agent asks you instead of guessing — a card
                      appears above the composer with options, free text, and a skip path. The per-run
                      question budget scales with max segments above.
                    </div>
                    <label style={{display:'flex', alignItems:'center', gap:'10px', cursor:'pointer', fontSize:'13px'}}>
                      <input type="checkbox" checked={interactionConfig.ask_user_enabled}
                        onChange={e=>patchInteraction({ ask_user_enabled:e.target.checked })}
                        style={{width:'16px', height:'16px', accentColor:'var(--text)'}} />
                      <span>Let the agent ask clarifying questions</span>
                    </label>
                    <div className="mono" style={{marginTop:'8px', fontSize:'11px', color:'var(--muted)'}}>
                      Off = the tool is removed and the model proceeds with its best judgment.
                    </div>
                  </div>
                </div>
              )}
              {activeTab==='memory' && (
                <div style={{display:'flex', flexDirection:'column', gap:'16px'}}>
                  <div className="mono" style={{fontSize:'11px', color:'var(--muted)'}}>
                    Unified memory persists facts, preferences, and project notes across chats.
                    Stored as plain Markdown (MEMORY.md) — global for you, per-workspace for projects —
                    and injected into every run. Files live as plaintext in the app data dir.
                    {memStats && (
                      <> Currently: <b>{memStats.global_bullets}</b> global · <b>{memStats.workspace_bullets}</b> workspace ({memStats.workspace_name}).</>
                    )}
                  </div>

                  <div style={{padding:'16px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)'}}>
                    <label style={{display:'flex', alignItems:'center', gap:'10px', cursor:'pointer', fontSize:'13px', marginBottom:'12px'}}>
                      <input type="checkbox" checked={memoryConfig.enabled}
                        onChange={e=>patchMemory({ enabled:e.target.checked })}
                        style={{width:'16px', height:'16px', accentColor:'var(--text)'}} />
                      <span>Enable unified memory</span>
                    </label>
                    <label style={{display:'flex', alignItems:'center', gap:'10px', cursor:'pointer', fontSize:'13px'}}>
                      <input type="checkbox" checked={memoryConfig.auto_extract}
                        disabled={!memoryConfig.enabled}
                        onChange={e=>patchMemory({ auto_extract:e.target.checked })}
                        style={{width:'16px', height:'16px', accentColor:'var(--text)'}} />
                      <span>Auto-extract facts after each chat (cheap model, background)</span>
                    </label>
                    <div className="mono" style={{marginTop:'8px', fontSize:'11px', color:'var(--muted)'}}>
                      The agent can also save/forget mid-run via the memory_save and memory_forget tools.
                    </div>
                  </div>

                  <div style={{padding:'16px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)'}}>
                    <div className="mono" style={{fontSize:'11px', color:'var(--muted)', marginBottom:'12px'}}>Scope &amp; budget</div>
                    <div style={{display:'flex', gap:'16px', flexWrap:'wrap', alignItems:'flex-end'}}>
                      <label style={{display:'flex', flexDirection:'column', gap:'6px', fontSize:'12px'}}>
                        <span className="mono" style={{color:'var(--muted)'}}>Read scope</span>
                        <select value={memoryConfig.scope}
                          disabled={!memoryConfig.enabled}
                          onChange={e=>patchMemory({ scope: e.target.value as MemoryScope })}
                          style={{width:'170px'}}>
                          <option value="both">Global + workspace</option>
                          <option value="global">Global only</option>
                          <option value="workspace">Workspace only</option>
                        </select>
                      </label>
                      <label style={{display:'flex', flexDirection:'column', gap:'6px', fontSize:'12px'}}>
                        <span className="mono" style={{color:'var(--muted)'}}>Extraction model</span>
                        <input value={memoryConfig.extract_model}
                          disabled={!memoryConfig.enabled}
                          onChange={e=>patchMemory({ extract_model: e.target.value })}
                          placeholder="gpt-4o-mini"
                          style={{width:'170px'}} />
                      </label>
                      <label style={{display:'flex', flexDirection:'column', gap:'6px', fontSize:'12px'}}>
                        <span className="mono" style={{color:'var(--muted)'}}>Max chars / run</span>
                        <input type="number" min={1024} max={50000} step={1000} value={memoryConfig.max_chars}
                          disabled={!memoryConfig.enabled}
                          onChange={e=>patchMemory({ max_chars: Math.max(1024, Number(e.target.value) || 8000) })}
                          style={{width:'120px'}} />
                      </label>
                    </div>
                    <div className="mono" style={{marginTop:'10px', fontSize:'11px', color:'var(--muted)'}}>
                      The extraction model runs against the active provider's credentials with its model swapped —
                      pick a cheap slug your provider serves (e.g. gpt-4o-mini, grok-4-fast, or a local Ollama model).
                    </div>
                  </div>

                  <div style={{padding:'16px', border:'1px solid var(--line)', borderRadius:'12px', background:'var(--bg)'}}>
                    <div style={{display:'flex', alignItems:'center', gap:'8px', marginBottom:'12px'}}>
                      <div className="mono" style={{fontSize:'11px', color:'var(--muted)'}}>Edit memory file</div>
                      <div style={{display:'flex', gap:'4px', marginLeft:'auto'}}>
                        {(['global','workspace'] as const).map(s=>(
                          <button key={s} onClick={()=>setMemScope(s)} style={{
                            padding:'4px 12px', borderRadius:999, border:'1px solid var(--line)',
                            background: memScope===s ? 'var(--text)' : 'transparent',
                            color: memScope===s ? 'var(--bg)' : 'var(--muted)',
                            fontSize:'11px', cursor:'pointer'
                          }}>{s}</button>
                        ))}
                      </div>
                    </div>
                    <textarea value={memText}
                      onChange={e=>{ setMemText(e.target.value); setMemDirty(true); }}
                      rows={12} spellCheck={false}
                      placeholder={memScope === 'global' ? 'No global memories yet — they appear here after chats, or write your own.' : 'No workspace memories yet for this project.'}
                      className="mono"
                      style={{width:'100%', fontSize:'12px', lineHeight:1.5, background:'var(--void)', color:'var(--text)', border:'1px solid var(--line)', borderRadius:'10px', padding:'10px 12px', resize:'vertical'}} />
                    <div style={{display:'flex', gap:'8px', marginTop:'10px', flexWrap:'wrap', alignItems:'center'}}>
                      <button className="btn-accent" onClick={saveMemoryText}
                        disabled={!memDirty || saving==='memory'} style={{borderRadius:999}}>
                        {saving==='memory' ? 'Saving…' : memDirty ? 'Save memory file' : 'Saved'}
                      </button>
                      <button className="btn-ghost" onClick={()=>loadMemoryText(memScope)}
                        disabled={!memDirty} style={{borderRadius:999}}>Discard edits</button>
                      <span style={{flex:1}} />
                      <button className="btn-ghost" onClick={()=>clearMemory('global')} style={{borderRadius:999}}>Clear global</button>
                      <button className="btn-ghost" onClick={()=>clearMemory('workspace')} style={{borderRadius:999}}>Clear workspace</button>
                      <button className="btn-ghost" onClick={()=>clearMemory('both')} style={{borderRadius:999, color:'var(--error)'}}>Clear all</button>
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
        <div
          onPointerDown={startPanelResize}
          title="Drag to resize"
          style={{
            position:'absolute', right:0, bottom:0, width:'28px', height:'28px',
            cursor:'nwse-resize', zIndex:5, display:'flex', alignItems:'flex-end', justifyContent:'flex-end',
            padding:'6px', touchAction:'none',
          }}
        >
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="var(--muted)" strokeWidth="1.5" strokeLinecap="round">
            <path d="M11 1 L1 11 M11 5 L5 11 M11 9 L9 11" />
          </svg>
        </div>
      </div>

      {/* Kilo model picker */}
      {showKiloPicker && (
        <div style={{position:'fixed', inset:0, background:'rgba(0,0,0,0.6)', backdropFilter:'blur(8px)', display:'flex', alignItems:'center', justifyContent:'center', zIndex:200, padding:'16px'}} onClick={()=> setShowKiloPicker(null)}>
          <div onClick={e=>e.stopPropagation()} style={{width:'100%', maxWidth:'560px', maxHeight:'78vh', minHeight:0, display:'flex', flexDirection:'column', background:'var(--panel)', border:'1px solid var(--line)', borderRadius:'16px', overflow:'hidden'}}>
            <div style={{padding:'14px 16px', borderBottom:'1px solid var(--line)', display:'flex', justifyContent:'space-between', alignItems:'center'}}>
               <div><div style={{fontWeight:600, fontSize:'13px'}}>Kilo models</div><div className="mono" style={{fontSize:'11px', color:'var(--muted)'}}>{kiloModels.length} available • configured Kilo Gateway catalog</div></div>
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
            <div style={{flex:1, minHeight:0, overflow:'auto', padding:'8px'}}>
              {kiloLoading ? <div className="mono" style={{textAlign:'center', padding:'24px', color:'var(--muted)'}}>Loading…</div>
               : kiloError ? <div className="mono" style={{textAlign:'center', padding:'24px', color:'var(--danger)'}}>{kiloError}</div>
               : filteredKilo.length===0 ? <div className="mono" style={{textAlign:'center', padding:'24px', color:'var(--muted)'}}>No matches</div>
               : filteredKilo.slice(0,120).map(m=>(
                 <button key={m.id} onClick={()=>selectKiloModel(m.id)} disabled={saving === (showKiloPicker || 'new')} style={{width:'100%', textAlign:'left', display:'flex', flexDirection:'column', gap:'2px', padding:'10px 12px', marginBottom:'6px', background:'var(--bg)', border:'1px solid var(--line)', borderRadius:'12px', opacity: saving === (showKiloPicker || 'new') ? 0.6 : 1}}>
                  <div style={{display:'flex', gap:'8px', alignItems:'center', flexWrap:'wrap'}}>
                    <span style={{fontWeight:600, fontSize:'12px', wordBreak:'break-all'}}>{m.id}</span>
                    {m.is_free ? <span className="mono" style={{fontSize:'10px', color:'var(--ok-text)', border:'1px solid var(--ok-border)', padding:'1px 6px', borderRadius:999}}>free</span> : <span className="mono" style={{fontSize:'10px', color:'var(--muted)', border:'1px solid var(--line)', padding:'1px 6px', borderRadius:999}}>paid</span>}
                    {m.context_length ? <span className="mono" style={{fontSize:'10px', color:'var(--muted)'}}>{Math.round(m.context_length/1000)}k</span> : null}
                    {m.supported_parameters?.some(p=>p==='reasoning_effort'||p==='reasoning') ? <span className="mono" style={{fontSize:'10px', color:'var(--accent)', border:'1px solid var(--line)', padding:'1px 6px', borderRadius:999}}>reasoning</span> : null}
                    {m.efforts?.length ? <span className="mono" style={{fontSize:'10px', color:'var(--muted)', border:'1px solid var(--line)', padding:'1px 6px', borderRadius:999}}>{m.efforts.join(' · ')}</span> : null}
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
