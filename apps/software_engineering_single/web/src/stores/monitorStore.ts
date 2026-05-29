import { create } from 'zustand';
import type {
  AgentOSStatus,
  TemporalStatus,
  ResourceUsage,
  ActiveTask,
  HealthCheckResult,
} from '@/types';
import { monitorApi } from '@/api';

interface MonitorState {
  agentOSStatus: AgentOSStatus | null;
  temporalStatus: TemporalStatus | null;
  resourceUsage: ResourceUsage | null;
  activeTasks: ActiveTask[];
  loading: boolean;
  error: string | null;

  fetchAgentOSStatus: () => Promise<void>;
  fetchTemporalStatus: () => Promise<void>;
  fetchResourceUsage: () => Promise<void>;
  fetchActiveTasks: () => Promise<void>;
  fetchAll: () => Promise<void>;
  healthCheck: () => Promise<HealthCheckResult>;
  clearError: () => void;
}

function transformSystemStatus(data: Record<string, unknown>): { agentOs: AgentOSStatus; temporal: TemporalStatus } {
  const agentOsRaw = (data.agentOs || {}) as Record<string, unknown>;
  const temporalRaw = (data.temporal || {}) as Record<string, unknown>;
  const grpcRaw = (data.grpc || {}) as Record<string, unknown>;

  return {
    agentOs: {
      running: agentOsRaw.status === 'running',
      version: (agentOsRaw.version as string) || '2.0.0',
      grpcConnected: grpcRaw.status === 'connected',
      uptime: 0,
      taskCount: 0,
    },
    temporal: {
      connected: temporalRaw.status === 'connected',
      namespace: temporalRaw.host as string || 'default',
      workerCount: 0,
      taskQueue: temporalRaw.queue as string || 'default',
      pendingWorkflows: 0,
    },
  };
}

function transformResources(data: Record<string, unknown>): ResourceUsage {
  const cpu = (data.cpu || {}) as Record<string, unknown>;
  const memory = (data.memory || {}) as Record<string, unknown>;
  const disk = (data.disk || {}) as Record<string, unknown>;

  const totalBytes = (memory.totalBytes as number) || 1;
  const availableBytes = (memory.availableBytes as number) || 0;
  const usedBytes = totalBytes - availableBytes;

  const diskTotalBytes = (disk.totalBytes as number) || 1;
  const diskAvailableBytes = (disk.availableBytes as number) || 0;
  const diskUsedBytes = diskTotalBytes - diskAvailableBytes;

  return {
    cpuPercent: Math.round((cpu.usedPercent as number) || 0),
    memoryUsedMB: Math.round(usedBytes / 1024 / 1024),
    memoryTotalMB: Math.round(totalBytes / 1024 / 1024),
    diskUsedGB: Math.round(diskUsedBytes / 1024 / 1024 / 1024 * 100) / 100,
    diskTotalGB: Math.round(diskTotalBytes / 1024 / 1024 / 1024 * 100) / 100,
  };
}

function transformHealthCheck(data: Record<string, unknown>): HealthCheckResult {
  const checks = (data.checks || {}) as Record<string, unknown>;
  return {
    agentOS: { healthy: (checks.grpc as boolean) || false, message: '' },
    temporal: { healthy: (checks.temporal as boolean) || false, message: '' },
    llm: { healthy: false, message: 'LLM health check not available' },
    overall: data.status === 'healthy',
  };
}

export const useMonitorStore = create<MonitorState>((set, get) => ({
  agentOSStatus: null,
  temporalStatus: null,
  resourceUsage: null,
  activeTasks: [],
  loading: false,
  error: null,

  fetchAgentOSStatus: async () => {
    try {
      const status = await monitorApi.getSystemStatus();
      const transformed = transformSystemStatus(status as unknown as Record<string, unknown>);
      set({ agentOSStatus: transformed.agentOs, temporalStatus: transformed.temporal });
    } catch (error) {
      set({ error: (error as Error).message });
    }
  },

  fetchTemporalStatus: async () => {
    try {
      const status = await monitorApi.getSystemStatus();
      const transformed = transformSystemStatus(status as unknown as Record<string, unknown>);
      set({ temporalStatus: transformed.temporal, agentOSStatus: transformed.agentOs });
    } catch (error) {
      set({ error: (error as Error).message });
    }
  },

  fetchResourceUsage: async () => {
    try {
      const usage = await monitorApi.getResources();
      set({ resourceUsage: transformResources(usage as unknown as Record<string, unknown>) });
    } catch (error) {
      set({ error: (error as Error).message });
    }
  },

  fetchActiveTasks: async () => {
    try {
      const tasks = await monitorApi.getActiveTasks();
      set({ activeTasks: tasks });
    } catch (error) {
      set({ error: (error as Error).message });
    }
  },

  fetchAll: async () => {
    set({ loading: true, error: null });
    try {
      const [status, usage, tasks] = await Promise.all([
        monitorApi.getSystemStatus(),
        monitorApi.getResources(),
        monitorApi.getActiveTasks(),
      ]);
      const transformed = transformSystemStatus(status as unknown as Record<string, unknown>);
      set({
        agentOSStatus: transformed.agentOs,
        temporalStatus: transformed.temporal,
        resourceUsage: transformResources(usage as unknown as Record<string, unknown>),
        activeTasks: tasks,
        loading: false,
      });
    } catch (error) {
      set({ error: (error as Error).message, loading: false });
    }
  },

  healthCheck: async () => {
    try {
      const result = await monitorApi.getHealth();
      return transformHealthCheck(result as unknown as Record<string, unknown>);
    } catch (error) {
      return {
        agentOS: { healthy: false, message: (error as Error).message },
        temporal: { healthy: false, message: (error as Error).message },
        llm: { healthy: false, message: (error as Error).message },
        overall: false,
      };
    }
  },

  clearError: () => set({ error: null }),
}));