export interface ProjectMeta {
  projectId: string;
  projectName: string;
  description: string;
  status: string;
  createdAt: string;
  updatedAt: string;
}

export interface CreateProjectInput {
  name: string;
  description: string;
}

export interface ProjectDetail extends ProjectMeta {
  taskCount: number;
  lastTaskAt?: string;
}