import { create } from 'zustand';
import type { ProjectMeta, CreateProjectInput } from '@/types';
import { projectApi } from '@/api';

interface ProjectState {
  projects: ProjectMeta[];
  currentProject: ProjectMeta | null;
  loading: boolean;
  error: string | null;

  fetchProjects: () => Promise<void>;
  fetchProject: (id: string) => Promise<void>;
  createProject: (input: CreateProjectInput) => Promise<ProjectMeta>;
  deleteProject: (id: string) => Promise<void>;
  setCurrentProject: (project: ProjectMeta | null) => void;
  clearError: () => void;
}

export const useProjectStore = create<ProjectState>((set, get) => ({
  projects: [],
  currentProject: null,
  loading: false,
  error: null,

  fetchProjects: async () => {
    set({ loading: true, error: null });
    try {
      const projects = await projectApi.list();
      set({ projects, loading: false });
    } catch (error) {
      set({ error: (error as Error).message, loading: false });
    }
  },

  fetchProject: async (id: string) => {
    set({ loading: true, error: null });
    try {
      const project = await projectApi.get(id);
      set({ currentProject: project as unknown as ProjectMeta, loading: false });
    } catch (error) {
      set({ error: (error as Error).message, loading: false });
    }
  },

  createProject: async (input: CreateProjectInput) => {
    set({ loading: true, error: null });
    try {
      const project = await projectApi.create(input);
      set((state) => ({
        projects: [...state.projects, project],
        loading: false,
      }));
      return project;
    } catch (error) {
      set({ error: (error as Error).message, loading: false });
      throw error;
    }
  },

  deleteProject: async (id: string) => {
    set({ loading: true, error: null });
    try {
      await projectApi.delete(id);
      set((state) => ({
        projects: state.projects.filter((p) => p.projectId !== id),
        currentProject: state.currentProject?.projectId === id ? null : state.currentProject,
        loading: false,
      }));
    } catch (error) {
      set({ error: (error as Error).message, loading: false });
    }
  },

  setCurrentProject: (project) => set({ currentProject: project }),

  clearError: () => set({ error: null }),
}));