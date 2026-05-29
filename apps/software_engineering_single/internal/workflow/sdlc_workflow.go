package workflow

import (
	"sync"
	"time"

	"go.temporal.io/sdk/temporal"
	"go.temporal.io/sdk/workflow"

	"github.com/agent-os/se-app/internal/types"
	pipelinepkg "github.com/agent-os/se-app/internal/workflow/pipeline"
)

const HumanReviewSignalName = "human-review-signal"

type humanReviewState struct {
	mu       sync.Mutex
	received bool
	signal   *types.HumanReviewSignal
}

func SDLCDSLWorkflow(ctx workflow.Context, dsl pipelinepkg.SDLCDSL) error {
	logger := workflow.GetLogger(ctx)

	pipelineConfig, err := dsl.ToPipelineConfig()
	if err != nil {
		return err
	}

	projectDir := ""
	userRequirement := ""
	prevOutputs := make(map[string]interface{})
	llmAPIKey := ""
	llmBaseURL := ""
	llmModel := ""

	for _, stage := range pipelineConfig.Stages {
		stage := stage

		params := ExecuteStageActivityParam{
			StageID:          stage.ID,
			StageType:        stage.Type,
			ProjectDir:       projectDir,
			UserRequirement:  userRequirement,
			PrevStageOutputs: prevOutputs,
			LLMApiKey:        llmAPIKey,
			LLMBaseURL:       llmBaseURL,
			LLMModel:         llmModel,
		}

		activityCtx := workflow.WithActivityOptions(ctx, workflow.ActivityOptions{
			StartToCloseTimeout: 30 * time.Minute,
			HeartbeatTimeout:    30 * time.Second,
			RetryPolicy:         &temporal.RetryPolicy{MaximumAttempts: 3},
		})

		var stageResult types.StageResult
		err := workflow.ExecuteActivity(activityCtx, "ExecuteStageActivity", params).Get(ctx, &stageResult)
		if err != nil {
			logger.Error("stage execution failed", "stage_id", stage.ID, "error", err)
			return err
		}

		stageOutput := stageResult.Output
		if stageOutput == nil {
			stageOutput = make(map[string]interface{})
		}

		if stage.AIReview {
			reviewCtx := workflow.WithActivityOptions(ctx, workflow.ActivityOptions{
				StartToCloseTimeout: 10 * time.Minute,
				HeartbeatTimeout:    10 * time.Second,
			})

			var reviewResult types.ReviewResult
			err := workflow.ExecuteActivity(reviewCtx, "AIReviewActivity", AIReviewActivityParam{
				StageID:     stage.ID,
				StageOutput: stageOutput,
				ProjectDir:  projectDir,
			}).Get(ctx, &reviewResult)
			if err != nil {
				logger.Error("AI review failed", "stage_id", stage.ID, "error", err)
				return err
			}

			if !reviewResult.Approved {
				logger.Warn("AI review rejected", "stage_id", stage.ID)
				return workflow.NewContinueAsNewError(ctx, SDLCDSLWorkflow, dsl)
			}
		}

		if stage.HumanReview {
			state := &humanReviewState{}

			workflow.Go(ctx, func(gCtx workflow.Context) {
				sel := workflow.NewSelector(gCtx)
				ch := workflow.GetSignalChannel(gCtx, HumanReviewSignalName)
				sel.AddReceive(ch, func(c workflow.ReceiveChannel, _ bool) {
					var sig types.HumanReviewSignal
					c.Receive(gCtx, &sig)
					state.mu.Lock()
					state.signal = &sig
					state.received = true
					state.mu.Unlock()
				})
				for {
					sel.Select(gCtx)
					if state.received {
						return
					}
				}
			})

			ok, aErr := workflow.AwaitWithTimeout(ctx, 24*time.Hour, func() bool {
				state.mu.Lock()
				defer state.mu.Unlock()
				return state.received
			})
			if aErr != nil {
				return aErr
			}

			if !ok {
				logger.Warn("human review timeout", "stage_id", stage.ID)
				return workflow.NewContinueAsNewError(ctx, SDLCDSLWorkflow, dsl)
			}

			if !state.signal.Approved {
				logger.Warn("human review rejected", "stage_id", stage.ID)
				return workflow.NewContinueAsNewError(ctx, SDLCDSLWorkflow, dsl)
			}
		}

		if stage.ContractSchema != "" {
			contractCtx := workflow.WithActivityOptions(ctx, workflow.ActivityOptions{
				StartToCloseTimeout: 5 * time.Minute,
			})

			var contractResp struct {
				Valid      bool
				Violations []string
			}
			err := workflow.ExecuteActivity(contractCtx, "ValidateContractActivity", ValidateContractActivityParam{
				OutputIRI:  stageResult.OutputIRI,
				SchemaName: stage.ContractSchema,
				OutputJSON: stageOutput,
			}).Get(ctx, &contractResp)
			if err != nil {
				logger.Warn("contract validation failed", "stage_id", stage.ID, "error", err)
			}
		}

		prevOutputs[stage.ID] = stageOutput
	}

	logger.Info("pipeline completed successfully")
	return nil
}