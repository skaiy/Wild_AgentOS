const API_BASE = '/api/v1';

export interface ChatMessageInput {
  role: 'user' | 'assistant' | 'system';
  content: string;
}

export interface ChatRequest {
  messages: ChatMessageInput[];
  project_id?: string;
}

export interface ChatResponse {
  content: string;
  status: string;
  summary?: string;
  output_iri?: string;
  stage_id?: string;
}

export interface StreamChunk {
  content: string;
  done: boolean;
  status: string;
}

export type StreamCallback = (chunk: StreamChunk, accumulated: string) => void;

export async function sendChatMessage(request: ChatRequest): Promise<ChatResponse> {
  const response = await fetch(`${API_BASE}/chat/sync`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(request),
  });

  if (!response.ok) {
    const errData = await response.json().catch(() => ({ error: `HTTP ${response.status}` }));
    throw new Error(errData.error || `HTTP ${response.status}`);
  }

  const data: ChatResponse = await response.json();
  return data;
}

export async function sendChatMessageStream(
  request: ChatRequest,
  onChunk: StreamCallback,
): Promise<void> {
  const response = await fetch(`${API_BASE}/chat`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Accept': 'text/event-stream',
    },
    body: JSON.stringify(request),
  });

  if (!response.ok) {
    const errData = await response.json().catch(() => ({ error: `HTTP ${response.status}` }));
    throw new Error(errData.error || `HTTP ${response.status}`);
  }

  const reader = response.body?.getReader();
  if (!reader) {
    throw new Error('ReadableStream not supported');
  }

  const decoder = new TextDecoder();
  let accumulated = '';
  let buffer = '';

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;

    buffer += decoder.decode(value, { stream: true });

    const lines = buffer.split('\n');
    buffer = lines.pop() || '';

    for (const line of lines) {
      const trimmed = line.trim();
      if (!trimmed || !trimmed.startsWith('data: ')) continue;

      const jsonStr = trimmed.slice(6);
      if (!jsonStr) continue;

      try {
        const chunk: StreamChunk = JSON.parse(jsonStr);
        accumulated += chunk.content;
        onChunk(chunk, accumulated);

        if (chunk.done) {
          return;
        }
      } catch {
      }
    }
  }
}