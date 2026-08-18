import { api } from './client';

export const monitorApi = {
  getSystemStatus: () => api.get('system/status'),

  getHealth: () => api.get('system/health'),

  getResources: () => api.get('system/resources'),

  getActiveTasks: async () => {
    const result = await api.get<{ activeTasks: unknown[] }>('system/active-tasks');
    return result.activeTasks || [];
  },
};