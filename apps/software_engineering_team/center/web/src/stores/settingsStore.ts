import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type { ServerConfig, LLMConfig, AgentOSConfig, RuntimeConfig } from '@/types';
import {
  DEFAULT_SERVER_CONFIG,
  DEFAULT_LLM_CONFIG,
  DEFAULT_AGENT_OS_CONFIG,
  DEFAULT_RUNTIME_CONFIG,
} from '@/types';
import { settingsApi } from '@/api';

interface SettingsState {
  server: ServerConfig;
  llm: LLMConfig;
  agentOS: AgentOSConfig;
  runtime: RuntimeConfig;
  loading: boolean;
  error: string | null;

  loadSettings: () => void;
  saveSettings: () => Promise<void>;
  updateServerConfig: (config: Partial<ServerConfig>) => void;
  updateLLMConfig: (config: Partial<LLMConfig>) => void;
  updateAgentOSConfig: (config: Partial<AgentOSConfig>) => void;
  updateRuntimeConfig: (config: Partial<RuntimeConfig>) => void;
  resetToDefaults: () => void;
  clearError: () => void;
}

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set, get) => ({
      server: DEFAULT_SERVER_CONFIG,
      llm: DEFAULT_LLM_CONFIG,
      agentOS: DEFAULT_AGENT_OS_CONFIG,
      runtime: DEFAULT_RUNTIME_CONFIG,
      loading: false,
      error: null,

      loadSettings: () => {
        const { llm } = get();
        if (llm.apiKey) {
          settingsApi.saveLLMConfig({
            api_key: llm.apiKey,
            base_url: llm.baseUrl,
            model: llm.model,
          } as any).catch(() => {});
        }
      },

      saveSettings: async () => {
        const { llm } = get();
        set({ loading: true, error: null });
        try {
          await settingsApi.saveLLMConfig({
            api_key: llm.apiKey,
            base_url: llm.baseUrl,
            model: llm.model,
            temperature: llm.temperature,
            max_tokens: llm.maxTokens,
          } as any);
          set({ loading: false });
        } catch (error) {
          set({ error: (error as Error).message, loading: false });
        }
      },

      updateServerConfig: (config) => {
        set((state) => ({
          server: { ...state.server, ...config },
        }));
      },

      updateLLMConfig: (config) => {
        set((state) => ({
          llm: { ...state.llm, ...config },
        }));
      },

      updateAgentOSConfig: (config) => {
        set((state) => ({
          agentOS: { ...state.agentOS, ...config },
        }));
      },

      updateRuntimeConfig: (config) => {
        set((state) => ({
          runtime: { ...state.runtime, ...config },
        }));
      },

      resetToDefaults: () => {
        set({
          server: DEFAULT_SERVER_CONFIG,
          llm: DEFAULT_LLM_CONFIG,
          agentOS: DEFAULT_AGENT_OS_CONFIG,
          runtime: DEFAULT_RUNTIME_CONFIG,
          error: null,
        });
      },

      clearError: () => set({ error: null }),
    }),
    {
      name: 'app-settings',
      partialize: (state) => ({
        server: state.server,
        llm: state.llm,
        agentOS: state.agentOS,
        runtime: state.runtime,
      }),
      onRehydrateStorage: () => (state) => {
        if (state?.llm?.apiKey) {
          settingsApi.saveLLMConfig({
            api_key: state.llm.apiKey,
            base_url: state.llm.baseUrl,
            model: state.llm.model,
          } as any).catch(() => {});
        }
      },
    }
  )
);