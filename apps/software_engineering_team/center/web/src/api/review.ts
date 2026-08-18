import { api } from './client';
import type { PendingReview, ReviewDetail, HumanReviewRequest, ReviewRecord } from '@/types';

export interface SubmitReviewPayload {
  workflow_id: string;
  task_id: string;
  stage_id: string;
  approved: boolean;
  comments: string[];
  reviewer: string;
}

export const reviewApi = {
  getPending: () => api.get<{ reviews: PendingReview[] }>('reviews/pending'),

  getDetail: async (stageId: string) => {
    const result = await api.get<{ history: ReviewRecord[] }>(`reviews/${stageId}/history`);
    return { stageId, history: result.history } as ReviewDetail;
  },

  submit: (stageId: string, request: HumanReviewRequest, context: { taskId: string; workflowId: string }) =>
    api.post<{ status: string; approved: boolean }>(`reviews/${stageId}/submit`, {
      workflow_id: context.workflowId,
      task_id: context.taskId,
      stage_id: stageId,
      approved: request.approved,
      comments: request.comments,
      reviewer: request.reviewer,
    } as SubmitReviewPayload),

  getHistory: (stageId: string) =>
    api.get<{ reviews: ReviewRecord[] }>(`reviews/${stageId}/history`),
};