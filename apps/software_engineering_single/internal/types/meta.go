package types

import "time"

type ProjectStatus string

const (
	ProjectStatusInit      ProjectStatus = "initialized"
	ProjectStatusRunning   ProjectStatus = "running"
	ProjectStatusCompleted ProjectStatus = "completed"
	ProjectStatusFailed    ProjectStatus = "failed"
	ProjectStatusArchived  ProjectStatus = "archived"
)

type TaskStatus string

const (
	TaskStatusPending    TaskStatus = "pending"
	TaskStatusRunning    TaskStatus = "running"
	TaskStatusPaused     TaskStatus = "paused"
	TaskStatusCompleted  TaskStatus = "completed"
	TaskStatusFailed     TaskStatus = "failed"
	TaskStatusRolledBack TaskStatus = "rolled_back"
)

type StageInstanceStatus string

const (
	StageStatusPending      StageInstanceStatus = "pending"
	StageStatusRunning      StageInstanceStatus = "running"
	StageStatusAiReview     StageInstanceStatus = "ai_review"
	StageStatusHumanReview  StageInstanceStatus = "human_review"
	StageStatusCompleted    StageInstanceStatus = "completed"
	StageStatusFailed       StageInstanceStatus = "failed"
	StageStatusSkipped      StageInstanceStatus = "skipped"
	StageStatusRolledBack   StageInstanceStatus = "rolled_back"
)

type ProjectMeta struct {
	ProjectID   string                 `json:"project_id" db:"project_id"`
	ProjectName string                 `json:"project_name" db:"project_name"`
	Description string                 `json:"description,omitempty" db:"description"`
	Status      ProjectStatus          `json:"status" db:"status"`
	Tags        []string               `json:"tags,omitempty" db:"-"`
	Extras      map[string]interface{} `json:"extras,omitempty" db:"-"`
	CreatedAt   time.Time              `json:"created_at" db:"created_at"`
	UpdatedAt   time.Time              `json:"updated_at" db:"updated_at"`
}

type TaskMeta struct {
	TaskID       string                 `json:"task_id" db:"task_id"`
	ProjectID    string                 `json:"project_id" db:"project_id"`
	PipelineName string                 `json:"pipeline_name" db:"pipeline_name"`
	Status       TaskStatus             `json:"status" db:"status"`
	CurrentStage string                 `json:"current_stage" db:"current_stage"`
	WorkflowID   string                 `json:"workflow_id" db:"workflow_id"`
	RunID        string                 `json:"run_id,omitempty" db:"run_id"`
	Stages       []StageInstanceMeta    `json:"stages" db:"-"`
	Error        string                 `json:"error,omitempty" db:"error"`
	StartedAt    time.Time              `json:"started_at" db:"started_at"`
	CompletedAt  *time.Time             `json:"completed_at,omitempty" db:"completed_at"`
	Extras       map[string]interface{} `json:"extras,omitempty" db:"-"`
}

type StageInstanceMeta struct {
	StageID          string               `json:"stage_id" db:"stage_id"`
	StageType        StageType            `json:"stage_type" db:"stage_type"`
	Name             string               `json:"name" db:"name"`
	Status           StageInstanceStatus  `json:"status" db:"status"`
	Order            int                  `json:"order" db:"order_idx"`
	RetryCount       int                  `json:"retry_count" db:"retry_count"`
	DurationMs       int64                `json:"duration_ms,omitempty" db:"duration_ms"`
	ContractValid    *bool                `json:"contract_valid,omitempty" db:"contract_valid"`
	AiReviewPassed   *bool                `json:"ai_review_passed,omitempty" db:"ai_review_passed"`
	HumanReviewPassed *bool               `json:"human_review_passed,omitempty" db:"human_review_passed"`
	OutputIRI        string               `json:"output_iri,omitempty" db:"output_iri"`
	Error            string               `json:"error,omitempty" db:"error"`
	StartedAt        time.Time            `json:"started_at" db:"started_at"`
	CompletedAt      *time.Time           `json:"completed_at,omitempty" db:"completed_at"`
}

type WorkflowSnapshot struct {
	TaskMeta
	Progress float64         `json:"progress"`
	Timeline []StageTimeline `json:"timeline"`
}

type StageTimeline struct {
	StageID    string    `json:"stage_id"`
	Name       string    `json:"name"`
	Status     string    `json:"status"`
	StartedAt  time.Time `json:"started_at"`
	DurationMs int64     `json:"duration_ms"`
}

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
	ListAllStageInstances() ([]*StageInstanceMeta, error)

	SearchTasksByStatus(status TaskStatus) ([]*TaskMeta, error)
	GetWorkflowSnapshot(projectID, taskID string) (*WorkflowSnapshot, error)
}