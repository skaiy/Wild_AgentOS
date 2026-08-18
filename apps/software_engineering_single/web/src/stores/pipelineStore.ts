import { create } from 'zustand';
import type { PipelineResult, StageInstanceMeta, TaskMeta, WSEvent, StageDetail } from '@/types';
import { pipelineApi } from '@/api';

interface PipelineState {
  currentPipeline: PipelineResult | null;
  stages: StageInstanceMeta[];
  currentStage: StageDetail | null;
  taskMeta: TaskMeta | null;
  loading: boolean;
  error: string | null;

  startPipeline: (projectId: string, pipelineName: string) => Promise<void>;
  fetchPipelineStatus: (taskId: string) => Promise<void>;
  fetchStageDetail: (taskId: string, stageId: string) => Promise<void>;
  retryTask: (taskId: string) => Promise<void>;
  rollbackTask: (taskId: string) => Promise<void>;
  updateStageFromWS: (event: WSEvent) => void;
  clearPipeline: () => void;
  clearError: () => void;
}

export const usePipelineStore = create<PipelineState>((set, get) => ({
  currentPipeline: null,
  stages: [],
  currentStage: null,
  taskMeta: null,
  loading: false,
  error: null,

  startPipeline: async (projectId: string, pipelineName: string) => {
    set({ loading: true, error: null });
    try {
      const result = await pipelineApi.start({
        project_name: pipelineName,
        user_requirement: `Pipeline for project ${projectId}`,
      });
      const taskMeta = await pipelineApi.getTask(result.taskId);
      set({ taskMeta, loading: false });
    } catch (error) {
      set({ error: (error as Error).message, loading: false });
    }
  },

  fetchPipelineStatus: async (taskId: string) => {
    set({ loading: true, error: null });
    try {
      const [taskMeta, stages] = await Promise.all([
        pipelineApi.getTask(taskId),
        pipelineApi.getStages(taskId),
      ]);
      set({ taskMeta, stages, loading: false });
    } catch (error) {
      set({ error: (error as Error).message, loading: false });
    }
  },

  fetchStageDetail: async (taskId: string, stageId: string) => {
    set({ loading: true, error: null });
    try {
      const stage = await pipelineApi.getStage(taskId, stageId);
      set({ currentStage: stage, loading: false });
    } catch (error) {
      set({ error: (error as Error).message, loading: false });
    }
  },

  retryTask: async (taskId: string) => {
    set({ loading: true, error: null });
    try {
      await pipelineApi.retryTask(taskId);
      await get().fetchPipelineStatus(taskId);
    } catch (error) {
      set({ error: (error as Error).message, loading: false });
    }
  },

  rollbackTask: async (taskId: string) => {
    set({ loading: true, error: null });
    try {
      await pipelineApi.rollbackTask(taskId);
      await get().fetchPipelineStatus(taskId);
    } catch (error) {
      set({ error: (error as Error).message, loading: false });
    }
  },

  updateStageFromWS: (event: WSEvent) => {
    const { stages } = get();
    const payload = event.payload as Record<string, unknown>;

    switch (event.type) {
      case 'stage_started': {
        const stageId = payload.stage_id as string;
        set({
          stages: stages.map((s) =>
            s.stageId === stageId ? { ...s, status: 'running' as const, startedAt: new Date().toISOString() } : s
          ),
        });
        break;
      }
      case 'stage_completed': {
        const stageId = payload.stage_id as string;
        const status = payload.status as string;
        const durationMs = payload.duration_ms as number;
        const iri = payload.iri as string | undefined;
        set({
          stages: stages.map((s) =>
            s.stageId === stageId
              ? { ...s, status: status as 'success' | 'failed', durationMs, iri, completedAt: new Date().toISOString() }
              : s
          ),
        });
        break;
      }
      case 'stage_failed': {
        const stageId = payload.stage_id as string;
        set({
          stages: stages.map((s) =>
            s.stageId === stageId ? { ...s, status: 'failed' as const, completedAt: new Date().toISOString() } : s
          ),
        });
        break;
      }
      case 'stage_human_review_required': {
        const stageId = payload.stage_id as string;
        set({
          stages: stages.map((s) =>
            s.stageId === stageId ? { ...s, status: 'reviewing' as const } : s
          ),
        });
        break;
      }
    }
  },

  clearPipeline: () => set({
    currentPipeline: null,
    stages: [],
    currentStage: null,
    taskMeta: null,
  }),

  clearError: () => set({ error: null }),
}));