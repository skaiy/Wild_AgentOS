import { create } from 'zustand';
import type { ChatMessage, SendMessageInput } from '@/types';
import { sendChatMessageStream, sendChatMessage } from '@/api/chat';
import type { ChatMessageInput } from '@/api/chat';

interface ChatState {
  messages: ChatMessage[];
  streaming: boolean;
  projectId: string | null;
  abortController: AbortController | null;

  sendMessage: (input: SendMessageInput) => Promise<void>;
  stopStreaming: () => void;
  appendMessage: (msg: ChatMessage) => void;
  clearMessages: () => void;
  setProjectId: (projectId: string | null) => void;
}

export const useChatStore = create<ChatState>((set, get) => ({
  messages: [],
  streaming: false,
  projectId: null,
  abortController: null,

  sendMessage: async (input: SendMessageInput) => {
    const userMessage: ChatMessage = {
      id: `msg-${Date.now()}`,
      role: 'user',
      content: [{ type: 'text', data: input.content }],
      createdAt: new Date().toISOString(),
      projectId: input.projectId,
      stageId: input.stageId,
    };

    const assistantId = `msg-${Date.now() + 1}`;
    const assistantMessage: ChatMessage = {
      id: assistantId,
      role: 'assistant',
      content: [{ type: 'text', data: '' }],
      createdAt: new Date().toISOString(),
      projectId: input.projectId,
    };

    set((state) => ({
      messages: [...state.messages, userMessage, assistantMessage],
      streaming: true,
    }));

    const chatMessages: ChatMessageInput[] = get().messages
      .filter((m) => m.id !== assistantId)
      .map((m) => ({
        role: m.role as 'user' | 'assistant' | 'system',
        content: m.content
          .map((c) => {
            if (c.type === 'text') return c.data as string;
            if (c.type === 'code') {
              const codeData = c.data as { code: string; language: string };
              return `\`\`\`${codeData.language}\n${codeData.code}\n\`\`\``;
            }
            return '';
          })
          .join('\n'),
      }))
      .filter((m) => m.content.trim());

    try {
      await sendChatMessageStream(
        {
          messages: chatMessages,
          project_id: input.projectId,
        },
        (chunk, accumulated) => {
          set((state) => ({
            messages: state.messages.map((m) =>
              m.id === assistantId
                ? { ...m, content: [{ type: 'text' as const, data: accumulated }] }
                : m
            ),
          }));

          if (chunk.done) {
            set({ streaming: false, abortController: null });
          }
        },
      );

      set({ streaming: false, abortController: null });
    } catch (error) {
      set((state) => ({
        messages: state.messages.map((m) =>
          m.id === assistantId
            ? { ...m, content: [{ type: 'text' as const, data: `请求失败: ${(error as Error).message}` }] }
            : m
        ),
        streaming: false,
        abortController: null,
      }));
    }
  },

  stopStreaming: () => {
    const { abortController } = get();
    if (abortController) {
      abortController.abort();
    }
    set({ streaming: false, abortController: null });
  },

  appendMessage: (msg: ChatMessage) => {
    set((state) => ({
      messages: [...state.messages, msg],
    }));
  },

  clearMessages: () => set({ messages: [] }),

  setProjectId: (projectId: string | null) => set({ projectId }),
}));