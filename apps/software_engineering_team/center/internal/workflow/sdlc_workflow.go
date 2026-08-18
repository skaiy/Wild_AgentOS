package workflow

import (
	"encoding/json"
	"fmt"
	"time"

	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/workflow"

	"github.com/agent-os/se-center/internal/types"
	"github.com/agent-os/se-center/internal/workflow/pipeline"
)

const (
	HumanReviewSignalName = "human-review-signal"
)

type ExecuteStageParams struct {
	TaskID          string
	ProjectID       string
	StageID         string
	StageType       string
	ProjectDir      string
	UserRequirement string
	PrevOutputs     map[string]interface{}
	LLMApiKey       string
	LLMBaseURL      string
	LLMModel        string
}

type AIReviewParams struct {
	TaskID      string
	StageID     string
	StageOutput string
}

type ValidateContractParams struct {
	StageID     string
	StageOutput map[string]interface{}
	Schema      string
}

type DispatchTaskParams struct {
	TaskID       string
	WorkflowID   string
	RunID        string
	CurrentStage string
}

func SDLCDSLWorkflow(ctx workflow.Context, dsl pipeline.SDCDSL) (string, error) {
	logger := workflow.GetLogger(ctx)

	currentIdx := dsl.State.CurrentStageIdx
	prevOutputs := dsl.State.PrevOutputs
	if prevOutputs == nil {
		prevOutputs = make(map[string]pipeline.StageResult)
	}

	logger.Info("workflow started/continued", "name", dsl.Name,
		"from_stage", currentIdx, "total_stages", len(dsl.Stages))

	for i := currentIdx; i < len(dsl.Stages); i++ {
		stage := dsl.Stages[i]
		logger.Info("executing stage", "id", stage.ID, "type", stage.StageType)

		ctx1 := workflow.WithActivityOptions(ctx, workflow.ActivityOptions{
			StartToCloseTimeout: 30 * time.Minute,
			HeartbeatTimeout:    30 * time.Second,
			RetryPolicy:         &temporal.RetryPolicy{MaximumAttempts: 3},
		})

		var stageResult types.StageResult
		err := workflow.ExecuteActivity(ctx1, "ExecuteStage", ExecuteStageParams{
			StageID:     stage.ID,
			StageType:   stage.StageType,
			PrevOutputs: toInterfaceMap(prevOutputs),
		}).Get(ctx, &stageResult)

		if err != nil {
			logger.Error("stage execution failed", "stage", stage.ID, "error", err)
			dsl.State.CurrentStageIdx = i
			dsl.State.PrevOutputs = prevOutputs
			return "", err
		}

		stageOutput := marshalStageOutput(stageResult.Output)
		prevOutputs[stage.ID] = pipeline.StageResult{
			StageID: stage.ID,
			Output:  stageOutput,
			Status:  stageResult.Status,
		}

		if stage.AIReview {
			var reviewResult types.ReviewResult
			err := workflow.ExecuteActivity(ctx1, "AIReview", AIReviewParams{
				StageID:     stage.ID,
				StageOutput: stageOutput,
			}).Get(ctx, &reviewResult)

			if err != nil {
				dsl.State.CurrentStageIdx = i
				dsl.State.PrevOutputs = prevOutputs
				return "", err
			}
			if !reviewResult.Approved {
				logger.Warn("AI review rejected", "stage", stage.ID, "score", reviewResult.Score)
				return dslContinueAsNew(ctx, dsl)
			}
		}

		if stage.HumanReview {
			sig, err := waitForHumanReview(ctx, stage.ID)
			if err != nil {
				dsl.State.CurrentStageIdx = i
				dsl.State.PrevOutputs = prevOutputs
				return "", err
			}
			if !sig.Approved {
				logger.Warn("human review rejected", "stage", stage.ID)
				return dslContinueAsNew(ctx, dsl)
			}
		}

		dsl.State.CurrentStageIdx = i + 1
		dsl.State.PrevOutputs = prevOutputs
	}

	logger.Info("workflow completed successfully")
	return "completed", nil
}

func dslContinueAsNew(ctx workflow.Context, dsl pipeline.SDCDSL) (string, error) {
	return "", workflow.NewContinueAsNewError(ctx, SDLCDSLWorkflow, dsl)
}

func marshalStageOutput(output map[string]interface{}) string {
	if output == nil {
		return ""
	}
	b, err := json.Marshal(output)
	if err != nil {
		return fmt.Sprintf("%v", output)
	}
	return string(b)
}

func toInterfaceMap(m map[string]pipeline.StageResult) map[string]interface{} {
	result := make(map[string]interface{}, len(m))
	for k, v := range m {
		result[k] = v
	}
	return result
}