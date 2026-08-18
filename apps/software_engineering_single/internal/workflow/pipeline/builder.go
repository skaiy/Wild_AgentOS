package pipeline

import (
	"fmt"

	"github.com/agent-os/se-app/internal/executor"
	"github.com/agent-os/se-app/internal/types"
)

type StageDefinition struct {
	ID             string
	Name           string
	Type           types.StageType
	Config         map[string]interface{}
	TimeoutSeconds int64
	MaxIterations  int
	HasAIReview    bool
	HasHumanReview bool
	ContractSchema string
	OnFailure      types.FailurePolicy
	RollbackTo     string
}

type PipelineDefinition struct {
	Name        string
	Version     string
	Description string
	Stages      []StageDefinition
	Executors   map[string]executor.StageExecutor
}

type PipelineBuilder struct {
	factory *executor.StageFactory
}

func NewPipelineBuilder(factory *executor.StageFactory) *PipelineBuilder {
	return &PipelineBuilder{factory: factory}
}

func (pb *PipelineBuilder) BuildFromConfig(config types.PipelineConfig) (*PipelineDefinition, error) {
	if len(config.Stages) == 0 {
		return nil, fmt.Errorf("pipeline config has no stages")
	}

	definition := &PipelineDefinition{
		Name:        config.ProjectName,
		Description: config.Description,
		Stages:      make([]StageDefinition, 0, len(config.Stages)),
		Executors:   make(map[string]executor.StageExecutor),
	}

	for _, sc := range config.Stages {
		stageDef := StageDefinition{
			ID:             sc.ID,
			Name:           sc.Name,
			Type:           sc.StageType,
			Config:         make(map[string]interface{}),
			TimeoutSeconds: sc.TimeoutSeconds,
			MaxIterations:  sc.MaxIterations,
			HasAIReview:    sc.HasAIReview,
			HasHumanReview: sc.HasHumanReview,
			ContractSchema: sc.ContractSchema,
			OnFailure:      sc.OnFailure,
			RollbackTo:     sc.RollbackTo,
		}

		exec, err := pb.factory.Create(string(sc.StageType), stageDef.Config)
		if err != nil {
			return nil, fmt.Errorf("failed to create executor for stage %q (type %q): %w", sc.ID, sc.StageType, err)
		}

		definition.Stages = append(definition.Stages, stageDef)
		definition.Executors[sc.ID] = exec
	}

	return definition, nil
}