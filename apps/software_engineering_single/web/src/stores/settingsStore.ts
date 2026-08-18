import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import type {
  ServerConfig,
  LLMConfig,
  AgentOSConfig,
  RuntimeConfig,
  ValidationResult,
} from '@/types';
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
  validateConfig: (type: 'server' | 'llm' | 'agentOS' | 'runtime') => Promise<ValidationResult>;
  exportConfig: () => string;
  importConfig: (json: string) => void;
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

      validateConfig: async (type) => {
        const { server, llm, agentOS, runtime } = get();
        let config: ServerConfig | LLMConfig | AgentOSConfig | RuntimeConfig;

        switch (type) {
          case 'server':
            config = server;
            break;
          case 'llm':
            config = llm;
            break;
          case 'agentOS':
            config = agentOS;
            break;
          case 'runtime':
            config = runtime;
            break;
        }

        try {
          const result = await settingsApi.validateConfig({ type, config });
          return result;
        } catch (error) {
          return {
            valid: false,
            errors: [{ field: 'general', message: (error as Error).message }],
          };
        }
      },

      exportConfig: () => {
        const { server, llm, agentOS, runtime } = get();
        return JSON.stringify({ server, llm, agentOS, runtime }, null, 2);
      },

      importConfig: (json) => {
        try {
          const config = JSON.parse(json);
          set({
            server: config.server || DEFAULT_SERVER_CONFIG,
            llm: config.llm || DEFAULT_LLM_CONFIG,
            agentOS: config.agentOS || DEFAULT_AGENT_OS_CONFIG,
            runtime: config.runtime || DEFAULT_RUNTIME_CONFIG,
            error: null,
          });
        } catch (error) {
          set({ error: 'Invalid configuration format' });
        }
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
