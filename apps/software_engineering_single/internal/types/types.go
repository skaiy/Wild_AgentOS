package types

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

func (s StageType) String() string {
	return string(s)
}