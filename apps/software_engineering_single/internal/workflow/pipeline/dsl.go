package pipeline

type SDLCDSL struct {
	Version     string        `json:"version" yaml:"version"`
	Pipeline    PipelineBlock `json:"pipeline" yaml:"pipeline"`
}

type PipelineBlock struct {
	Name        string         `json:"name" yaml:"name"`
	Description string         `json:"description,omitempty" yaml:"description,omitempty"`
	Stages      []StageBlock   `json:"stages" yaml:"stages"`
	Options     *OptionsBlock  `json:"options,omitempty" yaml:"options,omitempty"`
}

type StageBlock struct {
	ID             string            `json:"id" yaml:"id"`
	Type           string            `json:"type" yaml:"type"`
	Name           string            `json:"name,omitempty" yaml:"name,omitempty"`
	Config         map[string]interface{} `json:"config,omitempty" yaml:"config,omitempty"`
	Timeout        string            `json:"timeout,omitempty" yaml:"timeout,omitempty"`
	MaxIterations  int               `json:"max_iterations,omitempty" yaml:"max_iterations,omitempty"`
	AIReview       bool              `json:"ai_review" yaml:"ai_review"`
	HumanReview    bool              `json:"human_review" yaml:"human_review"`
	ContractSchema string            `json:"contract_schema,omitempty" yaml:"contract_schema,omitempty"`
	OnFailure      string            `json:"on_failure,omitempty" yaml:"on_failure,omitempty"`
	RollbackTo     string            `json:"rollback_to,omitempty" yaml:"rollback_to,omitempty"`
}

type OptionsBlock struct {
	Parallelism int    `json:"parallelism,omitempty" yaml:"parallelism,omitempty"`
	LogLevel    string `json:"log_level,omitempty" yaml:"log_level,omitempty"`
}

func (d *SDLCDSL) Validate() error {
	if d.Version == "" {
		return nil
	}
	if d.Pipeline.Name == "" {
		return nil
	}
	return nil
}

func (d *SDLCDSL) ToPipelineConfig() (*PipelineBlock, error) {
	return &d.Pipeline, nil
}