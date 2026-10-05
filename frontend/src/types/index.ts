// Shared TypeScript types matching Rust backend

export interface Session {
  id: string;
  createdAt: string;
  updatedAt: string;
}

export interface ProviderInfo {
  id: string;
  name: string;
  model: string;
  kind: 'OpenAi' | 'Anthropic' | 'Subprocess' | 'Mcp';
  config: ProviderConfig;
  /** Provider kind accepts a `reasoning_effort` request field. */
  supports_reasoning_effort?: boolean;
}

/** Per-model reasoning capability resolved by the backend. Kilo gateways
 * answer from the live model catalog; other kinds report a kind-wide default. */
export interface ReasoningProfile {
  model: string;
  /** Model accepts a `reasoning_effort` request field at all. */
  supported: boolean;
  /** Wire tiers (`none`…`max`) in ascending order; empty when unsupported. */
  efforts: string[];
  /** `kilo-catalog` | `provider-kind` | `none`. */
  source: string;
}

/** Saved model preset (provider + model + optional effort), switchable from
 *  the composer menu and header badge. Field names are snake_case — they
 *  deserialize straight into Rust's `ModelPreset`. */
export interface ModelPreset {
  id: string;
  name: string;
  provider_id: string;
  model: string;
  /** Display effort label (`Extra`, `High`, …); null/absent = provider default. */
  effort?: string | null;
}

export type ProviderConfig =
  | { type: 'http' | 'Http'; baseUrl: string; apiKey?: string }
  | { type: 'subprocess' | 'Subprocess'; command: string; args: string[]; env: Array<[string, string]> | Record<string, string> };

export interface ToolDefinition {
  name: string;
  description: string;
  parameters: Record<string, any>;
}

export interface AgentEvent {
  session_id: string;
  event: AgentEventType;
}

export type AgentEventType =
  | { type: 'TurnStarted'; turn: number }
  | { type: 'ThinkingStarted'; turn: number }
  | { type: 'ThinkingStep'; turn: number; text: string }
  | { type: 'AssistantText'; text: string }
  | { type: 'ToolCallStarted'; name: string; args: string }
  | { type: 'ToolCallCompleted'; name: string; output: string }
  | { type: 'TurnCompleted' }
  | { type: 'BudgetWarning'; remaining: number }
  | { type: 'SegmentBoundary'; segment: number; max_segments: number }
  | { type: 'UsageUpdated'; segment: number; turn: number; prompt_tokens: number; completion_tokens: number; total_tokens: number; cost_usd: number }
  | { type: 'SpendWarning'; percent_used: number; total_tokens: number; budget_tokens: number }
  | { type: 'Cancelled'; segment: number }
  | { type: 'SpendCapReached'; spent_usd: number; cap_usd: number }
  | { type: 'Error'; message: string }
  | { type: 'QuestionAsked'; id: string; questions: PendingQuestion[] };

/** One selectable answer to an `ask_user` clarifying question. */
export interface QuestionOption {
  label: string;
  description: string;
}

/** A question awaiting a user answer (`QuestionAsked` payload + `get_pending_questions` row). */
export interface PendingQuestion {
  id: string;
  question: string;
  options: QuestionOption[];
  allow_custom: boolean;
}

/** One batched `ask_user` call awaiting answers: `get_pending_questions` row shape. */
export interface PendingBatch {
  id: string;
  questions: PendingQuestion[];
}

/** UI preferences persisted in `config.toml` (`UiConfig` on the Rust side). */
export interface UiConfig {
  theme: string;
  show_tool_calls: boolean;
  auto_scroll: boolean;
  compact_mode: boolean;
}

/** Fallback used before `get_ui_config` resolves, and to fill missing fields. */
export const DEFAULT_UI_CONFIG: UiConfig = {
  theme: 'dark',
  show_tool_calls: true,
  auto_scroll: true,
  compact_mode: false,
};

/** Unified cross-chat memory policy (`MemoryConfig` on the Rust side).
 *  Field names are snake_case — they deserialize straight into Rust. */
export type MemoryScope = 'both' | 'global' | 'workspace';

export interface MemoryConfig {
  enabled: boolean;
  auto_extract: boolean;
  scope: MemoryScope;
  extract_model: string;
  max_chars: number;
}

/** Fallback used before `get_memory_config` resolves. */
export const DEFAULT_MEMORY_CONFIG: MemoryConfig = {
  enabled: true,
  auto_extract: true,
  scope: 'both',
  extract_model: 'gpt-4o-mini',
  max_chars: 8000,
};

/** Memory file stats for the composer indicator (`MemoryStats`). */
export interface MemoryStats {
  global_bullets: number;
  global_chars: number;
  workspace_name: string;
  workspace_bullets: number;
  workspace_chars: number;
}

/** Clarifying-question policy (`InteractionConfig` on the Rust side). */
export interface InteractionConfig {
  ask_user_enabled: boolean;
}

/** One item in the artifact viewer drawer. */
export interface Artifact {
  id: string;
  title: string;
  /** Normalized: 'html' | 'svg' | 'markdown'. */
  language: string;
  code: string;
  note?: string;
  /** Workspace path for file artifacts (live-refresh on rewrite). */
  path?: string;
  /** Bumped on every refresh so the iframe reloads. */
  rev?: number;
}

/** Languages whose chat code blocks get an Open-preview button. */
export const PREVIEWABLE_LANGUAGES = ['html', 'htm', 'svg', 'markdown', 'md'];

/** Normalize a fence language to an artifact language, or null. */
export function previewLanguage(language: string): string | null {
  const l = language.trim().toLowerCase();
  if (l === 'html' || l === 'htm') return 'html';
  if (l === 'svg') return 'svg';
  if (l === 'markdown' || l === 'md') return 'markdown';
  return null;
}

/** Artifact language for a file path, or null when not previewable. */
export function artifactLanguageForPath(path: string): string | null {
  const m = path.trim().toLowerCase().match(/\.([a-z0-9]+)$/);
  if (!m) return null;
  return previewLanguage(m[1]);
}

/** Fallback used before the interaction config resolves. */
export const DEFAULT_INTERACTION_CONFIG: InteractionConfig = {
  ask_user_enabled: true,
};

export interface Message {
  id: string;
  role: 'user' | 'assistant' | 'tool';
  content: string;
  toolCalls?: ToolCall[];
  toolResult?: ToolResult;
  /** Frontend-local wall time from ToolCallStarted to ToolCallCompleted. */
  durationMs?: number;
  /** Frontend-local reasoning timeline (ThinkingStarted / ThinkingStep). */
  thinking?: { open: boolean; steps: string[]; isThinking?: boolean; durationSec?: number };
  timestamp: Date;
  isStreaming?: boolean;
}

export interface ToolCall {
  id: string;
  name: string;
  arguments: string;
}

export interface ToolResult {
  toolCallId: string;
  content: string;
}

export interface SkillDto {
  name: string;
  display_name?: string | null;
  description: string;
  path: string;
  scope: string;
  enabled: boolean;
  plugin_name?: string | null;
  when_to_use?: string | null;
  allowed_tools?: string[] | null;
}

export interface HubIndex {
  skills: Record<string, { version: string; description: string; author?: string; path: string }>;
}