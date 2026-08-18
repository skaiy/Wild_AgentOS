import type { StageInstanceMeta } from './project';

export interface PipelineResult {
  taskId: string;
  projectId: string;
  workflowId: string;
  status: string;
  stages: StageInstanceMeta[];
  summary?: string;
  totalDurationMs?: number;
}

export interface PipelineInput {
  project_name: string;
  project_dir?: string;
  user_requirement?: string;
  stages?: PipelineStageInput[];
}

export interface PipelineStageInput {
  id: string;
  name: string;
  type: string;
  timeout: string;
  ai_review?: boolean;
  human_review?: boolean;
}