export interface AgentOSStatus {
  running: boolean;
  version: string;
  grpcConnected: boolean;
  uptime: number;
  taskCount: number;
}

export interface TemporalStatus {
  connected: boolean;
  namespace: string;
  workerCount: number;
  taskQueue: string;
  pendingWorkflows: number;
}

export interface ResourceUsage {
  cpuPercent: number;
  memoryUsedMB: number;
  memoryTotalMB: number;
  diskUsedGB: number;
  diskTotalGB: number;
}

export interface ActiveTask {
  taskId: string;
  projectId: string;
  pipeline: string;
  status: string;
  stage: string;
  startedAt: string;
  workflowId: string;
}

export interface SystemStatus {
  agentOs: AgentOSStatus;
  temporal: TemporalStatus;
}

export interface HealthCheckResult {
  agentOS: {
    healthy: boolean;
    message: string;
  };
  temporal: {
    healthy: boolean;
    message: string;
  };
  llm: {
    healthy: boolean;
    message: string;
  };
  overall: boolean;
}