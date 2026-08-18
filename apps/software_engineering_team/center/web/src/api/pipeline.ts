import { api } from './client';
import type { PipelineResult, PipelineInput, StageInstanceMeta, StageDetail } from '@/types';

export interface StartPipelineResponse {
  projectId: string;
  taskId: string;
  workflowId: string;
  status: string;
}

export const pipelineApi = {
  start: (input: PipelineInput) =>
    api.post<StartPipelineResponse>('pipelines', input),

  get: (id: string) => api.get<PipelineResult>(`pipelines/${id}`),

  getTask: (taskId: string) => api.get(`tasks/${taskId}`),

  retryTask: (taskId: string) => api.post(`tasks/${taskId}/retry`),

  rollbackTask: (taskId: string) => api.post(`tasks/${taskId}/rollback`),

  getStages: async (taskId: string) => {
    const result = await api.get<{ stages: StageInstanceMeta[] }>(`tasks/${taskId}/stages`);
    return result.stages || [];
  },

  getStage: (taskId: string, stageId: string) =>
    api.get<StageDetail>(`tasks/${taskId}/stages/${stageId}`),
};