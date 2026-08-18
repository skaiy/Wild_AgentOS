export interface PendingReview {
  stageId: string;
  taskId: string;
  projectId: string;
  projectName: string;
  stageName: string;
  stageType: string;
  startedAt: string;
  workflowId?: string;
  summary?: string;
}

export interface ReviewRecord {
  id: string;
  stageId: string;
  reviewer?: string;
  approved: boolean;
  comment?: string;
  createdAt: string;
}

export interface HumanReviewRequest {
  approved: boolean;
  comments: string[];
  reviewer: string;
}

export interface ReviewDetail extends PendingReview {
  output?: Record<string, unknown>;
  artifacts?: Array<{
    name: string;
    type: string;
    content?: string;
    path: string;
  }>;
  history: ReviewRecord[];
}