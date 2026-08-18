import { api } from './client';
import type { LogEntry, LogFilter } from '@/types';

export const logsApi = {
  getSystemLogs: async (filter?: LogFilter) => {
    const params: Record<string, string> = {};
    if (filter?.level) params.level = filter.level;
    if (filter?.keyword) params.keyword = filter.keyword;
    const result = await api.get<{ logs: LogEntry[] }>('logs/system', params);
    return result.logs || [];
  },

  getStageLogs: async (taskId: string, stageId: string) => {
    const result = await api.get<{ logs: LogEntry[] }>(`logs/stage/${taskId}/${stageId}`);
    return result.logs || [];
  },

  getAgentOSLogs: async (since?: string) => {
    const params = since ? { since } : undefined;
    const result = await api.get<{ logs: LogEntry[] }>('logs/agent-os', params);
    return result.logs || [];
  },
};