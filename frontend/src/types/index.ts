// Shared TypeScript types matching Rust backend

export interface Session {
  id: string;
  createdAt: string;
  updatedAt: string;
}

export interface ProviderInfo {
  id: string;
  name: string;
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
  | { type: 'AssistantText'; text: string }
  | { type: 'ToolCallStarted'; name: string; args: string }
  | { type: 'ToolCallCompleted'; name: string; output: string }
  | { type: 'TurnCompleted' }
  | { type: 'Error'; message: string };

export interface Message {
  id: string;
  role: 'user' | 'assistant' | 'tool';
  content: string;
  toolCalls?: ToolCall[];
  toolResult?: ToolResult;
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