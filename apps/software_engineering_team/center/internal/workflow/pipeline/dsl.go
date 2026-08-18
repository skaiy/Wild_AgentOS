package pipeline

import "fmt"

// SDCDSL — 核心 DSL 定义，由 SDLCDSLWorkflow 接收
type SDCDSL struct {
	Version string        `json:"version" yaml:"version"`
	Name    string        `json:"name" yaml:"name"`
	Stages  []StageBlock  `json:"stages" yaml:"stages"`
	State   WorkflowState `json:"state" yaml:"state"`
}

// WorkflowState — 工作流运行时快照，在 DSL 中透传用于断点续传
type WorkflowState struct {
	CurrentStageIdx int                    `json:"current_stage_idx" yaml:"current_stage_idx"`
	PrevOutputs     map[string]StageResult `json:"prev_outputs" yaml:"prev_outputs"`
}

// StageBlock — DSL 阶段定义
type StageBlock struct {
	ID             string `json:"id" yaml:"id"`
	StageType      string `json:"stage_type" yaml:"stage_type"`
	Name           string `json:"name,omitempty" yaml:"name,omitempty"`
	AIReview       bool   `json:"ai_review" yaml:"ai_review"`
	HumanReview    bool   `json:"human_review" yaml:"human_review"`
	ContractSchema string `json:"contract_schema,omitempty" yaml:"contract_schema,omitempty"`
	Timeout        string `json:"timeout,omitempty" yaml:"timeout,omitempty"`
}

// StageResult — 阶段执行结果，存储在 WorkflowState.PrevOutputs 中
type StageResult struct {
	StageID string `json:"stage_id"`
	Output  string `json:"output"`
	Status  string `json:"status"`
}

// Validate 校验 DSL 定义完整性
func (d *SDCDSL) Validate() error {
	if d.Name == "" {
		return fmt.Errorf("DSL name is required")
	}
	if len(d.Stages) == 0 {
		return fmt.Errorf("DSL stages list is empty")
	}
	for i, s := range d.Stages {
		if s.ID == "" {
			return fmt.Errorf("stage %d: id is required", i)
		}
		if s.StageType == "" {
			return fmt.Errorf("stage %d: stage_type is required", i)
		}
	}
	return nil
}