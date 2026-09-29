import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { Message, AgentEventType, UiConfig } from '../types';
import MarkdownRenderer from './MarkdownRenderer';
import RunStatusBar, { RunStatusData } from './RunStatusBar';
import ApertureLogo, { ApertureTile } from './ApertureLogo';
import { ChevronIcon, ToolsIcon, StopIcon, CopyIcon, CheckIcon, AttachIcon, XIcon } from './icons';
import ComposerPlusMenu, { Attachment, fmtBytes } from './ComposerPlusMenu';

/// Taskbar progress, best-effort (no-op outside Tauri).
async function setTaskbarProgress(status: 'indeterminate' | 'none' | 'error') {
  try {
    const { getCurrentWindow, ProgressBarStatus } = await import('@tauri-apps/api/window');
    const map = {
      indeterminate: ProgressBarStatus.Indeterminate,
      none: ProgressBarStatus.None,
      error: ProgressBarStatus.Error,
    } as const;
    await getCurrentWindow().setProgressBar({ status: map[status] });
  } catch {
    /* browser dev or minimal webview — ignore */
  }
}

/// OS notification, only when the window is unfocused. Best-effort.
async function notifyUser(title: string, body: string) {
  try {
    if (typeof document !== 'undefined' && document.hasFocus()) return;
    const mod = await import('@tauri-apps/plugin-notification');
    if (!(await mod.isPermissionGranted())) {
      if ((await mod.requestPermission()) !== 'granted') return;
    }
    mod.sendNotification({ title, body });
  } catch {
    /* plugin unavailable — stay silent */
  }
}

interface ChatProps {
  sessionId: string;
  onAddMcp: (name: string, command: string, args: string[]) => Promise<void>;
  availableTools: string[];
  ui: UiConfig;
}

export default function Chat({ sessionId, onAddMcp, availableTools, ui }: ChatProps) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState('');
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [showMcpDialog, setShowMcpDialog] = useState(false);
  const [mcpName, setMcpName] = useState('');
  const [mcpCommand, setMcpCommand] = useState('');
  const [mcpArgs, setMcpArgs] = useState('');
  const [mcpError, setMcpError] = useState<string | null>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const scrollerRef = useRef<HTMLDivElement>(null);

  // ── Live run status (status bar + banners + attention) ──────────────
  const [runStatus, setRunStatus] = useState<RunStatusData | null>(null);
  const [runBanner, setRunBanner] = useState<{ kind: 'warn' | 'cap'; text: string } | null>(null);
  const [runSummary, setRunSummary] = useState<string | null>(null);
  // Ref mirror: the `agent-event` listener closure is bound on mount, so
  // handlers must read run state from refs, never from stale state.
  const runStatusRef = useRef<RunStatusData | null>(null);
  const toolStartRef = useRef<{ name: string; at: number }[]>([]);
  const runIdRef = useRef(0);

  // Session that owns the in-flight run. Run UI (working spinner, banner,
  // status bar, Stop) renders only when the visible session owns the run, so
  // switching sessions never carries another session's run across — and
  // switching back to the running session restores it. The ref mirror is what
  // async continuations read to decide whether they may still touch UI.
  const [runSessionId, setRunSessionId] = useState<string | null>(null);
  const runSessionIdRef = useRef<string | null>(null);
  // Latest visible session, readable from async continuations created under
  // an older render.
  const sessionIdRef = useRef(sessionId);
  sessionIdRef.current = sessionId;
  const runHere = runSessionId === sessionId;
  const busy = isLoading && runHere;

  const setStatus = (
    next: RunStatusData | null | ((p: RunStatusData | null) => RunStatusData | null),
  ) => {
    const v =
      typeof next === 'function'
        ? (next as (p: RunStatusData | null) => RunStatusData | null)(runStatusRef.current)
        : next;
    runStatusRef.current = v;
    setRunStatus(v);
  };

  const touchStatus = (patch: Partial<RunStatusData>) => {
    setStatus(prev => (prev ? { ...prev, ...patch, lastActivityAt: Date.now() } : prev));
  };

  const finishRun = (kind: 'done' | 'error' | 'cancelled' | 'cap', detail: string) => {
    const s = runStatusRef.current;
    const elapsed = s ? Date.now() - s.startedAt : 0;
    const mm = String(Math.floor(elapsed / 60000)).padStart(2, '0');
    const ss = String(Math.floor((elapsed % 60000) / 1000)).padStart(2, '0');
    const tokens =
      s && s.totalTokens >= 1000 ? `${(s.totalTokens / 1000).toFixed(1)}k` : `${s?.totalTokens ?? 0}`;
    const cost = `$${(s?.costUsd ?? 0).toFixed(3)}`;
    const segs = s?.segment ?? 1;
    const summary =
      kind === 'done'
        ? `Done in ${mm}:${ss} · ${segs} segment${segs === 1 ? '' : 's'} · ${tokens} tokens · ${cost}`
        : kind === 'cap'
          ? `Stopped at cap in ${mm}:${ss} · ${tokens} tokens · ${cost}`
          : kind === 'cancelled'
            ? `Cancelled after ${mm}:${ss}`
            : `Failed after ${mm}:${ss}`;
    setStatus(prev => (prev ? { ...prev, finished: true, lastActivityAt: Date.now() } : prev));
    setRunSummary(summary);
    // Only the visible session that owns the run may clear the spinner —
    // a run finishing for another session must not blank its status bar.
    if (runSessionIdRef.current === sessionId) setIsLoading(false);
    if (kind === 'error') {
      setTaskbarProgress('error');
      notifyUser('Maverick — run failed', detail.slice(0, 200));
    } else if (kind === 'cap') {
      setTaskbarProgress('error');
      notifyUser('Maverick — spend cap reached', detail.slice(0, 200));
    } else if (kind === 'done') {
      setTaskbarProgress('none');
      notifyUser('Maverick — run finished', summary);
    } else {
      setTaskbarProgress('none');
    }
  };

  // Monotonic token: a load that resolves after a newer one started (or after
  // the visible session changed) must not overwrite the transcript.
  const loadSeqRef = useRef(0);

  const loadSessionMessages = async (targetSessionId: string) => {
    const seq = ++loadSeqRef.current;
    try {
      const raw = await invoke<any[]>('get_session_messages', { sessionId: targetSessionId });
      if (seq !== loadSeqRef.current) return;
      const mapped: Message[] = (raw || []).map(m => ({
        ...m,
        timestamp: m.timestamp ? new Date(m.timestamp) : new Date(),
      }));
      setMessages(mapped);
      requestAnimationFrame(() => scrollerRef.current?.scrollTo({ top: scrollerRef.current.scrollHeight, behavior: 'auto' }));
    } catch (e) {
      console.warn('Failed to load session messages', e);
    }
  };

  useEffect(() => {
    // Drop the previous session's transcript immediately — the fetch below is
    // async, and showing session B's messages in session A is a data leak.
    setMessages([]);
    loadSessionMessages(sessionId);

    let disposed = false;
    let unlisten: UnlistenFn | null = null;
    listen('agent-event', (event: any) => {
      const payload = event.payload as { session_id: string; event: AgentEventType };
      if (payload.session_id !== sessionId) return;
      handleAgentEvent(payload.event);
    }).then(u => {
      // The effect may have cleaned up before the listener resolved — that
      // would otherwise leak a listener bound to a stale session forever.
      if (disposed) u();
      else unlisten = u;
    }).catch(e => {
      console.error('Failed to listen to agent-event', e);
    });

    return () => {
      disposed = true;
      if (unlisten) unlisten();
    };
  }, [sessionId]);

  // auto-resize textarea
  useEffect(() => {
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
      textareaRef.current.style.height = Math.min(textareaRef.current.scrollHeight, 160) + 'px';
    }
  }, [input]);

  const handleAgentEvent = (event: AgentEventType) => {
    // The listener already filters by session, but a second session can own
    // the run UI: events from a session that is not the current run's session
    // must not mutate another run's status bar / transcript.
    if (runSessionIdRef.current !== sessionId) return;
    switch (event.type) {
      case 'TurnStarted':
        touchStatus({ turn: event.turn });
        break;
      case 'ThinkingStarted':
        setMessages(prev => {
          const last = prev[prev.length - 1];
          if (last && last.role === 'assistant' && last.thinking) {
            const upd = [...prev];
            upd[upd.length - 1] = { ...upd[upd.length - 1], thinking: { ...last.thinking, open: true } };
            return upd;
          }
          return [...prev, {
            id: `think-${Date.now()}-${Math.random()}`,
            role: 'assistant',
            content: '',
            timestamp: new Date(),
            thinking: { open: true, steps: [] }
          }];
        });
        break;
      case 'ThinkingStep':
        setMessages(prev => {
          const upd = [...prev];
          let idx = -1;
          for (let i = upd.length - 1; i >= 0; i--) {
            if (upd[i].role === 'assistant' && upd[i].thinking) { idx = i; break; }
          }
          if (idx >= 0) {
            upd[idx] = {
              ...upd[idx],
              thinking: { ...upd[idx].thinking!, steps: [...(upd[idx].thinking?.steps || []), event.text] }
            };
          }
          return upd;
        });
        break;
      case 'AssistantText':
        setMessages(prev => {
          const last = prev[prev.length - 1];
          if (last && last.role === 'assistant' && !last.toolCalls) {
            const upd = [...prev];
            upd[upd.length - 1] = { ...upd[upd.length - 1], content: upd[upd.length - 1].content + event.text };
            return upd;
          }
          return [...prev, { id: `msg-${Date.now()}-${Math.random()}`, role: 'assistant', content: event.text, timestamp: new Date() }];
        });
        touchStatus({});
        break;
      case 'ToolCallStarted':
        toolStartRef.current.push({ name: event.name, at: Date.now() });
        touchStatus({ lastTool: event.name });
        setMessages(prev => [...prev, { id: `tool-${Date.now()}-${Math.random()}`, role: 'tool', content: event.name, toolCalls: [{ id: `call-${Date.now()}`, name: event.name, arguments: event.args }], timestamp: new Date() }]);
        break;
      case 'ToolCallCompleted': {
        const at = Date.now();
        let durationMs: number | undefined;
        const starts = toolStartRef.current;
        for (let i = starts.length - 1; i >= 0; i--) {
          if (starts[i].name === event.name) {
            durationMs = at - starts[i].at;
            starts.splice(i, 1);
            break;
          }
        }
        setMessages(prev => {
          const upd = [...prev];
          let idx = -1;
          for (let i = upd.length - 1; i >= 0; i--) {
            if (upd[i].role === 'tool') { idx = i; break; }
          }
          if (idx >= 0) {
            upd[idx] = { ...upd[idx], content: event.name, durationMs, toolResult: { toolCallId: `call-${Date.now()}`, content: event.output } };
          }
          return upd;
        });
        touchStatus({ lastTool: event.name });
        break;
      }
      // NOTE: the backend emits TurnCompleted per segment, not per run, so
      // this only refreshes the transcript — the run finalizes when
      // `send_message` resolves (or on Cancelled / SpendCapReached / Error).
      case 'TurnCompleted':
        loadSessionMessages(sessionId);
        touchStatus({});
        break;
      case 'BudgetWarning':
        setRunBanner({ kind: 'warn', text: `Budget nearly exhausted — ${event.remaining} tool turn${event.remaining === 1 ? '' : 's'} left before wrap-up.` });
        touchStatus({});
        break;
      case 'SegmentBoundary': {
        const cap = runStatusRef.current?.capUsd ?? null;
        const label =
          cap != null && event.segment > event.max_segments
            ? `Continuing work (segment ${event.segment}+ · unbounded, cap set)…`
            : `Continuing work (segment ${event.segment}/${event.max_segments})…`;
        setMessages(prev => [...prev, { id: `seg-${Date.now()}`, role: 'assistant', content: label, timestamp: new Date() }]);
        setStatus(prev =>
          prev ? { ...prev, segment: event.segment, maxSegments: event.max_segments, lastActivityAt: Date.now() } : prev,
        );
        break;
      }
      case 'UsageUpdated':
        setStatus(prev =>
          prev ? { ...prev, totalTokens: event.total_tokens, costUsd: event.cost_usd, lastActivityAt: Date.now() } : prev,
        );
        break;
      case 'SpendWarning': {
        const text = `Spend notice: ${event.percent_used}% of the token budget used (${event.total_tokens}/${event.budget_tokens} tokens).`;
        setMessages(prev => [...prev, { id: `spend-${Date.now()}`, role: 'assistant', content: text, timestamp: new Date() }]);
        touchStatus({});
        notifyUser('Maverick — spend notice', text);
        break;
      }
      case 'Cancelled':
        setMessages(prev => [...prev, { id: `cancel-${Date.now()}`, role: 'assistant', content: 'Run cancelled.', timestamp: new Date() }]);
        finishRun('cancelled', '');
        break;
      case 'SpendCapReached': {
        const text = `Spend cap reached: $${event.spent_usd.toFixed(4)} of $${event.cap_usd.toFixed(4)} — stopped gracefully.`;
        setRunBanner({ kind: 'cap', text });
        setMessages(prev => [...prev, { id: `cap-${Date.now()}`, role: 'assistant', content: text, timestamp: new Date() }]);
        finishRun('cap', text);
        break;
      }
      case 'Error':
        setMessages(prev => [...prev, { id: `error-${Date.now()}`, role: 'assistant', content: `Error: ${event.message}`, timestamp: new Date() }]);
        finishRun('error', event.message);
        break;
      default:
        break;
    }
    if (ui.auto_scroll) {
      requestAnimationFrame(() => scrollerRef.current?.scrollTo({ top: scrollerRef.current.scrollHeight, behavior: 'smooth' }));
    }
  };

  const handleSend = async () => {
    if ((!input.trim() && !attachments.length) || busy) return;
    // Attachments serialize as fenced blocks; prompt context first, ask last.
    const blocks = attachments
      .map(a => `[Attached file: ${a.name}]\n\`\`\`\n${a.content}\n\`\`\``)
      .join('\n\n');
    const text = blocks ? `${blocks}\n\n${input.trim()}` : input;
    const sentSession = sessionId;
    const userMessage: Message = { id: `msg-${Date.now()}`, role: 'user', content: text, timestamp: new Date() };
    setMessages(prev => [...prev, userMessage]);
    setInput('');
    setAttachments([]);
    setIsLoading(true);
    setRunSessionId(sentSession);
    runSessionIdRef.current = sentSession;
    // Fresh run state for the status bar; the spend cap (if any) arrives
    // async below and switches the segment label to unbounded mode.
    const runId = ++runIdRef.current;
    setRunBanner(null);
    setRunSummary(null);
    toolStartRef.current = [];
    const startedAt = Date.now();
    const init: RunStatusData = {
      segment: 1, maxSegments: 1, turn: 0, startedAt, lastActivityAt: startedAt,
      totalTokens: 0, costUsd: 0, lastTool: null, capUsd: null, finished: false,
    };
    runStatusRef.current = init;
    setRunStatus(init);
    setTaskbarProgress('indeterminate');
    invoke<any>('get_budget_config')
      .then(cfg => {
        if (runIdRef.current !== runId) return;
        const cap = typeof cfg?.spend_cap_usd === 'number' ? (cfg.spend_cap_usd as number) : null;
        const maxSeg = typeof cfg?.max_segments === 'number' ? (cfg.max_segments as number) : 1;
        setStatus(prev => (prev ? { ...prev, capUsd: cap, maxSegments: maxSeg } : prev));
      })
      .catch(() => {});
    if (textareaRef.current) textareaRef.current.style.height = '44px';
    // A session switch hands the run UI to whoever is visible now, and a newer
    // send overwrites run ownership entirely — a stale continuation must stop
    // writing into the wrong transcript in both cases.
    const stillMyRun = () => runSessionIdRef.current === sentSession;
    const visibleHere = () => sessionIdRef.current === sentSession;
    try {
      await invoke('send_message', { sessionId: sentSession, text });
      if (!stillMyRun()) {
        notifyUser('Maverick — run finished', 'A background run finished.');
      } else {
        // Transcript load only matters while this session is on screen; the
        // run summary/status updates are gated by `runHere` at render time.
        if (visibleHere()) await loadSessionMessages(sentSession);
        // Natural finish (or graceful cap stop): finalize unless a terminal
        // event (cap / cancel / error) already did.
        if (!runStatusRef.current?.finished) finishRun('done', '');
      }
    } catch (error) {
      // A terminal event (Error / Cancelled) usually already explained the
      // failure — only add a bubble when the run died before emitting one.
      if (!stillMyRun()) {
        notifyUser('Maverick — run failed', `${error}`.slice(0, 200));
      } else if (!runStatusRef.current?.finished) {
        if (visibleHere()) {
          setMessages(prev => [...prev, { id: `error-${Date.now()}`, role: 'assistant', content: `Send failed: ${error}`, timestamp: new Date() }]);
        }
        finishRun('error', `${error}`);
      }
    } finally {
      // Never clear a newer run's spinner: only the run that still owns the
      // UI may flip `isLoading` back off.
      if (stillMyRun()) setIsLoading(false);
      if (ui.auto_scroll) {
        requestAnimationFrame(() => scrollerRef.current?.scrollTo({ top: scrollerRef.current.scrollHeight, behavior: 'smooth' }));
      }
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  // Skills picker drops a natural-language skill reference at the caret.
  const insertSkill = (name: string) => {
    const snippet = `use skill "${name}"`;
    const ta = textareaRef.current;
    const start = ta?.selectionStart ?? input.length;
    const end = ta?.selectionEnd ?? input.length;
    const next = input.slice(0, start) + snippet + input.slice(end);
    setInput(next);
    requestAnimationFrame(() => {
      ta?.focus();
      ta?.setSelectionRange(start + snippet.length, start + snippet.length);
    });
  };

  const handleCancel = async () => {
    // Cancel the run's own session, not whatever session happens to be visible.
    const target = runSessionIdRef.current ?? sessionId;
    try {
      await invoke('cancel_message', { sessionId: target });
    } catch (e) {
      console.warn('cancel_message failed', e);
    }
  };

  const handleAddMcp = async () => {
    if (!mcpName.trim() || !mcpCommand.trim()) return;
    const args = mcpArgs.split(' ').filter(a => a.trim());
    setMcpError(null);
    try {
      await onAddMcp(mcpName, mcpCommand, args);
      setShowMcpDialog(false);
      setMcpName('');
      setMcpCommand('');
      setMcpArgs('');
    } catch (e) {
      // Keep the dialog open so a failed handshake is visible instead of
      // looking like a successful add.
      setMcpError(String(e));
    }
  };

  const suggestions = [
    { k: 'Build', v: 'Run tests and fix failures' },
    { k: 'Explore', v: 'Read src-tauri/ and explain the agent loop' },
    { k: 'Search', v: 'Find all TODOs in the codebase' },
    { k: 'Automate', v: 'Create a branch, commit, and push' },
  ];

  return (
    <div style={{display:'flex', flexDirection:'column', flex:1, minHeight:0, background:'var(--bg)', position:'relative'}}>
      {/* Messages */}
      <div ref={scrollerRef} style={{flex:1, overflowY:'auto', overflowX:'hidden', display:'flex', flexDirection:'column'}}>
        <div style={{width:'100%', maxWidth:'768px', margin:'0 auto', flex:1, display:'flex', flexDirection:'column', padding: messages.length===0 ? '0 24px' : '32px 24px 0', gap:'0', minHeight:'100%'}}>
          {messages.length===0 ? (
            <div style={{flex:1, display:'flex', flexDirection:'column', alignItems:'center', justifyContent:'center', gap:'28px', padding:'40px 0 80px', textAlign:'center'}}>
              <div style={{
                width:'64px', height:'64px', borderRadius:'18px', background:'var(--void)',
                border:'1px solid var(--line)', display:'flex', alignItems:'center', justifyContent:'center',
                boxShadow:'0 4px 24px rgba(0,0,0,0.4)'
              }}>
                <ApertureLogo size={40} animated />
              </div>
              <div>
                <h2 style={{fontSize:'26px', fontWeight:500, letterSpacing:'-0.03em', lineHeight:1.1}}>Where should we start?</h2>
                <div className="mono" style={{fontSize:'12px', color:'var(--muted)', marginTop:'8px', letterSpacing:'0.02em'}}>Agent intelligence • local tools • background tasks</div>
              </div>
              <div style={{width:'100%', maxWidth:'640px', display:'grid', gridTemplateColumns:'1fr 1fr', gap:'10px', textAlign:'left', marginTop:'4px'}}>
                {suggestions.map(c=>(
                  <button key={c.k} onClick={()=>setInput(c.v)} style={{textAlign:'left', padding:'14px 16px', background:'var(--panel-3)', border:'1px solid var(--line)', borderRadius:'14px', display:'flex', flexDirection:'column', gap:'6px', transition:'all .2s cubic-bezier(0.16,1,0.3,1)'}}>
                    <div style={{display:'flex', alignItems:'center', gap:'8px'}}>
                      <span className="mono" style={{fontSize:'11px', color:'var(--accent-2)', letterSpacing:'0.06em', fontWeight:600}}>{c.k}</span>
                    </div>
                    <div style={{fontSize:'13px', lineHeight:1.45, color:'var(--text)'}}>{c.v}</div>
                  </button>
                ))}
              </div>
              <div className="mono" style={{fontSize:'11px', color:'var(--faint)', maxWidth:'520px', lineHeight:1.6, marginTop:'4px'}}>
                Terminal • files • search • fetch • background jobs — all executed locally
              </div>
            </div>
          ) : (
            <div style={{display:'flex', flexDirection:'column', gap:'0', paddingBottom:'24px'}}>
              {(ui.show_tool_calls ? messages : messages.filter(m => m.role !== 'tool')).map(msg => (
                <MessageBubble key={msg.id} message={msg} compact={ui.compact_mode} />
              ))}
              {busy && (
                <div style={{display:'flex', gap:'14px', padding:'16px 0', alignItems:'center'}}>
                  <ApertureTile size={28} animated />
                  <div className="mono" style={{display:'flex', gap:'8px', alignItems:'center', fontSize:'12px', color:'var(--muted)'}}>
                    <span style={{width:7, height:7, borderRadius:'50%', background:'var(--accent)', display:'inline-block'}} className="ping-soft-wrap">
                      <span style={{display:'inline-block', width:7, height:7, borderRadius:'50%', background:'var(--accent)'}} className="ping-soft" />
                    </span>
                    working…
                  </div>
                </div>
              )}
              <div ref={messagesEndRef} />
            </div>
          )}
        </div>
      </div>

      {/* Floating prompt dock */}
      <div style={{padding:'0 16px 18px', background:'linear-gradient(transparent, var(--bg) 28%)', flexShrink:0, display:'flex', justifyContent:'center'}}>
        <div style={{width:'100%', maxWidth:'768px', display:'flex', flexDirection:'column', gap:'10px'}}>
          {runHere && (runBanner || runStatus) && (
            <div style={{display:'flex', flexDirection:'column', gap:'8px'}}>
              {runBanner && (
                <div
                  className="mono"
                  style={{
                    fontSize:'11px', padding:'8px 12px', borderRadius:'14px', border:'1px solid',
                    ...(runBanner.kind === 'cap'
                      ? { color:'var(--error)', background:'var(--error-bg)', borderColor:'var(--error-border)' }
                      : { color:'var(--warn)', background:'var(--warn-bg)', borderColor:'var(--warn-border)' }),
                  }}
                >
                  {runBanner.text}
                </div>
              )}
              <RunStatusBar status={runStatus} summary={runSummary} />
            </div>
          )}
          {/* Dock */}
          <div style={{
            display:'flex', flexDirection:'column',
            background:'rgba(0,0,0,0.95)',
            backdropFilter:'blur(10px)',
            border:'1px solid var(--line-2)',
            borderRadius:'28px',
            boxShadow:'0 8px 32px rgba(0,0,0,0.45)',
            padding:'14px 16px 10px',
            gap:'8px',
            transition:'border-color .2s cubic-bezier(0.16,1,0.3,1)'
          }}>
            {attachments.length > 0 && (
              <div style={{display:'flex', gap:'6px', flexWrap:'wrap'}}>
                {attachments.map(a => (
                  <span key={a.id} className="mono" style={{display:'inline-flex', alignItems:'center', gap:'6px', maxWidth:'240px', fontSize:'11px', padding:'4px 8px', background:'var(--chip)', border:'1px solid var(--line)', borderRadius:999, color:'var(--text)'}}>
                    <AttachIcon size={11} />
                    <span style={{overflow:'hidden', textOverflow:'ellipsis', whiteSpace:'nowrap'}}>{a.name}</span>
                    <span style={{color:'var(--faint)'}}>{fmtBytes(a.size)}</span>
                    <button onClick={()=>setAttachments(prev=>prev.filter(x=>x.id!==a.id))} aria-label={`Remove ${a.name}`} style={{display:'flex', padding:0, border:'none', background:'transparent', color:'var(--muted)', cursor:'pointer', borderRadius:'50%'}}>
                      <XIcon size={9} />
                    </button>
                  </span>
                ))}
              </div>
            )}
            <textarea
              ref={textareaRef}
              value={input} onChange={e=>setInput(e.target.value)} onKeyDown={handleKeyDown}
              placeholder="Ask Maverick anything…"
              rows={1}
              style={{
                width:'100%', minHeight:'28px', maxHeight:'160px', resize:'none',
                background:'transparent', border:'none', padding:'2px 2px',
                fontSize:'14.5px', lineHeight:1.6, outline:'none', color:'var(--text)'
              }}
              disabled={busy}
            />
            <div style={{display:'flex', alignItems:'center', justifyContent:'space-between', borderTop:'1px solid var(--line)', paddingTop:'8px'}}>
              <div style={{display:'flex', gap:'8px', alignItems:'center', flexWrap:'wrap'}}>
                <ComposerPlusMenu
                  onAttach={a => setAttachments(prev => {
                    if (prev.length >= 6) { alert('Attachment limit is 6 files per message'); return prev; }
                    return [...prev, a];
                  })}
                  onInsertSkill={insertSkill}
                  onAddMcp={() => setShowMcpDialog(true)}
                />
                <span className="mono" style={{fontSize:'11.5px', color:'var(--muted)', background:'var(--chip)', border:'1px solid var(--line)', padding:'5px 11px', borderRadius:999, display:'inline-flex', alignItems:'center', gap:'6px'}}>
                  <ToolsIcon size={13} /> {availableTools.length} tools
                </span>
              </div>
              <div style={{display:'flex', gap:'8px', alignItems:'center'}}>
                {busy && (
                  <button className="btn-ghost" onClick={handleCancel} style={{fontSize:'12px', padding:'6px 12px', borderRadius:999, color:'var(--error)', display:'inline-flex', alignItems:'center', gap:'6px'}}>
                    <StopIcon size={10} /> Stop
                  </button>
                )}
                <button
                  onClick={handleSend} disabled={(!input.trim() && !attachments.length) || busy}
                  aria-label="Send"
                  style={{
                    width:'36px', height:'36px', padding:0, display:'flex', alignItems:'center', justifyContent:'center',
                    background: (input.trim() || attachments.length) && !busy ? 'var(--accent)' : 'var(--control-off)',
                    color: (input.trim() || attachments.length) && !busy ? 'var(--bg)' : 'var(--faint)',
                    border:'none',
                    borderRadius:'50%', flexShrink:0, transition:'all .15s'
                  }}
                >
                  {busy ? <span className="mono" style={{fontSize:'11px'}}>…</span> : <span style={{transform:'rotate(180deg)', display:'flex'}}><SendArrow /></span>}
                </button>
              </div>
            </div>
          </div>
          <div className="mono" style={{textAlign:'center', fontSize:'10px', color:'var(--faint)', opacity:0.8, letterSpacing:'0.02em'}}>
            Maverick can make mistakes. Verify important info.
          </div>
        </div>
      </div>

      {showMcpDialog && (
        <div style={{position:'fixed', inset:0, background:'rgba(0,0,0,0.55)', backdropFilter:'blur(10px)', display:'flex', alignItems:'center', justifyContent:'center', zIndex:100, padding:'16px'}}>
          <div className="panel" style={{width:'100%', maxWidth:'520px', padding:'22px', borderRadius:'20px', boxShadow:'0 16px 48px rgba(0,0,0,0.5)'}}>
            <div style={{display:'flex', alignItems:'center', gap:'10px', marginBottom:'16px'}}>
              <span style={{width:'28px', height:'28px', borderRadius:'8px', background:'var(--void)', border:'1px solid var(--line)', display:'flex', alignItems:'center', justifyContent:'center'}}><ApertureLogo size={16} /></span>
              <div><div style={{fontWeight:600, fontSize:'14px'}}>Add MCP server</div><div className="mono" style={{fontSize:'11px', color:'var(--muted)'}}>Extend Maverick with external tools</div></div>
            </div>
            <div style={{display:'flex', flexDirection:'column', gap:'10px'}}>
              <input placeholder="Server name • filesystem" value={mcpName} onChange={e=>setMcpName(e.target.value)} />
              <input placeholder="Command • npx" value={mcpCommand} onChange={e=>setMcpCommand(e.target.value)} />
              <input placeholder="Args • -y @modelcontextprotocol/server-filesystem /tmp" value={mcpArgs} onChange={e=>{setMcpArgs(e.target.value); setMcpError(null);}} />
              {mcpError && (
                <div className="mono" style={{fontSize:'11px', color:'var(--error)', lineHeight:1.5, wordBreak:'break-word'}}>{mcpError}</div>
              )}
              <div style={{display:'flex', justifyContent:'flex-end', gap:'8px', marginTop:'10px'}}>
                <button className="btn-ghost" onClick={()=>{setShowMcpDialog(false); setMcpError(null);}} style={{borderRadius:999}}>Cancel</button>
                <button className="btn-accent" onClick={handleAddMcp} disabled={!mcpName.trim()||!mcpCommand.trim()} style={{borderRadius:999}}>Add server</button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

function SendArrow() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M8 3v10M3.5 8.5 8 13l4.5-4.5" />
    </svg>
  );
}

function MessageBubble({ message, compact = false }: { message: Message; compact?: boolean }) {
  const [copied, setCopied] = useState(false);
  const isUser = message.role === 'user';
  const isTool = message.role === 'tool';
  const isError = message.content.startsWith('Error:') || message.content.startsWith('Send failed:');

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(message.content);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {}
  };

  if (isTool) {
    const hasResult = !!message.toolResult;
    return (
      <details
        open={!hasResult}
        className="tool-fold"
        style={{
          margin: compact ? '4px 0' : '10px 0',
          borderRadius:'14px',
          border:'1px solid var(--line)',
          background:'rgba(0,0,0,0.80)',
          overflow:'hidden',
        }}
      >
        <summary style={{
          display:'flex', alignItems:'center', gap:'9px', padding:'9px 12px', cursor:'pointer', listStyle:'none',
          fontSize:'12px', color:'var(--muted)'
        }}>
          <span style={{position:'relative', width:8, height:8, flexShrink:0, display:'inline-flex', alignItems:'center', justifyContent:'center'}}>
            {hasResult ? (
              <span style={{width:8, height:8, borderRadius:'50%', background:'var(--accent-2)', display:'inline-block'}} />
            ) : (
              <>
                <span className="ping-soft" style={{position:'absolute', width:8, height:8, borderRadius:'50%', background:'var(--accent)', opacity:0.75}} />
                <span style={{width:8, height:8, borderRadius:'50%', background:'var(--accent)', display:'inline-block'}} />
              </>
            )}
          </span>
          <span className="mono" style={{fontWeight:600, color: hasResult ? 'var(--text)' : 'var(--muted)'}}>{message.content}</span>
          <span className="mono" style={{fontSize:'10px', color: hasResult ? 'var(--accent-2)' : 'var(--muted)', border:`1px solid ${hasResult ? 'var(--accent-border)' : 'var(--line)'}`, padding:'1px 7px', borderRadius:999}}>
            {hasResult ? 'completed' : 'running…'}
          </span>
          {hasResult && message.durationMs != null && (
            <span className="mono" style={{fontSize:'10px', color:'var(--faint)'}}>· {(message.durationMs / 1000).toFixed(1)}s</span>
          )}
          <span style={{flex:1}} />
          <ChevronIcon size={14} className="fold-chev" />
        </summary>
        <div style={{padding:'0 12px 12px', borderTop:'1px solid var(--line)', paddingTop: message.toolCalls ? 10 : 0}}>
          {message.toolCalls && <div className="mono" style={{fontSize:'11px', color:'var(--faint)', wordBreak:'break-all', background:'var(--panel-2)', border:'1px solid var(--line)', padding:'8px 10px', borderRadius:'10px'}}>{message.toolCalls[0]?.arguments.slice(0,600)}</div>}
          {message.toolResult && <div className="mono" style={{marginTop:'8px', fontSize:'12px', background:'var(--panel-2)', border:'1px solid var(--line)', padding:'10px 12px', borderRadius:'12px', whiteSpace:'pre-wrap', maxHeight:'220px', overflowY:'auto', lineHeight:1.5}}>{message.toolResult.content.slice(0,2000)}</div>}
        </div>
        <style>{`.tool-fold summary::-webkit-details-marker{display:none} .tool-fold .fold-chev{transition:transform .2s} .tool-fold[open] .fold-chev{transform:rotate(180deg)}`}</style>
      </details>
    );
  }
  if (isUser) {
    return (
      <div style={{display:'flex', justifyContent:'flex-end', padding: compact ? '4px 0' : '10px 0'}}>
        <div style={{maxWidth:'80%', background:'var(--bubble-user)', border:'1px solid var(--line-2)', color:'var(--text)', padding:'10px 16px', borderRadius:'18px', boxShadow:'0 1px 8px rgba(0,0,0,0.2)'}}>
          <div style={{whiteSpace:'pre-wrap', wordBreak:'break-word', fontSize:'14.5px', lineHeight:1.6}}>{message.content}</div>
        </div>
      </div>
    );
  }
  return (
    <div style={{display:'flex', gap:'14px', padding: compact ? '6px 0' : '14px 0', alignItems:'flex-start'}}>
      <ApertureTile size={28} />
      <div style={{minWidth:0, flex:1, paddingTop:'1px'}}>
        {message.thinking && message.thinking.steps.length > 0 && (
          <details
            open={message.thinking.open}
            className="thinking-fold"
            style={{
              margin:'0 0 10px',
              borderRadius:'14px',
              border:'1px solid var(--line)',
              background:'rgba(0,0,0,0.85)',
              overflow:'hidden',
            }}
          >
            <summary style={{
              display:'flex', alignItems:'center', gap:'9px', padding:'9px 12px', cursor:'pointer', listStyle:'none',
              fontSize:'12px', color:'var(--muted)'
            }}>
              <span style={{position:'relative', width:8, height:8, flexShrink:0, display:'inline-flex', alignItems:'center', justifyContent:'center'}}>
                <span className="ping-soft" style={{position:'absolute', width:8, height:8, borderRadius:'50%', background:'var(--accent)', opacity:0.75}} />
                <span style={{width:8, height:8, borderRadius:'50%', background:'var(--accent)', display:'inline-block'}} />
              </span>
              <span className="mono" style={{fontWeight:600, color:'var(--text)'}}>Thought for {message.thinking.steps.length} step{message.thinking.steps.length === 1 ? '' : 's'}</span>
              <span className="mono" style={{fontSize:'10px', color:'var(--faint)', marginLeft:'auto'}}>{message.thinking.open ? 'thinking…' : 'done'}</span>
              <ChevronIcon size={14} className="fold-chev" />
            </summary>
            <div style={{padding:'10px 12px 12px', borderTop:'1px solid var(--line)', display:'flex', flexDirection:'column', gap:'8px', position:'relative'}}>
              <div style={{position:'absolute', left:'21px', top:'14px', bottom:'14px', width:'1px', background:'var(--line)'}} />
              {message.thinking.steps.map((s, i) => (
                <div key={i} style={{display:'flex', gap:'8px', position:'relative', zIndex:1, alignItems:'flex-start'}}>
                  <span style={{width:6, height:6, borderRadius:'50%', background:'var(--accent-2)', flexShrink:0, marginTop:'5px'}} />
                  <span className="mono" style={{fontSize:'11.5px', color:'var(--muted)', lineHeight:1.55, wordBreak:'break-word'}}>{s}</span>
                </div>
              ))}
            </div>
            <style>{`.thinking-fold summary::-webkit-details-marker{display:none} .thinking-fold .fold-chev{transition:transform .2s} .thinking-fold[open] .fold-chev{transform:rotate(180deg)}`}</style>
          </details>
        )}
        <div style={{
          background: isError ? 'var(--error-bg)' : 'transparent',
          border: isError ? '1px solid var(--error-border)' : 'none',
          padding: isError ? '12px 14px' : '0',
          borderRadius: isError ? '14px' : '0'
        }}>
          {isError ? (
            <div style={{ whiteSpace: 'pre-wrap', color: 'var(--error)' }}>{message.content}</div>
          ) : (
            <MarkdownRenderer content={message.content} />
          )}
        </div>
        <div className="mono" style={{fontSize:'11px', color:'var(--faint)', marginTop:'8px', display:'flex', alignItems:'center', gap:'12px'}}>
          <span>{message.timestamp.toLocaleTimeString([], {hour:'2-digit', minute:'2-digit'})}</span>
          {!isError && (
            <button
              onClick={handleCopy}
              className="btn-ghost"
              style={{
                padding: '2px 7px',
                fontSize: '11px',
                borderRadius: '4px',
                color: copied ? 'var(--accent-2)' : 'var(--muted)',
                display: 'inline-flex',
                alignItems: 'center',
                gap: '4px',
                cursor: 'pointer',
              }}
              aria-label="Copy message"
            >
              {copied ? <CheckIcon size={11} /> : <CopyIcon size={11} />}
              <span>{copied ? 'Copied' : 'Copy'}</span>
            </button>
          )}
          {isError && <span style={{color:'var(--error)'}}>• needs attention</span>}
        </div>
      </div>
    </div>
  );
}
