import { api, unwrapContainer } from './client';
import type { TaskMeta, PipelineResult, PipelineInput, StageInstanceMeta, StageDetail } from '@/types';

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

  getTask: (taskId: string) => api.get<TaskMeta>(`tasks/${taskId}`),

  retryTask: (taskId: string) => api.post(`tasks/${taskId}/retry`),

  rollbackTask: (taskId: string) => api.post(`tasks/${taskId}/rollback`),

  getStages: async (taskId: string) => {
    const result = await api.get<{ stages: StageInstanceMeta[] }>(`tasks/${taskId}/stages`);
    return unwrapContainer<StageInstanceMeta[]>(result);
  },

  getStage: (taskId: string, stageId: string) =>
    api.get<StageDetail>(`tasks/${taskId}/stages/${stageId}`),
};