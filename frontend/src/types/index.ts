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
  kind: 'Xai' | 'OpenAi' | 'Anthropic' | 'Subprocess' | 'Mcp';
  config: ProviderConfig;
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
  | { type: 'Error'; message: string };

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

export interface Message {
  id: string;
  role: 'user' | 'assistant' | 'tool';
  content: string;
  toolCalls?: ToolCall[];
  toolResult?: ToolResult;
  /** Frontend-local wall time from ToolCallStarted to ToolCallCompleted. */
  durationMs?: number;
  /** Frontend-local reasoning timeline (ThinkingStarted / ThinkingStep). */
  thinking?: { open: boolean; steps: string[] };
  timestamp: Date;
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