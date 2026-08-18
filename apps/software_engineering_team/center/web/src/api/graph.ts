import { api } from './client';
import type { GraphData } from '@/types';

export const graphApi = {
  getProjectGraph: (projectId: string) => api.get<GraphData>(`projects/${projectId}/graph`),
};