import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen, UnlistenFn } from '@tauri-apps/api/event';
import { Message, AgentEventType, Artifact, PendingBatch, UiConfig, ProviderInfo, ReasoningProfile, ModelPreset } from '../types';
import { artifactLanguageForPath } from '../types';
import { type RunStatusData } from './RunStatusBar';
import ApertureLogo, { ApertureTile } from './ApertureLogo';
import ShinyText from './ShinyText';
import { ChevronIcon, CopyIcon, CheckIcon } from './icons';
import MarkdownRenderer from './MarkdownRenderer';
import ComposerPlusMenu, { Attachment, fmtBytes } from './ComposerPlusMenu';
import PromptBar, { PromptBarModel } from './PromptBar';
import ArtifactDrawer from './ArtifactDrawer';
import QuestionCard from './QuestionCard';
import ThoughtLine from './ThoughtLine';
import CallChip, { CallChipIcon } from './CallChip';
import { OpenUiBlock, splitOpenUiSegments, MarkdownProseBlock, type ActionEvent as OpenUiActionEvent } from '../openui/library';
import { EmailDraftBlock, isEmailDraftCode } from '../openui/email-drafts';

/** Assistant body: EVERYTHING renders through the OpenUI pipeline — prose as
 *  the `Markdown` component (`root = Markdown()` + text via context), fenced
 *  segments as live generative UI. The plain MarkdownRenderer path survives
 *  only as the escape hatch for generative_ui === false. */
function AssistantContent({ content, streaming, genUiEnabled, onOpenArtifact, onOpenUiAction }: {
  content: string;
  streaming?: boolean;
  genUiEnabled: boolean;
  onOpenArtifact?: (a: { language: string; code: string; title: string }) => void;
  onOpenUiAction: (event: OpenUiActionEvent) => void;
}) {
  const segments = useMemo(
    () => (genUiEnabled ? splitOpenUiSegments(content) : null),
    [content, genUiEnabled],
  );
  // Escape hatch: with generative UI off, this is byte-for-byte the old path.
  if (!genUiEnabled) {
    return <MarkdownRenderer content={content} onOpenArtifact={onOpenArtifact} streaming={streaming} />;
  }
  if (!segments || segments.every(s => s.kind === 'text')) {
    return <MarkdownProseBlock text={content} isStreaming={streaming} onOpenArtifact={onOpenArtifact} />;
  }
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: '10px' }}>
      {segments.map((seg, i) => {
        if (seg.kind === 'text') {
          return seg.body.trim() ? (
            <MarkdownProseBlock key={i} text={seg.body} isStreaming={streaming} onOpenArtifact={onOpenArtifact} />
          ) : null;
        }
        if (!seg.body.trim()) return null;
        // A fence still missing its closer while streaming is a UI under
        // construction — show a quiet placeholder, not flashing raw code.
        if (!seg.complete && streaming) {
          return <div key={i} style={{ fontSize: '13px', color: 'var(--muted)' }}>Composing interface…</div>;
        }
        // Email drafts render with the react-email library; everything else
        // with the Maverick component library.
        return isEmailDraftCode(seg.body) ? (
          <EmailDraftBlock key={i} code={seg.body} isStreaming={streaming ?? false} onAction={onOpenUiAction} />
        ) : (
          <OpenUiBlock key={i} code={seg.body} isStreaming={streaming ?? false} onAction={onOpenUiAction} />
        );
      })}
    </div>
  );
}

/// Fallback effort tiers: shown while a model's profile loads, and for kinds
/// where the backend reports only its kind-wide default. Sent as strings; the
/// backend maps them onto its ReasoningEffort enum (`Extra` → `xhigh`).
const DEFAULT_EFFORTS: string[] = ['Low', 'Medium', 'High', 'Extra', 'Max'];

/// Wire tier → slider label (`Extra` is the UI name for `xhigh`; the backend
/// aliases it back on send).
const EFFORT_LABELS: Record<string, string> = {
  none: 'None',
  minimal: 'Minimal',
  low: 'Low',
  medium: 'Medium',
  high: 'High',
  xhigh: 'Extra',
  max: 'Max',
};
const effortLabel = (wire: string): string => EFFORT_LABELS[wire] ?? wire;

/// Home-screen headline pool — one playful line per session, shuffled on click.
const FUN_HEADLINES: string[] = [
  "Let's noodle.",
  "What's cooking?",
  'Fresh chat, fresh chaos.',
  'Got a wild idea?',
  'Spill it — what are we building?',
  'Whatcha thinking about?',
  'Okay, what are we breaking today?',
  'Type something. Or everything.',
  'Ideas? I have coffee.',
  "Let's make a productive mess.",
];

const pickHeadline = (exclude?: string): string => {
  if (FUN_HEADLINES.length <= 1) return FUN_HEADLINES[0] ?? '';
  let next = FUN_HEADLINES[Math.floor(Math.random() * FUN_HEADLINES.length)] ?? '';
  while (exclude !== undefined && next === exclude) {
    next = FUN_HEADLINES[Math.floor(Math.random() * FUN_HEADLINES.length)] ?? '';
  }
  return next;
};

/// Tool names that create or modify workspace files (artifact viewer sources).
/// `search_replace` is the agent's Edit tool — the one it actually reaches for,
/// and with an empty `old_string` it creates a file. Missing it meant a whole
/// "write three files" run produced no preview at all.
const WRITE_TOOL_NAMES = [
  'write_to_file', 'write_file', 'create_file',
  'search_replace', 'hashline_edit', 'edit_file', 'apply_patch',
];

/// The backend prefixes a failed tool result with this marker. Anything else
/// from a write-family tool means the file changed on disk.
const TOOL_ERROR_PREFIX = '[TOOL_ERROR] ';

/** Did this write-family tool call succeed? */
function writeSucceeded(output: string | undefined): boolean {
  return !!output && !output.startsWith(TOOL_ERROR_PREFIX);
}

/** Path a write-family tool call targeted: live args JSON first, else the
 *  "Successfully wrote N bytes to P" result line (reloaded transcripts have
 *  no args). Null when no path can be determined. */
function extractWritePath(msg: Message): string | null {
  const raw = msg.toolCalls?.[0]?.arguments;
  if (raw) {
    try {
      const a = JSON.parse(raw);
      const p = a.path ?? a.file_path ?? a.target_file ?? a.filename;
      if (typeof p === 'string' && p.trim()) return p.trim();
    } catch { /* not JSON — fall through to result parse */ }
  }
  const m = msg.toolResult?.content?.match(/Successfully wrote \d+ bytes to (.+?)\s*$/);
  return m ? m[1].trim() : null;
}

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
  providers: ProviderInfo[];
  selectedProvider: string;
  onProviderChange: (id: string) => void;
  /** Saved presets, rendered at the top of the composer's model menu. */
  presets?: ModelPreset[];
  /** Apply a preset — App owns the global default (settings + provider). */
  onApplyPreset?: (preset: ModelPreset) => void;
  /** Open App's "save current as preset" name dialog. */
  onSavePresetRequest?: () => void;
  /** Mirror the picked effort up so App can snapshot it when saving. */
  onEffortChange?: (effort: string) => void;
  /** App pushes a preset's effort down after apply; `token` busts re-applies
   *  of the same preset (same tier). */
  presetEffort?: { tier: string; token: number } | null;
  /** Live run state mirrored up for Settings → Status. */
  onRunStatus?: (status: RunStatusData | null, summary: string | null) => void;
}

export default function Chat({ sessionId, onAddMcp, ui, providers, selectedProvider, onProviderChange, presets = [], onApplyPreset, onSavePresetRequest, onEffortChange, presetEffort, onRunStatus }: ChatProps) {
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState('');
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  // Reasoning effort picked in the composer. Empty until the user touches the
  // slider: sends without a picked effort keep the provider default, so a
  // model that rejects `reasoning_effort` never gets one by accident.
  const [effort, setEffort] = useState('');
  // Effort tiers the *selected model* accepts (backend: Kilo catalog for
  // gateways, kind default otherwise). Unsupported models get an empty list,
  // which hides the slider entirely.
  const [efforts, setEfforts] = useState<string[]>(DEFAULT_EFFORTS);
  const [profile, setProfile] = useState<ReasoningProfile | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  // The `agent-event` listener closure binds once per session switch, so
  // async continuations must read live values through these ref mirrors —
  // closure-captured state stays frozen at its mount-time value.
  const isLoadingRef = useRef(false);
  isLoadingRef.current = isLoading;
  const uiRef = useRef(ui);
  uiRef.current = ui;
  const [showMcpDialog, setShowMcpDialog] = useState(false);
  const [mcpName, setMcpName] = useState('');
  const [mcpCommand, setMcpCommand] = useState('');
  const [mcpArgs, setMcpArgs] = useState('');
  const [mcpError, setMcpError] = useState<string | null>(null);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const scrollerRef = useRef<HTMLDivElement>(null);

  // Sync the effort slider to the selected model's capabilities. The backend
  // answers from the Kilo catalog (cached) for gateway providers and from the
  // provider kind otherwise; a picked tier that the new model lacks is dropped
  // so a stale effort can never be sent.
  useEffect(() => {
    let alive = true;
    const provider = providers.find(p => p.id === selectedProvider);
    setProfile(null);
    invoke<ReasoningProfile>('get_model_reasoning', {
      provider_id: selectedProvider,
      model: provider?.model ?? null,
    })
      .then(resolved => {
        if (!alive) return;
        setProfile(resolved);
        const labels = resolved.supported ? resolved.efforts.map(effortLabel) : [];
        setEfforts(labels);
        setEffort(prev => (prev && labels.includes(prev) ? prev : ''));
      })
      .catch(() => {
        if (!alive) return;
        setProfile(null);
        setEfforts(DEFAULT_EFFORTS);
      });
    return () => {
      alive = false;
    };
  }, [selectedProvider, providers]);

  // Mirror the picked effort up to App (save-preset snapshots it). A ref keeps
  // the effect clear of prop-identity churn across renders.
  const onEffortChangeRef = useRef(onEffortChange);
  onEffortChangeRef.current = onEffortChange;
  useEffect(() => {
    onEffortChangeRef.current?.(effort);
  }, [effort]);

  // App applies a preset by pushing its effort through a nonce so re-applying
  // the same preset still fires. The profile sync above stays the validator:
  // it drops tiers the target model doesn't accept once the profile resolves.
  useEffect(() => {
    if (!presetEffort) return;
    setEffort(presetEffort.tier || '');
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [presetEffort?.token]);

  // Exact-match active preset: provider + model + effort must all line up,
  // so the composer's checked row tracks what the next send will actually use.
  const activePreset =
    presets.find(p => {
      if (p.provider_id !== selectedProvider) return false;
      const provider = providers.find(x => x.id === selectedProvider);
      if (!provider || p.model !== provider.model) return false;
      return (p.effort || '') === (effort || '');
    }) ?? null;

  // Composer model list, memoized: rebuilding it per render (10x/sec while
  // streaming) busts PromptBar's internal memo and re-renders the menu.
  const composerModels = useMemo<PromptBarModel[]>(() => [
    ...presets.map<PromptBarModel>(p => ({
      key: `preset:${p.id}`,
      name: p.name,
      tag: p.model,
      description: `${providers.find(x => x.id === p.provider_id)?.name ?? p.provider_id} · ${p.effort || 'default effort'}`,
    })),
    ...providers.map<PromptBarModel>(p => ({
      key: p.id,
      name: p.name,
      tag: p.model,
      description:
        p.id === selectedProvider
          ? profile
            ? profile.supported
              ? `Reasoning: ${efforts.join(' · ')}`
              : 'No reasoning effort'
            : undefined
          : p.supports_reasoning_effort
            ? 'Reasoning models supported'
            : undefined,
    })),
  ], [presets, providers, selectedProvider, profile, efforts]);

  // Visible transcript rows, memoized: the filter allocated a new array per
  // render (~10x/sec while streaming) even when nothing changed.
  const visibleMessages = useMemo(
    () => (ui.show_tool_calls ? messages : messages.filter(m => m.role !== 'tool')),
    [ui.show_tool_calls, messages],
  );

  // ── Live run status (status bar + banners + attention) ─────────────────
  const [runStatus, setRunStatus] = useState<RunStatusData | null>(null);
  const [runBanner, setRunBanner] = useState<{ kind: 'warn' | 'cap'; text: string } | null>(null);
  const [runSummary, setRunSummary] = useState<string | null>(null);
  // Ref mirror: the `agent-event` listener closure is bound on mount, so
  // handlers must read run state from refs, never from stale state.
  const runStatusRef = useRef<RunStatusData | null>(null);
  // Last wall-clock push of token totals to React state (UsageUpdated events
  // arrive per token batch; mirroring each one re-renders the whole app).
  const lastStatusPushRef = useRef(0);
  const toolStartRef = useRef<{ name: string; at: number; args?: string }[]>([]);
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
  // Home-screen headline: a fresh playful line whenever the session changes;
  // clicking the headline shuffles to another one.
  const [greeting, setGreeting] = useState(() => pickHeadline());
  useEffect(() => {
    setGreeting(pickHeadline());
  }, [sessionId]);
  // Per-session memory kill-switch (composer + menu). Loop-local on the
  // backend: re-sync whenever the visible session changes so a fresh loop
  // (new/reopened session) picks up this session's choice.
  const [memoryOff, setMemoryOff] = useState<Record<string, boolean>>({});
  const memoryEnabled = !(memoryOff[sessionId] ?? false);
  useEffect(() => {
    invoke('set_session_memory_enabled', { session_id: sessionId, enabled: !(memoryOff[sessionId] ?? false) }).catch(() => {});
    // Only the session switch re-syncs; the toggle pushes directly.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId]);
  const toggleMemory = () => {
    const next = !memoryEnabled;
    setMemoryOff(prev => ({ ...prev, [sessionId]: !next }));
    invoke('set_session_memory_enabled', { session_id: sessionId, enabled: next }).catch(e => alert(String(e)));
  };
  // Artifact viewer drawer (Claude-style): chat code blocks + workspace
  // files the agent wrote. Cleared on session switch.
  const [artifacts, setArtifacts] = useState<Artifact[]>([]);
  const [activeArtifactId, setActiveArtifactId] = useState<string | null>(null);
  const [artifactOpen, setArtifactOpen] = useState(false);
  const artifactsRef = useRef<Artifact[]>([]);
  artifactsRef.current = artifacts;
  // Live drawer width, reported by ArtifactDrawer: the chat column narrows to
  // clear it instead of continuing underneath.
  const [drawerWidth, setDrawerWidth] = useState(560);
  const artifactOpenRef = useRef(false);
  artifactOpenRef.current = artifactOpen;
  /** Set at run start when a run may auto-open the viewer exactly once. */
  const autoOpenPendingRef = useRef(false);
  // Stable callbacks so memoized bubbles below don't re-render on every
  // parent render: both use functional setState, no live values needed.
  const openArtifact = useCallback((a: { title: string; language: string; code: string; note?: string; path?: string; url?: string }) => {
    const id = `art-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    setArtifacts(prev => [...prev.slice(-19), { ...a, id }]);
    setActiveArtifactId(id);
    setArtifactOpen(true);
  }, []);
  const openFileArtifact = useCallback(async (path: string) => {
    try {
      const r = await invoke<{ path: string; content: string; size: number; truncated: boolean }>('preview_workspace_file', { path, session_id: sessionId });
      const lang = artifactLanguageForPath(r.path) ?? artifactLanguageForPath(path);
      if (!lang) {
        alert(`Not previewable (only .html, .svg, .md): ${path}`);
        return;
      }
      // Resolve a loopback URL for this exact file so relative CSS/JS, ES
      // modules and fetch() work. Best-effort: on failure the drawer falls
      // back to inline `srcDoc`, which still renders single-file output.
      let url: string | undefined;
      try {
        url = await invoke<string>('preview_server_start', { path, session_id: sessionId });
      } catch (e) {
        console.warn('preview server unavailable, rendering inline', e);
      }
      openArtifact({
        title: r.path.split(/[\\/]/).pop() || path,
        language: lang,
        code: r.content,
        note: r.truncated ? 'Showing the first 4 MB of this file.' : undefined,
        path,
        url,
      });
    } catch (e) { alert(String(e)); }
  }, [sessionId, openArtifact]);
  // Live refresh: the agent rewrote `path` — reload every tracked artifact
  // for it so an open preview never goes stale. Failures keep the old copy
  // (file deleted/moved mid-run).
  const refreshFileArtifacts = async (path: string) => {
    const norm = (s: string) => s.trim().replace(/\\/g, '/').replace(/^\.\//, '');
    // The agent may echo the same file as `index.html` on one call and an
    // absolute path on the next (per-call cwd, absolute writes). Match on a
    // path boundary so both forms reach the same artifact.
    const same = (a: string, b: string) =>
      a === b || a.endsWith(`/${b}`) || b.endsWith(`/${a}`);
    const dirOf = (p: string) => {
      const i = p.lastIndexOf('/');
      return i < 0 ? '' : p.slice(0, i);
    };
    // An empty directory means the path is root-relative — the same directory
    // in a different form, so it matches anything.
    const sameDir = (a: string, b: string) => {
      const da = dirOf(a);
      const db = dirOf(b);
      return da === db || !da || !db || da.endsWith(`/${db}`) || db.endsWith(`/${da}`);
    };
    const target = norm(path);
    const tracked = artifactsRef.current.filter(a => !!a.path);
    // A write to a tracked file reloads it. A write to a *sibling* — the
    // stylesheet or script a tracked page links — reloads that page too: the
    // agent writes index.html first, so without this the drawer would keep the
    // first unstyled paint while style.css and app.js land behind it.
    const direct = tracked.filter(a => same(norm(a.path!), target));
    const hits = direct.length ? direct : tracked.filter(a => sameDir(norm(a.path!), target));
    if (!hits.length) return;
    for (const hit of hits) {
      const p = hit.path!;
      try {
        const r = await invoke<{ path: string; content: string; size: number; truncated: boolean }>('preview_workspace_file', { path: p, session_id: sessionId });
        setArtifacts(prev => prev.map(a =>
          a.path && same(norm(a.path), norm(p))
            ? { ...a, code: r.content, rev: (a.rev ?? 0) + 1, note: r.truncated ? 'Showing the first 4 MB of this file.' : undefined }
            : a,
        ));
      } catch { /* keep the stale copy */ }
    }
  };
  // Auto-open the viewer when the agent writes a previewable file, so the
  // result shows up without hunting for the eye icon. Controlled by the
  // `auto_preview` UI pref (Settings → Behavior, on by default). Gated on
  // `autoOpenPendingRef` so a run opens it at most once, and never while the
  // drawer is already open — an open drawer is served by the refresh above.
  const maybeAutoOpenArtifact = useCallback((path: string) => {
    if (ui.auto_preview === false) return;
    if (!autoOpenPendingRef.current) return;
    if (!artifactLanguageForPath(path)) return;
    autoOpenPendingRef.current = false;
    void openFileArtifact(path);
  }, [ui.auto_preview, openFileArtifact]);
  // Clarifying-question batches waiting on the user (`ask_user` tool).
  // Source of truth is the loop: re-sync on session switch so a run left
  // waiting in another session shows its card when you return.
  const [pendingQuestions, setPendingQuestions] = useState<PendingBatch[]>([]);
  const [answeringIds, setAnsweringIds] = useState<Record<string, boolean>>({});
  useEffect(() => {
    invoke<PendingBatch[]>('get_pending_questions', { session_id: sessionId })
      .then(qs => setPendingQuestions(qs))
      .catch(() => setPendingQuestions([]));
  }, [sessionId]);
  const answerQuestion = async (id: string, answers: string[]) => {
    setAnsweringIds(prev => ({ ...prev, [id]: true }));
    try {
      await invoke('answer_question', { session_id: sessionId, question_id: id, answers });
      setPendingQuestions(prev => prev.filter(q => q.id !== id));
    } catch {
      // Stale card (run ended/cancelled) or transient failure: re-sync and
      // let the user retry if the batch is genuinely still pending.
      try {
        const qs = await invoke<PendingBatch[]>('get_pending_questions', { session_id: sessionId });
        setPendingQuestions(qs);
      } catch {
        setPendingQuestions(prev => prev.filter(q => q.id !== id));
      }
    } finally {
      setAnsweringIds(prev => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
    }
  };
  const [isStreaming, setIsStreaming] = useState(false);
  const isStreamingRef = useRef(false);
  const streamFrameRef = useRef<number | null>(null);
  const streamTargetMapRef = useRef<Map<string, { current: string; target: string }>>(new Map());
  const thinkingStartTimeRef = useRef<number>(0);
  const currentAssistantIdRef = useRef<string | null>(null);
  const runHere = runSessionId === sessionId;
  const busy = (isLoading || isStreaming) && runHere;
  // Settings → Status renders the live run, so mirror it up. Only the session
  // that owns the run reports; switching chats leaves the other session's run
  // out of the panel instead of showing someone else's numbers.
  useEffect(() => {
    onRunStatus?.(runHere ? runStatus : null, runHere ? runSummary : null);
  }, [runStatus, runSummary, runHere, onRunStatus]);

  const clearStreamingLoop = () => {
    if (streamFrameRef.current !== null) {
      cancelAnimationFrame(streamFrameRef.current);
      streamFrameRef.current = null;
    }
  };

  const pushStreamingText = (msgId: string, deltaText: string) => {
    let existing = streamTargetMapRef.current.get(msgId);
    if (!existing) {
      existing = { current: '', target: '' };
      streamTargetMapRef.current.set(msgId, existing);
    }
    // Backend emits deltas per chunk — append, never overwrite,
    // or each chunk would replace the previous one and garble the stream.
    existing.target += deltaText;

    setIsStreaming(true);
    isStreamingRef.current = true;

    if (streamFrameRef.current !== null) return;

    let lastTick = performance.now();
    const pumpLoop = (now: number) => {
      // Throttled pump (~100ms): one setMessages per tick for all active
      // streams. 16ms rAF cadence re-rendered the whole transcript 60x/sec
      // (each tick re-parsing every message's markdown) and stacked smooth
      // scroll animations — cost grows with transcript length.
      if (now - lastTick >= 100) {
        lastTick = now;
        let activeAny = false;
        const advanced = new Map<string, string>();
        streamTargetMapRef.current.forEach((data, id) => {
          if (data.current.length < data.target.length) {
            activeAny = true;
            const remaining = data.target.slice(data.current.length);
            const nextSpace = remaining.indexOf(' ');
            // Natural token pacing (word-chunk stream), larger steps since
            // ticks are less frequent.
            const stepSize = nextSpace > 0 && nextSpace <= 24 ? nextSpace + 1 : Math.min(remaining.length, 16);
            data.current += remaining.slice(0, stepSize);
            if (data.current.length >= data.target.length) data.current = data.target;
            advanced.set(id, data.current);
          }
        });

        if (advanced.size > 0) {
          setMessages(prev =>
            prev.map(m => {
              const text = advanced.get(m.id);
              return text !== undefined ? { ...m, content: text, isStreaming: true } : m;
            })
          );
        }

        if (uiRef.current.auto_scroll) {
          const sc = scrollerRef.current;
          if (sc) {
            // Only follow when the user is already near the bottom, and use
            // instant scrolling while streaming: smooth per tick queues
            // overlapping animations and yanks scrolled-up readers.
            const nearBottom = sc.scrollHeight - sc.scrollTop - sc.clientHeight < 160;
            if (nearBottom) sc.scrollTo({ top: sc.scrollHeight, behavior: 'auto' });
          }
        }

        if (!activeAny) {
          clearStreamingLoop();
          isStreamingRef.current = false;
          setIsStreaming(false);
          setMessages(prev => prev.map(m => (m.isStreaming ? { ...m, isStreaming: false } : m)));
          // Read the live run state: the frozen mount-time closure value would
          // otherwise kill the spinner while the run is still in flight.
          if (!isLoadingRef.current) {
            setIsLoading(false);
          }
          return;
        }
      }
      streamFrameRef.current = requestAnimationFrame(pumpLoop);
    };

    streamFrameRef.current = requestAnimationFrame(pumpLoop);
  };

  // Flush streaming text on window regain focus
  useEffect(() => {
    const onVisibility = () => {
      if (document.visibilityState === 'visible' && isStreamingRef.current) {
        streamTargetMapRef.current.forEach((data, id) => {
          if (data.current.length < data.target.length) {
            data.current = data.target;
            setMessages(prev =>
              prev.map(m => (m.id === id ? { ...m, content: data.target, isStreaming: false } : m))
            );
          }
        });
        clearStreamingLoop();
        isStreamingRef.current = false;
        setIsStreaming(false);
      }
    };
    document.addEventListener('visibilitychange', onVisibility);
    return () => document.removeEventListener('visibilitychange', onVisibility);
  }, []);

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

  // A run that ends before the first AssistantText (error, stop, cap, empty
  // completion) must not leave ThoughtLine spinning: flip its thinking off
  // and stamp a duration so it reads as finished. Returns the same array when
  // nothing is thinking, so React bails out of the state update.
  const finalizeThinking = (prev: Message[]): Message[] => {
    if (!prev.some(m => m.thinking?.isThinking)) return prev;
    const startedAt = thinkingStartTimeRef.current || Date.now();
    const durationSec = Math.max(1, Math.round((Date.now() - startedAt) / 1000));
    return prev.map(m =>
      m.thinking?.isThinking
        ? { ...m, thinking: { ...m.thinking, isThinking: false, durationSec: m.thinking.durationSec || durationSec } }
        : m,
    );
  };

  const finishRun = (kind: 'done' | 'error' | 'cancelled' | 'cap', detail: string) => {
    if (kind !== 'done') {
      clearStreamingLoop();
      isStreamingRef.current = false;
      setIsStreaming(false);
      setMessages(prev => finalizeThinking(prev.map(m => (m.isStreaming ? { ...m, isStreaming: false } : m))));
    } else {
      setMessages(prev => finalizeThinking(prev));
    }
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
    // A finished run can never be waiting on the user — drop stale cards.
    setPendingQuestions([]);
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
      const raw = await invoke<any[]>('get_session_messages', { session_id: targetSessionId });
      if (seq !== loadSeqRef.current) return;
      const mapped: Message[] = (raw || []).map(m => ({
        ...m,
        timestamp: m.timestamp ? new Date(m.timestamp) : new Date(),
      }));
      setMessages(prev => {
        const streamingMsg = prev.find(m => m.isStreaming);
        const thinkingMap = new Map<string, any>();
        prev.forEach(m => {
          if (m.thinking) thinkingMap.set(m.id, m.thinking);
        });
        const next = mapped.map(m => {
          if (streamingMsg && m.id === streamingMsg.id) {
            return streamingMsg;
          }
          if (thinkingMap.has(m.id)) {
            return { ...m, thinking: thinkingMap.get(m.id) };
          }
          return m;
        });
        if (streamingMsg && !next.some(m => m.id === streamingMsg.id)) {
          next.push(streamingMsg);
        }
        return next;
      });
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
    // Artifacts belong to the visible session — never carry them across.
    setArtifacts([]);
    setActiveArtifactId(null);
    setArtifactOpen(false);

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
      clearStreamingLoop();
      isStreamingRef.current = false;
      setIsStreaming(false);
      streamTargetMapRef.current.clear();
      // Drop the streaming-bubble id with the stream buffers: a stale id from
      // the previous session never matches the reloaded rows and would append
      // a duplicate assistant bubble.
      currentAssistantIdRef.current = null;
      if (unlisten) unlisten();
    };
  }, [sessionId]);

  // Textarea sizing is owned by PromptBar — no auto-resize effect here.

  const sanitizeThinkingStep = (raw: string): string => {
    const trimmed = raw.trim();
    if (/^building request for turn \d+/i.test(trimmed)) {
      return 'Analyzing prompt and selecting capabilities';
    }
    if (/^provider returned \d+ item/i.test(trimmed)) {
      return 'Synthesizing response';
    }
    return trimmed;
  };

  const handleAgentEvent = (event: AgentEventType) => {
    // The listener already filters by session, but a second session can own
    // the run UI: events from a session that is not the current run's session
    // must not mutate another run's status bar / transcript.
    if (runSessionIdRef.current !== sessionId) return;
    switch (event.type) {
      case 'TurnStarted':
        touchStatus({ turn: event.turn });
        break;
      case 'ThinkingStarted': {
        // The run already terminalized (error/stop/cap): a straggler thinking
        // event must not restart the spinner that finishRun just cleared.
        if (runStatusRef.current?.finished) break;
        thinkingStartTimeRef.current = Date.now();
        const targetId = currentAssistantIdRef.current;
        setMessages(prev => {
          const idx = targetId ? prev.findIndex(m => m.id === targetId) : -1;
          if (idx >= 0) {
            const upd = [...prev];
            upd[idx] = {
              ...upd[idx],
              thinking: {
                open: false,
                steps: upd[idx].thinking?.steps || ['Analyzing request...'],
                isThinking: true
              }
            };
            return upd;
          }
          const newId = `asst-${Date.now()}`;
          currentAssistantIdRef.current = newId;
          return [...prev, {
            id: newId,
            role: 'assistant',
            content: '',
            timestamp: new Date(),
            thinking: { open: false, steps: ['Analyzing request...'], isThinking: true }
          }];
        });
        break;
      }
      case 'ThinkingStep': {
        if (runStatusRef.current?.finished) break;
        const sanitized = sanitizeThinkingStep(event.text);
        setMessages(prev => {
          const targetId = currentAssistantIdRef.current;
          const upd = [...prev];
          let idx = targetId ? upd.findIndex(m => m.id === targetId) : -1;
          if (idx < 0) {
            for (let i = upd.length - 1; i >= 0; i--) {
              if (upd[i].role === 'assistant') { idx = i; break; }
            }
          }
          if (idx >= 0) {
            const prevSteps = upd[idx].thinking?.steps || [];
            const newSteps = prevSteps[prevSteps.length - 1] === sanitized
              ? prevSteps
              : [...prevSteps, sanitized];
            upd[idx] = {
              ...upd[idx],
              thinking: {
                open: upd[idx].thinking?.open ?? false,
                steps: newSteps,
                isThinking: true
              }
            };
          }
          return upd;
        });
        break;
      }
      case 'AssistantText': {
        let incomingText = event.text;
        const thinkMatch = incomingText.match(/<think>([\s\S]*?)<\/think>/i);
        let extraThoughts: string[] = [];
        if (thinkMatch) {
          const rawThink = thinkMatch[1].trim();
          incomingText = incomingText.replace(/<think>[\s\S]*?<\/think>/i, '').trim();
          if (rawThink) {
            extraThoughts = rawThink.split('\n').map(s => s.trim()).filter(Boolean);
          }
        }

        const durationSec = Math.max(1, Math.round((Date.now() - (thinkingStartTimeRef.current || Date.now())) / 1000));
        let targetId = currentAssistantIdRef.current;
        if (!targetId) {
          targetId = `asst-${Date.now()}`;
          currentAssistantIdRef.current = targetId;
        }

        setMessages(prev => {
          const upd = [...prev];
          const idx = upd.findIndex(m => m.id === targetId);
          if (idx >= 0) {
            const existing = upd[idx];
            const existingThinking = existing.thinking;
            const combinedSteps = [...(existingThinking?.steps || []), ...extraThoughts];
            upd[idx] = {
              ...existing,
              thinking: existingThinking ? {
                ...existingThinking,
                steps: combinedSteps,
                isThinking: false,
                durationSec: existingThinking.durationSec || durationSec,
                open: false,
              } : (extraThoughts.length > 0 ? {
                open: false,
                steps: extraThoughts,
                isThinking: false,
                durationSec,
              } : undefined),
              isStreaming: true,
            };
            return upd;
          }
          return [...prev, {
            id: targetId!,
            role: 'assistant',
            content: '',
            isStreaming: true,
            timestamp: new Date(),
            thinking: {
              open: false,
              steps: extraThoughts.length > 0 ? extraThoughts : ['Completed thought process'],
              isThinking: false,
              durationSec,
            }
          }];
        });

        if (incomingText) {
          pushStreamingText(targetId, incomingText);
        }
        touchStatus({});
        break;
      }

      case 'ToolCallStarted':
        toolStartRef.current.push({ name: event.name, at: Date.now(), args: event.args });
        touchStatus({ lastTool: event.name });
        setMessages(prev => [...prev, { id: `tool-${Date.now()}-${Math.random()}`, role: 'tool', content: event.name, toolCalls: [{ id: `call-${Date.now()}`, name: event.name, arguments: event.args }], timestamp: new Date() }]);
        break;
      case 'ToolCallCompleted': {
        const at = Date.now();
        let durationMs: number | undefined;
        let startedArgs: string | undefined;
        const starts = toolStartRef.current;
        for (let i = starts.length - 1; i >= 0; i--) {
          if (starts[i].name === event.name) {
            durationMs = at - starts[i].at;
            startedArgs = starts[i].args;
            starts.splice(i, 1);
            break;
          }
        }
        setMessages(prev => {
          const upd = [...prev];
          let idx = -1;
          // Pair by tool name against a row still awaiting its result: a
          // reverse scan by role alone lands concurrent-call outputs on the
          // wrong card.
          for (let i = upd.length - 1; i >= 0; i--) {
            if (upd[i].role === 'tool' && !upd[i].toolResult && upd[i].toolCalls?.[0]?.name === event.name) { idx = i; break; }
          }
          if (idx >= 0) {
            upd[idx] = { ...upd[idx], content: event.name, durationMs, toolResult: { toolCallId: `call-${Date.now()}`, content: event.output } };
          }
          return upd;
        });
        // Live artifact refresh: a successful rewrite of a tracked file
        // reloads its preview so an open drawer never goes stale. Success is
        // "no error marker", not one tool's exact wording — `search_replace`
        // returns a structured result, never "Successfully wrote …".
        if (WRITE_TOOL_NAMES.includes(event.name) && writeSucceeded(event.output)) {
          let p: string | null = null;
          if (startedArgs) {
            try {
              const a = JSON.parse(startedArgs);
              const cand = a.path ?? a.file_path ?? a.target_file ?? a.filename;
              if (typeof cand === 'string' && cand.trim()) p = cand.trim();
            } catch { /* fall through to result parse */ }
          }
          if (!p) {
            const m = event.output.match(/Successfully wrote \d+ bytes to (.+?)\s*$/);
            if (m) p = m[1].trim();
          }
          if (p) { refreshFileArtifacts(p); maybeAutoOpenArtifact(p); }
        }
        touchStatus({ lastTool: event.name });
        break;
      }
      // NOTE: the backend emits TurnCompleted per segment, not per run, so
      // this only refreshes the transcript — the run finalizes when
      // `send_message` resolves (or on Cancelled / SpendCapReached / Error).
      case 'TurnCompleted':
        if (!isStreamingRef.current) {
          loadSessionMessages(sessionId);
        }
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
      case 'UsageUpdated': {
        // Throttled to ~1Hz: the backend emits per token batch and each
        // update re-renders the whole app via the onRunStatus mirror.
        const now = Date.now();
        if (now - lastStatusPushRef.current >= 1000) {
          lastStatusPushRef.current = now;
          setStatus(prev =>
            prev ? { ...prev, totalTokens: event.total_tokens, costUsd: event.cost_usd, lastActivityAt: now } : prev,
          );
        } else {
          // Keep the ref fresh so the status bar lands on final totals.
          const s = runStatusRef.current;
          if (s) {
            s.totalTokens = event.total_tokens;
            s.costUsd = event.cost_usd;
          }
        }
        break;
      }
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
      case 'QuestionAsked':
        setPendingQuestions(prev => prev.some(q => q.id === event.id)
          ? prev
          : [...prev, { id: event.id, questions: event.questions }]);
        break;
      default:
        break;
    }
    if (uiRef.current.auto_scroll) {
      requestAnimationFrame(() => scrollerRef.current?.scrollTo({ top: scrollerRef.current.scrollHeight, behavior: 'smooth' }));
    }
  };

  const handleSend = async (draft: string) => {
    if ((!draft.trim() && !attachments.length) || busy) return;
    // Attachments serialize as fenced blocks; prompt context first, ask last.
    const blocks = attachments
      .map(a => `[Attached file: ${a.name}]\n\`\`\`\n${a.content}\n\`\`\``)
      .join('\n\n');
    const text = blocks ? `${blocks}\n\n${draft.trim()}` : draft;
    const sentSession = sessionId;
    const userMessage: Message = { id: `msg-${Date.now()}`, role: 'user', content: text, timestamp: new Date() };
    const assistantId = `asst-${Date.now()}`;
    currentAssistantIdRef.current = assistantId;
    thinkingStartTimeRef.current = Date.now();
    const assistantMessage: Message = {
      id: assistantId,
      role: 'assistant',
      content: '',
      timestamp: new Date(),
      thinking: { open: false, steps: ['Analyzing request and selecting capabilities'], isThinking: true }
    };
    setMessages(prev => [...prev, userMessage, assistantMessage]);
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
    // Auto-open the artifact viewer at most once per run, and never while the
    // drawer is already open (that would yank the user's active artifact out
    // from under them — an open drawer just gets refreshed instead).
    autoOpenPendingRef.current = !artifactOpenRef.current;
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
    // A session switch hands the run UI to whoever is visible now, and a newer
    // send overwrites run ownership entirely — a stale continuation must stop
    // writing into the wrong transcript in both cases.
    const stillMyRun = () => runSessionIdRef.current === sentSession;
    const visibleHere = () => sessionIdRef.current === sentSession;
    try {
      await invoke('send_message', {
        session_id: sentSession,
        text,
        effort: effort && efforts.includes(effort) ? effort : null,
      });
      if (!stillMyRun()) {
        notifyUser('Maverick — run finished', 'A background run finished.');
      } else {
        // Transcript load only matters while this session is on screen; the
        // run summary/status updates are gated by `runHere` at render time.
        if (visibleHere() && !isStreamingRef.current) await loadSessionMessages(sentSession);
        // Natural finish (or graceful cap stop): finalize unless a terminal
        // event (cap / cancel / error) already did.
        if (!runStatusRef.current?.finished) finishRun('done', '');
      }
    } catch (error) {
      // A terminal event (Error / Cancelled) usually already explained the
      // failure — only add a bubble when the run died before emitting one.
      // A busy rejection (double-send while a run is in flight) is not a
      // failure: notify and leave the transcript alone.
      const msg = `${error}`;
      if (msg.includes('already in progress')) {
        notifyUser('Maverick — already running', 'A run is already in progress for this session.');
        // This send never became a run: undo the optimistic run state handleSend
        // opened for it, or the status bar and the taskbar spinner stay active
        // forever with nothing behind them.
        setStatus(null);
        setRunSummary(null);
        setTaskbarProgress('none');
        return;
      }
      if (!stillMyRun()) {
        notifyUser('Maverick — run failed', msg.slice(0, 200));
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

  // Generative-UI component interactions (OpenUI form submits, action buttons)
  // arrive here. A ref keeps the callback identity stable so MemoBubble keeps
  // working; the ref mirror is the file's standard trick for the same problem.
  const handleSendRef = useRef(handleSend);
  handleSendRef.current = handleSend;
  const handleOpenUiAction = useCallback((event: OpenUiActionEvent) => {
    if (event.type === 'open_url' && typeof event.params?.url === 'string') {
      window.open(event.params.url, '_blank', 'noopener,noreferrer');
      return;
    }
    // Everything else continues the conversation: the action's human-readable
    // message plus the form state (if any) becomes a normal user turn.
    const state =
      event.formState && Object.keys(event.formState).length > 0
        ? `\n\n\`\`\`json\n${JSON.stringify(event.formState, null, 2)}\n\`\`\``
        : '';
    void handleSendRef.current(`${event.humanFriendlyMessage}${state}`);
  }, []);

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
    clearStreamingLoop();
    isStreamingRef.current = false;
    setIsStreaming(false);
    streamTargetMapRef.current.clear();
    setMessages(prev => finalizeThinking(prev.map(m => (m.isStreaming ? { ...m, isStreaming: false } : m))));
    setIsLoading(false);
    // Cancel the run's own session, not whatever session happens to be visible.
    const target = runSessionIdRef.current ?? sessionId;
    try {
      await invoke('cancel_message', { session_id: target });
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
  // Home screen = no messages yet: the composer lifts to the vertical center
  // with the fun headline above it and the suggestion cards below it.
  const isEmptyState = messages.length === 0;

  return (
    <div style={{display:'flex', flexDirection:'column', flex:1, minHeight:0, background:'var(--bg)', position:'relative',
      // Dock the composer column beside the artifact viewer instead of
      // letting it continue underneath the overlay.
      marginRight: artifactOpen ? drawerWidth : 0, transition: 'margin-right 0.2s ease'}}>
      {/* Messages — collapsed on the home screen; the empty-state hero lives
          inside the dock below so the composer can center vertically. */}
      <div
        ref={scrollerRef}
        style={isEmptyState
          ? { height: 0, flexShrink: 0, overflow: 'hidden' }
          : { flex: 1, overflowY: 'auto', overflowX: 'hidden', display: 'flex', flexDirection: 'column' }}
      >
        <div style={{width:'100%', maxWidth:'768px', margin:'0 auto', flex:1, display:'flex', flexDirection:'column', padding:'24px 20px 0', gap:'0', minHeight:'100%'}}>
          <div style={{display:'flex', flexDirection:'column', gap:'0', paddingBottom:'24px'}}>
            {visibleMessages.map(msg => (
              <MemoBubble
                key={msg.id}
                message={msg}
                compact={ui.compact_mode}
                onOpenArtifact={openArtifact}
                onPreviewFile={openFileArtifact}
                genUiEnabled={ui.generative_ui !== false}
                onOpenUiAction={handleOpenUiAction}
              />
            ))}
            <div ref={messagesEndRef} />
          </div>
        </div>
      </div>

      {/* Floating prompt dock — on the home screen it expands to fill the
          viewport and centers logo → fun headline → composer. */}
      <div
        style={isEmptyState
          ? { flex: 1, display: 'flex', flexDirection: 'column', justifyContent: 'center', alignItems: 'center', padding: '0 16px', background: 'none' }
          : { padding: '0 16px 20px', background: 'linear-gradient(transparent, var(--bg) 28%)', flexShrink: 0, display: 'flex', justifyContent: 'center' }}
      >
        <div
          className="thread-composer-surface"
          style={{ width: '100%', maxWidth: '768px', display: 'flex', flexDirection: 'column', gap: isEmptyState ? '18px' : '8px' }}
        >
          {isEmptyState && (
            <div style={{display:'flex', flexDirection:'column', alignItems:'center', gap:'18px', textAlign:'center'}}>
              <div style={{
                width:'48px', height:'48px', borderRadius:'50%', background:'var(--panel)',
                border:'1px solid var(--line)', display:'flex', alignItems:'center', justifyContent:'center',
              }}>
                <ApertureLogo size={28} />
              </div>
              <div
                onClick={() => setGreeting(prev => pickHeadline(prev))}
                title="Click for another"
                style={{ cursor:'pointer', userSelect:'none' }}
              >
                <ShinyText
                  text={greeting}
                  color="var(--muted)"
                  shineColor="var(--text)"
                  spread={120}
                  speed={3}
                  direction="left"
                  className="text-[30px] font-medium tracking-[-0.025em] leading-[1.2]"
                />
              </div>
            </div>
          )}
          {runHere && runBanner && (
            <div
              className="mono"
              style={{
                fontSize:'11px', padding:'7px 12px', borderRadius:'10px', border:'1px solid',
                ...(runBanner.kind === 'cap'
                  ? { color:'var(--error)', background:'var(--error-bg)', borderColor:'var(--error-border)' }
                  : { color:'var(--warn)', background:'var(--warn-bg)', borderColor:'var(--warn-border)' }),
              }}
            >
              {runBanner.text}
            </div>
          )}
          {runHere && pendingQuestions.length > 0 && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
              {pendingQuestions.map(b => (
                <QuestionCard
                  key={b.id}
                  batch={b}
                  answering={!!answeringIds[b.id]}
                  onAnswer={answerQuestion}
                />
              ))}
            </div>
          )}
            {/* Composer — PromptBar owns the field, model picker, and effort slider */}
            <PromptBar
              value={input}
              onChange={setInput}
              inputRef={textareaRef}
              onSend={t => handleSend(t)}
              onStop={handleCancel}
              models={composerModels}
              defaultModel={activePreset ? `preset:${activePreset.id}` : selectedProvider}
            onModelChange={key => {
              if (key.startsWith('preset:')) {
                const preset = presets.find(p => `preset:${p.id}` === key);
                if (preset) onApplyPreset?.(preset);
                return;
              }
              onProviderChange(key);
            }}
            modelMenuFooter={
              onSavePresetRequest ? (
                <button
                  type="button"
                  className="prompt-bar__menu-save"
                  onMouseDown={e => e.preventDefault()}
                  onClick={() => onSavePresetRequest()}
                >
                  + Save current as preset…
                </button>
              ) : undefined
            }
            efforts={efforts}
            defaultEffort={effort}
            onEffortChange={setEffort}
            chips={attachments.map(a => ({ key: a.id, name: a.name, meta: fmtBytes(a.size) }))}
            onRemoveChip={key => setAttachments(prev => prev.filter(x => x.id !== key))}
            extraCanSend={attachments.length > 0}
            leftSlot={
              <ComposerPlusMenu
                sessionId={sessionId}
                onAttach={a => setAttachments(prev => {
                  if (prev.length >= 6) { alert('Attachment limit is 6 files per message'); return prev; }
                  return [...prev, a];
                })}
                onInsertSkill={insertSkill}
                onAddMcp={() => setShowMcpDialog(true)}
                memoryEnabled={memoryEnabled}
                onToggleMemory={toggleMemory}
              />
            }
            sources={[]}
            commands={[]}
            busy={busy}
            background="#00000d"
            color="#f5f5f5"
            menuBackground="#000006"
            sparkColor="#ffffff"
            sparkBoost={1}
            width={768}
            radius={16}
          />
        </div>
      </div>
      {artifactOpen && (
        <ArtifactDrawer
          artifacts={artifacts}
          activeId={activeArtifactId}
          onSelect={setActiveArtifactId}
          onClose={() => setArtifactOpen(false)}
          onWidthChange={setDrawerWidth}
        />
      )}
      {showMcpDialog && (
        <div style={{position:'fixed', inset:0, background:'rgba(0,0,0,0.55)', backdropFilter:'blur(10px)', display:'flex', alignItems:'center', justifyContent:'center', zIndex:100, padding:'16px'}}>
          <div className="panel" style={{width:'100%', maxWidth:'520px', padding:'22px', borderRadius:'20px', border:'1px solid var(--line)'}}>
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

function MessageBubble({ message, compact = false, onOpenArtifact, onPreviewFile, genUiEnabled, onOpenUiAction }: {
  message: Message;
  compact?: boolean;
  onOpenArtifact?: (a: { language: string; code: string; title: string }) => void;
  onPreviewFile?: (path: string) => void;
  genUiEnabled: boolean;
  onOpenUiAction: (event: OpenUiActionEvent) => void;
}) {
  const [copied, setCopied] = useState(false);
  const [open, setOpen] = useState(false);
  const isUser = message.role === 'user';
  const isTool = message.role === 'tool';
  const isError = message.content.startsWith('Error:') || message.content.startsWith('Send failed:');

  const handleCopy = async (text: string) => {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {}
  };

  // 1. Tool Call Capsule (hybrid collapsible card)
  if (isTool) {
    const hasResult = !!message.toolResult;
    const isRunning = !hasResult;
    const toolName = message.content;
    const duration = message.durationMs != null ? `${message.durationMs}ms` : null;
    const args = message.toolCalls?.[0]?.arguments;
    const output = message.toolResult?.content;

    const chipIcon: CallChipIcon =
      toolName === 'bash' || toolName === 'shell'
        ? 'terminal'
        : toolName === 'grep' || toolName === 'search' || toolName === 'glob'
          ? 'search'
          : toolName === 'edit' || toolName === 'write'
            ? 'edit'
            : 'file';
    let chipArg = '';
    if (args) {
      try {
        const parsed = JSON.parse(args);
        chipArg = parsed.command ?? parsed.path ?? parsed.query ?? parsed.file_path ?? '';
      } catch {
        if (args.length < 60 && !args.startsWith('{')) chipArg = args;
      }
      if (!chipArg) chipArg = args.length > 60 ? `${args.slice(0, 57)}…` : args;
    }
    const chipStatus = isRunning
      ? 'running'
      : output?.startsWith(TOOL_ERROR_PREFIX) || output?.startsWith('Error')
        ? 'error'
        : 'done';
    // Artifact viewer: successful write-family calls targeting a previewable
    // path get a Preview-file button (live args or the result line).
    const writePath =
      !isRunning && WRITE_TOOL_NAMES.includes(toolName) && writeSucceeded(output)
        ? extractWritePath(message)
        : null;
    const writePreviewLang = writePath ? artifactLanguageForPath(writePath) : null;

    return (
      <div style={{ margin: compact ? '4px 0' : '6px 0', width: '100%' }}>
        <button
          onClick={() => setOpen(o => !o)}
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: '8px',
            padding: '4px 8px',
            fontSize: '12.5px',
            borderRadius: '10px',
            border: '1px solid var(--line)',
            background: open ? 'var(--control-hover)' : 'transparent',
            color: 'var(--text)',
            cursor: 'pointer',
            maxWidth: '100%',
            transition: 'all 0.15s ease',
          }}
          title={open ? 'Click to collapse' : 'Click to inspect execution'}
        >
          <CallChip
            icon={chipIcon}
            name={toolName}
            argument={chipArg}
            status={chipStatus}
            size={30}
            radius={9}
            color="var(--text)"
            surfaceColor="var(--panel-2)"
            progressColor="var(--accent)"
            doneColor="var(--ok)"
            errorColor="var(--error)"
            showTimer={isRunning}
          />

          {duration && !isRunning && (
            <span className="mono" style={{ fontSize: '11px', color: 'var(--faint)' }}>
              ({duration})
            </span>
          )}

          <span
            style={{
              flexShrink: 0,
              transition: 'transform 0.3s cubic-bezier(0.16, 1, 0.3, 1)',
              transform: open ? 'rotate(180deg)' : 'none',
              color: 'var(--faint)',
              marginLeft: '2px',
              display: 'flex',
            }}
          >
            <ChevronIcon size={12} />
          </span>
        </button>

        {/* Smooth CSS Grid Drawer */}
        <div
          style={{
            display: 'grid',
            gridTemplateRows: open ? '1fr' : '0fr',
            opacity: open ? 1 : 0,
            transition: 'grid-template-rows 300ms cubic-bezier(0.16, 1, 0.3, 1), opacity 200ms ease',
          }}
        >
          <div style={{ minHeight: 0, overflow: 'hidden' }}>
            <div
              style={{
                marginTop: '6px',
                borderRadius: '10px',
                border: '1px solid var(--line)',
                background: 'var(--code-surface)',
                overflow: 'hidden',
                fontSize: '12px',
              }}
            >
              {args && (
                <div style={{ borderBottom: output ? '1px solid var(--line)' : 'none' }}>
                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', padding: '6px 12px', background: 'var(--code-head)' }}>
                    <span className="mono" style={{ fontSize: '11px', color: 'var(--muted)', textTransform: 'uppercase', letterSpacing: '0.04em' }}>Input</span>
                    <CopySnippet text={args} />
                  </div>
                  <pre className="mono" style={{ margin: 0, padding: '10px 12px', overflowX: 'auto', color: 'var(--code-fg)', lineHeight: 1.5, fontSize: '11.5px' }}>
                    <code>{args}</code>
                  </pre>
                </div>
              )}
              {output && (
                <div>
                  <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', padding: '6px 12px', background: 'var(--code-head)' }}>
                    <span className="mono" style={{ fontSize: '11px', color: 'var(--muted)', textTransform: 'uppercase', letterSpacing: '0.04em' }}>Output</span>
                    <span style={{ display: 'inline-flex', alignItems: 'center', gap: '6px' }}>
                      {writePreviewLang && writePath && (
                        <button
                          onClick={(e) => { e.stopPropagation(); onPreviewFile?.(writePath); }}
                          className="btn-ghost"
                          style={{ padding: '3px 8px', fontSize: '11px', borderRadius: '6px', color: 'var(--muted)', cursor: 'pointer' }}
                          title={`Open ${writePath} in artifact viewer`}
                        >
                          Preview file
                        </button>
                      )}
                      <CopySnippet text={output} />
                    </span>
                  </div>
                  <pre className="mono" style={{ margin: 0, padding: '10px 12px', overflowX: 'auto', maxHeight: '240px', color: 'var(--code-fg)', lineHeight: 1.5, fontSize: '11.5px', whiteSpace: 'pre-wrap' }}>
                    <code>{output}</code>
                  </pre>
                </div>
              )}
            </div>
          </div>
        </div>
      </div>
    );
  }

  // 2. User Message
  if (isUser) {
    return (
      <div style={{ display: 'flex', justifyContent: 'flex-end', padding: compact ? '4px 0' : '10px 0' }}>
        <div
          style={{
            maxWidth: '82%',
            background: 'var(--bubble-user)',
            color: 'var(--text)',
            padding: '10px 18px',
            borderRadius: '22px',
            border: '1px solid var(--line)',
          }}
        >
          <div style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-word', fontSize: '15px', lineHeight: 1.6 }}>
            {message.content}
          </div>
        </div>
      </div>
    );
  }

  // 3. Assistant Message (with ChatGPT o1/o3 reasoning fold)
  const isCurrentlyBusy = message.isStreaming || message.thinking?.isThinking;
  const hasContent = message.content.length > 0;

  return (
    <div style={{ display: 'flex', gap: '14px', padding: compact ? '6px 0' : '16px 0', alignItems: 'flex-start' }}>
      <ApertureTile size={28} />
      <div style={{ minWidth: 0, flex: 1, paddingTop: '1px' }}>
        {/* Collapsible Reasoning */}
        {message.thinking && message.thinking.steps.length > 0 && (
          <ThoughtLine
            working={!!message.thinking.isThinking}
            steps={message.thinking.steps}
            label="Thinking…"
            doneLabel="Thought for"
            glyph="sparkle"
            collapsible
            collapseOnSettle
            color="var(--muted)"
            fontSize={13}
            elapsed={message.thinking.durationSec}
          />
        )}

        {/* Assistant Content */}
        {(hasContent || message.isStreaming) && (
          <div style={{
            background: isError ? 'var(--error-bg)' : 'transparent',
            border: isError ? '1px solid var(--error-border)' : 'none',
            padding: isError ? '12px 14px' : '0',
            borderRadius: isError ? '12px' : '0'
          }}>
            {isError ? (
              <div style={{ whiteSpace: 'pre-wrap', color: 'var(--error)', fontSize: '14px' }}>{message.content}</div>
            ) : (
              <div style={{ position: 'relative' }}>
                <AssistantContent
                  content={message.content}
                  streaming={message.isStreaming}
                  genUiEnabled={genUiEnabled}
                  onOpenArtifact={onOpenArtifact}
                  onOpenUiAction={onOpenUiAction}
                />
                {message.isStreaming && <span className="streaming-caret" />}
              </div>
            )}
          </div>
        )}

        {/* Footer Actions */}
        {!isCurrentlyBusy && hasContent && (
          <div style={{ fontSize: '11px', color: 'var(--faint)', marginTop: '8px', display: 'flex', alignItems: 'center', gap: '12px' }}>
            <span>{message.timestamp.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</span>
            {!isError && (
              <button
                onClick={() => handleCopy(message.content)}
                className="btn-ghost"
                style={{
                  padding: '2px 6px',
                  fontSize: '11px',
                  borderRadius: '4px',
                  color: copied ? 'var(--ok)' : 'var(--muted)',
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
            {isError && <span style={{ color: 'var(--error)' }}>• needs attention</span>}
          </div>
        )}
      </div>
    </div>
  );
}
// Memoized transcript row: the stream pump replaces the messages array ~10x/sec,
// so without this every bubble (and its markdown parse) re-renders per tick.
// Only the bubble whose `message` object identity changed re-renders.
const MemoBubble = memo(MessageBubble);

function CopySnippet({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const handleCopy = async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {}
  };
  return (
    <button
      onClick={handleCopy}
      className="btn-ghost"
      style={{
        padding: '2px 6px',
        fontSize: '11px',
        borderRadius: '4px',
        color: copied ? 'var(--ok)' : 'var(--muted)',
        display: 'inline-flex',
        alignItems: 'center',
        gap: '4px',
        cursor: 'pointer',
      }}
    >
      {copied ? <CheckIcon size={11} /> : <CopyIcon size={11} />}
      <span>{copied ? 'Copied' : 'Copy'}</span>
    </button>
  );
}

