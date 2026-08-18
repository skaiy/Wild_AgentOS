export type WSEventType =
  | 'pipeline_started'
  | 'stage_started'
  | 'stage_completed'
  | 'stage_failed'
  | 'stage_ai_review'
  | 'stage_human_review_required'
  | 'pipeline_completed'
  | 'stage_progress'
  | 'agent_os_event';

export interface WSEvent {
  type: WSEventType;
  projectId: string;
  payload: Record<string, unknown>;
  timestamp: number;
}

export interface PipelineStartedPayload {
  project_id: string;
  task_id: string;
  workflow_id: string;
}

export interface StageStartedPayload {
  stage_id: string;
  stage_type: string;
  name: string;
}

export interface StageCompletedPayload {
  stage_id: string;
  status: string;
  iri?: string;
  duration_ms: number;
}

export interface StageFailedPayload {
  stage_id: string;
  errors: Array<{ code: string; message: string }>;
}

export interface StageProgressPayload {
  stage_id: string;
  progress: number;
  message: string;
}

export interface PipelineCompletedPayload {
  status: string;
  summary?: string;
  total_duration_ms: number;
}