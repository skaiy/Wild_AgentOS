import { api } from './client';
import type { SystemStatus, HealthCheckResult, ResourceUsage, ActiveTask } from '@/types';

export const monitorApi = {
  getSystemStatus: () => api.get<SystemStatus>('system/status'),

  getHealth: () => api.get<HealthCheckResult>('system/health'),

  getResources: () => api.get<ResourceUsage>('system/resources'),

  getActiveTasks: async () => {
    const result = await api.get<{ activeTasks: ActiveTask[] }>('system/active-tasks');
    return result.activeTasks || [];
  },
};
