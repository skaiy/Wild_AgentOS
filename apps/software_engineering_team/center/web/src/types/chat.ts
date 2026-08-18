export type MessageRole = 'user' | 'assistant' | 'system';

export type MessageContentType = 'text' | 'code' | 'diff' | 'terminal' | 'tool_call';

export interface ChatMessage {
  id: string;
  role: MessageRole;
  content: MessageContent[];
  createdAt: string;
  projectId?: string;
  stageId?: string;
}

export interface MessageContent {
  type: MessageContentType;
  data: string | CodeContent | DiffContent | TerminalContent | ToolCallContent;
}

export interface CodeContent {
  code: string;
  language: string;
}

export interface DiffContent {
  oldCode: string;
  newCode: string;
  language?: string;
}

export interface TerminalContent {
  log: string;
}

export interface ToolCallContent {
  toolName: string;
  arguments: Record<string, unknown>;
  result?: unknown;
  status: 'pending' | 'success' | 'error';
}

export interface SendMessageInput {
  content: string;
  projectId?: string;
  stageId?: string;
}