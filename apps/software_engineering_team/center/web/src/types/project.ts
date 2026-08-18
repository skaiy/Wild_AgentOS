export interface ProjectMeta {
  projectId: string;
  projectName: string;
  description: string;
  status: string;
  createdAt: string;
  updatedAt: string;
}

export interface CreateProjectInput {
  name: string;
  description: string;
}

export interface ProjectDetail extends ProjectMeta {
  taskCount: number;
  lastTaskAt?: string;
}

export type TaskStatus = 'pending' | 'running' | 'completed' | 'failed' | 'paused';

export interface TaskMeta {
  taskId: string;
  projectId: string;
  workflowId: string;
  status: TaskStatus;
  pipelineName: string;
  currentStage: string;
  error?: string;
  startedAt?: string;
  completedAt?: string;
}

export type StageType =
  | 'requirement'
  | 'design'
  | 'coding'
  | 'testing'
  | 'review'
  | 'cicd'
  | 'deploy';

export type StageStatus =
  | 'pending'
  | 'running'
  | 'success'
  | 'failed'
  | 'reviewing'
  | 'skipped';

export type FailurePolicy = 'fail' | 'retry' | 'skip' | 'rollback';

export interface StageInstanceMeta {
  stageId: string;
  stageType: StageType;
  name: string;
  status: StageStatus;
  order: number;
  startedAt?: string;
  completedAt?: string;
  durationMs?: number;
  retryCount: number;
  error?: string;
}

export interface StageDetail extends StageInstanceMeta {
  summary?: string;
  output?: Record<string, unknown>;
  artifacts?: Artifact[];
  errors?: StageError[];
  contractSchema?: string;
  timeoutSeconds: number;
  onFailure: FailurePolicy;
}

export interface Artifact {
  name: string;
  type: string;
  path: string;
  size?: number;
}

export interface StageError {
  code: string;
  message: string;
  timestamp: string;
}