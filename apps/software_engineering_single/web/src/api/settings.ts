import { api } from './client';
import type { LLMConfig, ValidationResult, ConfigValidationRequest } from '@/types';

export const settingsApi = {
  getLLMConfig: () => api.get<LLMConfig>('config/llm'),

  saveLLMConfig: (config: LLMConfig) => api.post<{ success: boolean }>('config/llm', config),

  validateConfig: (request: ConfigValidationRequest) =>
    api.post<ValidationResult>('config/validate', request),
};
