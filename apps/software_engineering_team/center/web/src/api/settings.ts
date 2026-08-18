import { api } from './client';
import type { LLMConfig } from '@/types';

export const settingsApi = {
  getLLMConfig: () => api.get<LLMConfig>('config/llm'),

  saveLLMConfig: (config: Partial<LLMConfig>) => api.post<{ success: boolean }>('config/llm', config),
};