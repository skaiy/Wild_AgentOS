package types

type MetaStore interface {
	CreateProject(meta *ProjectMeta) error
	GetProject(projectID string) (*ProjectMeta, error)
	ListProjects(filter map[string]interface{}) ([]*ProjectMeta, error)
	UpdateProjectStatus(projectID string, status ProjectStatus) error
	UpdateProject(projectID string, name, description string) error
	DeleteProject(projectID string) error

	CreateTask(meta *TaskMeta) error
	GetTask(taskID string) (*TaskMeta, error)
	ListTasks(projectID string) ([]*TaskMeta, error)
	ListAllTasks() ([]*TaskMeta, error)
	UpdateTaskStatus(taskID string, status TaskStatus, currentStage string) error
	UpdateTaskWorkflow(taskID string, workflowID, runID string) error

	SaveStageInstance(taskID string, meta *StageInstanceMeta) error
	UpdateStageInstanceStatus(taskID, stageID string, status StageInstanceStatus) error
	GetStageInstance(taskID, stageID string) (*StageInstanceMeta, error)
	ListStageInstances(taskID string) ([]*StageInstanceMeta, error)
	ListAllStageInstances(offset, limit int) ([]*StageInstanceMeta, error)

	SearchTasksByStatus(status TaskStatus) ([]*TaskMeta, error)
	GetWorkflowSnapshot(projectID, taskID string) (*WorkflowSnapshot, error)
}