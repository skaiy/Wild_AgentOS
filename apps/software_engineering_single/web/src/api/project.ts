import { api, unwrapContainer } from './client';
import type { ProjectMeta, ProjectDetail, CreateProjectInput } from '@/types';

export const projectApi = {
  list: async () => {
    const result = await api.get<{ projects: ProjectMeta[] }>('projects');
    return unwrapContainer<ProjectMeta[]>(result);
  },

  get: (id: string) => api.get<ProjectDetail>(`projects/${id}`),

  create: (input: CreateProjectInput) =>
    api.post<ProjectMeta>('projects', {
      project_name: input.name,
      description: input.description,
    }),

  delete: (id: string) => api.delete(`projects/${id}`),

  getTasks: async (id: string) => {
    const result = await api.get<{ tasks: unknown[] }>(`projects/${id}/tasks`);
    return unwrapContainer<unknown[]>(result);
  },

  createTask: (id: string, input: { pipeline_name: string }) =>
    api.post(`projects/${id}/tasks`, input),

  getGraph: (id: string) => api.get(`projects/${id}/graph`),

  getSnapshot: (id: string) => api.get(`projects/${id}/snapshot`),
};