package types

import "time"

type StageType string

const (
	StageRequirement StageType = "requirement"
	StageDesign      StageType = "design"
	StageCoding      StageType = "coding"
	StageTesting     StageType = "testing"
	StageReview      StageType = "review"
	StageCICD        StageType = "cicd"
	StageDeploy      StageType = "deploy"
)

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

type FailurePolicy struct {
	Policy     string `json:"policy" yaml:"policy"`
	MaxRetries int    `json:"max_retries,omitempty" yaml:"max_retries,omitempty"`
}

type StageConfig struct {
	ID             string        `json:"id" yaml:"id"`
	Name           string        `json:"name" yaml:"name"`
	StageType      StageType     `json:"stage_type" yaml:"stage_type"`
	TimeoutSeconds int64         `json:"timeout_seconds" yaml:"timeout_seconds"`
	MaxIterations  int           `json:"max_iterations,omitempty" yaml:"max_iterations,omitempty"`
	HasAIReview    bool          `json:"has_ai_review" yaml:"has_ai_review"`
	HasHumanReview bool          `json:"has_human_review" yaml:"has_human_review"`
	ContractSchema string        `json:"contract_schema,omitempty" yaml:"contract_schema,omitempty"`
	OnFailure      FailurePolicy `json:"on_failure" yaml:"on_failure"`
	RollbackTo     string        `json:"rollback_to,omitempty" yaml:"rollback_to,omitempty"`
}

type StageInput struct {
	StageID          string
	StageType        StageType
	ProjectDir       string
	UserRequirement  string
	PrevStageOutputs map[string]interface{}
}

type StageResult struct {
	StageID    string                 `json:"stage_id"`
	Status     string                 `json:"status"`
	Summary    string                 `json:"summary"`
	Output     map[string]interface{} `json:"output,omitempty"`
	OutputIRI  string                 `json:"output_iri,omitempty"`
	Artifacts  []string               `json:"artifacts,omitempty"`
	Errors     []string               `json:"errors,omitempty"`
	DurationMs int64                  `json:"duration_ms"`
}

type HumanReviewSignal struct {
	StageID  string   `json:"stage_id"`
	Approved bool     `json:"approved"`
	Comments []string `json:"comments"`
}

type ReviewResult struct {
	Approved bool     `json:"approved"`
	Score    int      `json:"score"`
	Comments []string `json:"comments"`
	Reviewer string   `json:"reviewer"`
}

type PipelineInput struct {
	ProjectName     string         `json:"project_name"`
	ProjectDir      string         `json:"project_dir"`
	UserRequirement string         `json:"user_requirement"`
	ConfigOverride  PipelineConfig `json:"config_override,omitempty"`
}

type PipelineConfig struct {
	ProjectName string        `json:"project_name,omitempty" yaml:"project_name,omitempty"`
	Description string        `json:"description,omitempty" yaml:"description,omitempty"`
	Stages      []StageConfig `json:"stages" yaml:"stages"`
}

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
	StageID           string               `json:"stage_id" db:"stage_id"`
	TaskID            string               `json:"task_id" db:"task_id"`
	StageType         StageType            `json:"stage_type" db:"stage_type"`
	Name              string               `json:"name" db:"name"`
	Status            StageInstanceStatus  `json:"status" db:"status"`
	Order             int                  `json:"order" db:"order_idx"`
	RetryCount        int                  `json:"retry_count" db:"retry_count"`
	DurationMs        int64                `json:"duration_ms,omitempty" db:"duration_ms"`
	ContractValid     *bool                `json:"contract_valid,omitempty" db:"contract_valid"`
	AiReviewPassed    *bool                `json:"ai_review_passed,omitempty" db:"ai_review_passed"`
	HumanReviewPassed *bool                `json:"human_review_passed,omitempty" db:"human_review_passed"`
	OutputIRI         string               `json:"output_iri,omitempty" db:"output_iri"`
	Error             string               `json:"error,omitempty" db:"error"`
	StartedAt         time.Time            `json:"started_at" db:"started_at"`
	CompletedAt       *time.Time           `json:"completed_at,omitempty" db:"completed_at"`
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

func (s StageType) String() string {
	return string(s)
}