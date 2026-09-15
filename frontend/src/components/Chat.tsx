import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { Message, AgentEventType } from '../types';
import MarkdownRenderer from './MarkdownRenderer';

interface ChatProps {
  sessionId: string;
  onNewSession: () => void;
  onAddMcp: (name: string, command: string, args: string[]) => void;
  availableTools: string[];
}

function SendIcon() {
  return <svg width="16" height="16" viewBox="0 0 16 16" fill="none"><path d="M3 8 L13 8 M8 3 L13 8 L8 13" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round"/></svg>;
}
function SparkIcon() {
  return <svg width="18" height="18" viewBox="0 0 18 18" fill="none"><path d="M9 1.5 L10.8 6.2 L15.5 9 L10.8 11.8 L9 16.5 L7.2 11.8 L2.5 9 L7.2 6.2 Z" fill="currentColor" opacity="0.9"/><circle cx="14.5" cy="3.5" r="1.3" fill="currentColor"/><circle cx="4" cy="13" r="1" fill="currentColor" opacity="0.6"/></svg>;
}
function ToolIcon() {
  return <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.2"><rect x="1.5" y="2.5" width="9" height="7" rx="1.2"/><path d="M4 4.5 H8 M4 6.5 H7 M4 8.5 H6"/></svg>;
}

export default function Chat({ sessionId, onNewSession, onAddMcp, availableTools }: ChatProps) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState('');
  const [isLoading, setIsLoading] = useState(false);
  const [showMcpDialog, setShowMcpDialog] = useState(false);
  const [mcpName, setMcpName] = useState('');
  const [mcpCommand, setMcpCommand] = useState('');
  const [mcpArgs, setMcpArgs] = useState('');
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const scrollerRef = useRef<HTMLDivElement>(null);

  const loadSessionMessages = async (targetSessionId: string) => {
    try {
      const raw = await invoke<any[]>('get_session_messages', { sessionId: targetSessionId });
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
    loadSessionMessages(sessionId);

    let unlisten: UnlistenFn | null = null;
    listen('agent-event', (event: any) => {
      const payload = event.payload as { session_id: string; event: AgentEventType };
      if (payload.session_id !== sessionId) return;
      handleAgentEvent(payload.event);
    }).then(u => {
      unlisten = u;
    }).catch(e => {
      console.error('Failed to listen to agent-event', e);
    });

    return () => {
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
    switch (event.type) {
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
        break;
      case 'ToolCallStarted':
        setMessages(prev => [...prev, { id: `tool-${Date.now()}-${Math.random()}`, role: 'tool', content: event.name, toolCalls: [{ id: `call-${Date.now()}`, name: event.name, arguments: event.args }], timestamp: new Date() }]);
        break;
      case 'ToolCallCompleted':
        setMessages(prev => {
          const upd = [...prev];
          let idx = -1;
          for (let i = upd.length - 1; i >= 0; i--) {
            if (upd[i].role === 'tool') { idx = i; break; }
          }
          if (idx >= 0) {
            upd[idx] = { ...upd[idx], content: event.name, toolResult: { toolCallId: `call-${Date.now()}`, content: event.output } };
          }
          return upd;
        });
        break;
      case 'TurnCompleted':
        setIsLoading(false);
        loadSessionMessages(sessionId);
        break;
      case 'Error':
        setIsLoading(false);
        setMessages(prev => [...prev, { id: `error-${Date.now()}`, role: 'assistant', content: `Error: ${event.message}`, timestamp: new Date() }]);
        break;
      default:
        break;
    }
    requestAnimationFrame(() => scrollerRef.current?.scrollTo({ top: scrollerRef.current.scrollHeight, behavior: 'smooth' }));
  };

  const handleSend = async () => {
    if (!input.trim() || isLoading) return;
    const text = input;
    const userMessage: Message = { id: `msg-${Date.now()}`, role: 'user', content: text, timestamp: new Date() };
    setMessages(prev => [...prev, userMessage]);
    setInput('');
    setIsLoading(true);
    if (textareaRef.current) textareaRef.current.style.height = '44px';
    try {
      await invoke('send_message', { sessionId, text });
      await loadSessionMessages(sessionId);
    } catch (error) {
      setMessages(prev => [...prev, { id: `error-${Date.now()}`, role: 'assistant', content: `Send failed: ${error}`, timestamp: new Date() }]);
    } finally {
      setIsLoading(false);
      requestAnimationFrame(() => scrollerRef.current?.scrollTo({ top: scrollerRef.current.scrollHeight, behavior: 'smooth' }));
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  };

  const handleAddMcp = async () => {
    if (!mcpName.trim() || !mcpCommand.trim()) return;
    const args = mcpArgs.split(' ').filter(a => a.trim());
    await onAddMcp(mcpName, mcpCommand, args);
    setShowMcpDialog(false);
    setMcpName('');
    setMcpCommand('');
    setMcpArgs('');
  };

  const suggestions = [
    { k: 'Build', v: 'Run tests and fix failures', icon: '⚡' },
    { k: 'Explore', v: 'Read src-tauri/ and explain the agent loop', icon: '🔍' },
    { k: 'Search', v: 'Find all TODOs in the codebase', icon: '📂' },
    { k: 'Automate', v: 'Create a branch, commit, and push', icon: '🚀' },
  ];

  return (
    <div style={{display:'flex', flexDirection:'column', flex:1, minHeight:0, background:'var(--bg)', position:'relative'}}>
      {/* Messages */}
      <div ref={scrollerRef} style={{flex:1, overflowY:'auto', overflowX:'hidden', display:'flex', flexDirection:'column'}}>
        <div style={{width:'100%', maxWidth:'760px', margin:'0 auto', flex:1, display:'flex', flexDirection:'column', padding: messages.length===0 ? '0 20px' : '28px 20px 0', gap:'0', minHeight:'100%'}}>
          {messages.length===0 ? (
            <div style={{flex:1, display:'flex', flexDirection:'column', alignItems:'center', justifyContent:'center', gap:'28px', padding:'40px 0 80px', textAlign:'center'}}>
              <div style={{width:'56px', height:'56px', borderRadius:'16px', background:'var(--panel)', border:'1px solid var(--line)', display:'flex', alignItems:'center', justifyContent:'center', boxShadow:'0 4px 20px rgba(0,0,0,0.3)'}}>
                <span style={{fontWeight:700, fontSize:'18px', letterSpacing:'-0.04em', color:'var(--rosso)'}}>M</span>
              </div>
              <div>
                <h2 style={{fontSize:'26px', fontWeight:650, letterSpacing:'-0.03em', lineHeight:1.1}}>Where should we start?</h2>
                <div className="mono" style={{fontSize:'12px', color:'var(--muted)', marginTop:'8px', letterSpacing:'0.02em'}}>ChatGPT familiar • Agent intelligence • Grok build ready</div>
              </div>
              <div style={{width:'100%', maxWidth:'640px', display:'grid', gridTemplateColumns:'1fr 1fr', gap:'10px', textAlign:'left', marginTop:'4px'}}>
                {suggestions.map(c=>(
                  <button key={c.k} onClick={()=>setInput(c.v)} style={{textAlign:'left', padding:'14px 16px', background:'var(--panel)', border:'1px solid var(--line)', borderRadius:'16px', display:'flex', flexDirection:'column', gap:'6px', transition:'all .15s'}}>
                    <div style={{display:'flex', alignItems:'center', gap:'8px'}}>
                      <span style={{fontSize:'14px'}}>{c.icon}</span>
                      <span className="mono" style={{fontSize:'11px', color:'var(--muted)', letterSpacing:'0.06em', fontWeight:600}}>{c.k}</span>
                    </div>
                    <div style={{fontSize:'13px', lineHeight:1.45, color:'var(--text)'}}>{c.v}</div>
                  </button>
                ))}
              </div>
              <div className="mono" style={{fontSize:'11px', color:'var(--faint)', maxWidth:'520px', lineHeight:1.6, marginTop:'4px'}}>
                Tools ready • run_terminal_cmd • read_file • write_to_file • grep • list_dir • duckduckgo_search • skill • Grok build
              </div>
            </div>
          ) : (
            <div style={{display:'flex', flexDirection:'column', gap:'0', paddingBottom:'24px'}}>
              {messages.map(msg => <MessageBubble key={msg.id} message={msg} />)}
              {isLoading && (
                <div style={{display:'flex', gap:'12px', padding:'18px 0', alignItems:'center'}}>
                  <div style={{width:'28px', height:'28px', flexShrink:0, display:'flex', alignItems:'center', justifyContent:'center', background:'var(--text)', borderRadius:'50%', color:'var(--bg)', fontSize:'10px', fontWeight:700}}>MV</div>
                  <div style={{display:'flex', gap:'4px', alignItems:'center', padding:'10px 14px', background:'var(--panel)', border:'1px solid var(--line)', borderRadius:'16px'}}>
                    <span style={{width:'6px', height:'6px', background:'var(--muted)', borderRadius:'50%', display:'inline-block', animation:'pulse 1.2s infinite'}}/>
                    <span style={{width:'6px', height:'6px', background:'var(--muted)', borderRadius:'50%', display:'inline-block', animation:'pulse 1.2s infinite .2s'}}/>
                    <span style={{width:'6px', height:'6px', background:'var(--muted)', borderRadius:'50%', display:'inline-block', animation:'pulse 1.2s infinite .4s'}}/>
                  </div>
                </div>
              )}
              <div ref={messagesEndRef} />
            </div>
          )}
        </div>
      </div>

      {/* Floating Chat Bar — ChatGPT style */}
      <div style={{padding:'0 16px 18px', background:'linear-gradient(transparent, var(--bg) 28%)', flexShrink:0, display:'flex', justifyContent:'center'}}>
        <div style={{width:'100%', maxWidth:'760px', display:'flex', flexDirection:'column', gap:'10px'}}>
          {/* Floating pill */}
          <div style={{
            display:'flex', flexDirection:'column',
            background:'var(--panel)',
            border:'1px solid var(--line)',
            borderRadius:'28px',
            boxShadow:'0 8px 32px rgba(0,0,0,0.45), 0 0 0 1px rgba(255,255,255,0.04)',
            padding:'10px 10px 10px 18px',
            gap:'8px'
          }}>
            <div style={{display:'flex', gap:'10px', alignItems:'flex-end'}}>
              <textarea
                ref={textareaRef}
                value={input} onChange={e=>setInput(e.target.value)} onKeyDown={handleKeyDown}
                placeholder="Ask Maverick anything…"
                rows={1}
                style={{
                  flex:1, minHeight:'44px', maxHeight:'160px', resize:'none',
                  background:'transparent', border:'none', padding:'10px 2px',
                  fontSize:'15px', lineHeight:1.5, outline:'none', color:'var(--text)'
                }}
                disabled={isLoading}
              />
              <button
                onClick={handleSend} disabled={!input.trim() || isLoading}
                style={{
                  width:'40px', height:'40px', padding:0, display:'flex', alignItems:'center', justifyContent:'center',
                  background: input.trim() && !isLoading ? 'var(--text)' : '#2A2A2A',
                  color: input.trim() && !isLoading ? 'var(--bg)' : 'var(--faint)',
                  border:'none',
                  borderRadius:'50%', flexShrink:0, transition:'all .15s'
                }}
                aria-label="Send"
              >
                {isLoading ? <span className="mono" style={{fontSize:'11px'}}>…</span> : <SendIcon />}
              </button>
            </div>
            {/* bar footer */}
            <div style={{display:'flex', gap:'8px', alignItems:'center', flexWrap:'wrap', paddingRight:'2px'}}>
              <div style={{display:'flex', gap:'6px', alignItems:'center'}}>
                <span className="mono" style={{fontSize:'10px', color:'var(--faint)', border:'1px solid var(--line)', padding:'4px 8px', borderRadius:999, display:'inline-flex', alignItems:'center', gap:'6px'}}>
                  <ToolIcon /> {availableTools.length} tools
                </span>
                <span className="mono" style={{fontSize:'10px', color:'var(--muted)', display:'none'}}>{availableTools.slice(0,2).join(' • ')}</span>
              </div>
              <div style={{flex:1}} />
              <button className="btn-ghost" onClick={()=>setShowMcpDialog(true)} style={{fontSize:'12px', padding:'6px 12px', borderRadius:999, border:'1px solid var(--line)'}}>＋ MCP</button>
              <button className="btn-ghost" onClick={onNewSession} style={{fontSize:'12px', padding:'6px 12px', borderRadius:999}}>New chat</button>
            </div>
          </div>
          <div className="mono" style={{textAlign:'center', fontSize:'10px', color:'var(--faint)', opacity:0.8, letterSpacing:'0.02em'}}>
            Maverick can make mistakes. Verify critical commands. • Grok build • Ferrari precision
          </div>
        </div>
      </div>

      {showMcpDialog && (
        <div style={{position:'fixed', inset:0, background:'rgba(0,0,0,0.55)', backdropFilter:'blur(10px)', display:'flex', alignItems:'center', justifyContent:'center', zIndex:100, padding:'16px'}}>
          <div className="panel" style={{width:'100%', maxWidth:'520px', padding:'22px', borderRadius:'20px', boxShadow:'0 16px 48px rgba(0,0,0,0.5)'}}>
            <div style={{display:'flex', alignItems:'center', gap:'10px', marginBottom:'16px'}}>
              <span style={{width:'28px', height:'28px', borderRadius:'8px', background:'var(--panel)', border:'1px solid var(--line)', display:'flex', alignItems:'center', justifyContent:'center'}}><SparkIcon /></span>
              <div><div style={{fontWeight:600, fontSize:'14px'}}>Add MCP server</div><div className="mono" style={{fontSize:'11px', color:'var(--muted)'}}>Extend Maverick with external tools</div></div>
            </div>
            <div style={{display:'flex', flexDirection:'column', gap:'10px'}}>
              <input placeholder="Server name • filesystem" value={mcpName} onChange={e=>setMcpName(e.target.value)} />
              <input placeholder="Command • npx" value={mcpCommand} onChange={e=>setMcpCommand(e.target.value)} />
              <input placeholder="Args • -y @modelcontextprotocol/server-filesystem /tmp" value={mcpArgs} onChange={e=>setMcpArgs(e.target.value)} />
              <div style={{display:'flex', justifyContent:'flex-end', gap:'8px', marginTop:'10px'}}>
                <button className="btn-ghost" onClick={()=>setShowMcpDialog(false)} style={{borderRadius:999}}>Cancel</button>
                <button className="btn-rosso" onClick={handleAddMcp} disabled={!mcpName.trim()||!mcpCommand.trim()} style={{borderRadius:999}}>Add server</button>
              </div>
            </div>
          </div>
        </div>
      )}
      <style>{`@keyframes pulse{0%,100%{opacity:.3}50%{opacity:1}}`}</style>
    </div>
  );
}

function MessageBubble({ message }: { message: Message }) {
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
      <div style={{display:'flex', gap:'10px', padding:'14px 16px', background:'var(--panel)', border:'1px solid var(--line)', borderRadius:'16px', margin:'10px 0'}}>
        <div style={{marginTop:'2px', width:'22px', height:'22px', borderRadius:'50%', background: hasResult ? 'rgba(34,197,94,0.15)' : 'var(--rosso-dim)', border:`1px solid ${hasResult ? 'rgba(34,197,94,0.3)' : 'rgba(227,6,19,0.2)'}`, display:'flex', alignItems:'center', justifyContent:'center', flexShrink:0}}>
          <span style={{width:'7px', height:'7px', background: hasResult ? '#22c55e' : 'var(--rosso)', borderRadius:'50%', display:'inline-block'}}/>
        </div>
        <div style={{minWidth:0, flex:1}}>
          <div style={{display:'flex', alignItems:'center', gap:'8px', flexWrap:'wrap'}}>
            <span className="mono" style={{fontSize:'12px', fontWeight:600, color:'var(--text)'}}>{message.content}</span>
            <span className="mono" style={{fontSize:'10px', color: hasResult ? '#22c55e' : 'var(--muted)', border:`1px solid ${hasResult ? 'rgba(34,197,94,0.3)' : 'var(--line)'}`, padding:'2px 7px', borderRadius:999}}>{hasResult ? 'completed' : 'running…'}</span>
          </div>
          {message.toolCalls && <div className="mono" style={{fontSize:'11px', color:'var(--faint)', marginTop:'6px', wordBreak:'break-all', background:'var(--bg)', border:'1px solid var(--line)', padding:'8px 10px', borderRadius:'10px'}}>{message.toolCalls[0]?.arguments.slice(0,600)}</div>}
          {message.toolResult && <div className="mono" style={{marginTop:'10px', fontSize:'12px', background:'var(--bg)', border:'1px solid var(--line)', padding:'10px 12px', borderRadius:'12px', whiteSpace:'pre-wrap', maxHeight:'220px', overflowY:'auto', lineHeight:1.5}}>{message.toolResult.content.slice(0,2000)}</div>}
        </div>
      </div>
    );
  }
  if (isUser) {
    return (
      <div style={{display:'flex', justifyContent:'flex-end', padding:'10px 0'}}>
        <div style={{maxWidth:'78%', background:'#2A2A2A', border:'1px solid var(--line-2)', color:'var(--text)', padding:'12px 16px', borderRadius:'20px', borderBottomRightRadius:'6px', boxShadow:'0 1px 8px rgba(0,0,0,0.2)'}}>
          <div style={{whiteSpace:'pre-wrap', wordBreak:'break-word', fontSize:'14px', lineHeight:1.6}}>{message.content}</div>
        </div>
      </div>
    );
  }
  return (
    <div style={{display:'flex', gap:'12px', padding:'16px 0', alignItems:'flex-start'}}>
      <div style={{
        width:'28px', height:'28px', flexShrink:0, display:'flex', alignItems:'center', justifyContent:'center',
        background:'var(--text)', borderRadius:'50%', color:'var(--bg)', fontSize:'10px', fontWeight:750, letterSpacing:'-0.02em'
      }}>MV</div>
      <div style={{minWidth:0, flex:1, paddingTop:'1px'}}>
        <div style={{
          background: isError ? 'rgba(227,6,19,0.08)' : 'transparent',
          border: isError ? '1px solid rgba(227,6,19,0.2)' : 'none',
          padding: isError ? '12px 14px' : '0',
          borderRadius: isError ? '12px' : '0'
        }}>
          {isError ? (
            <div style={{ whiteSpace: 'pre-wrap', color: '#ff6b6b' }}>{message.content}</div>
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
                color: copied ? '#22c55e' : 'var(--muted)',
                display: 'inline-flex',
                alignItems: 'center',
                gap: '4px',
                cursor: 'pointer',
              }}
              aria-label="Copy message"
            >
              {copied ? (
                <>
                  <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.6"><path d="M2.5 6.5 L4.5 8.5 L9.5 3.5"/></svg>
                  <span>Copied</span>
                </>
              ) : (
                <>
                  <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.3"><rect x="4" y="4" width="6" height="6" rx="1"/><path d="M3 8 H2.5 A1 1 0 0 1 1.5 7 V2.5 A1 1 0 0 1 2.5 1.5 H7 A1 1 0 0 1 8 2.5 V3"/></svg>
                  <span>Copy</span>
                </>
              )}
            </button>
          )}
          {isError && <span style={{color:'#ff6b6b'}}>• needs attention</span>}
        </div>
      </div>
    </div>
  );
}
