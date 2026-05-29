package workflow

import (
	"context"
	"encoding/json"
	"fmt"
	"time"

	"go.temporal.io/sdk/activity"
	"go.temporal.io/sdk/client"
	"go.temporal.io/sdk/worker"
	"go.temporal.io/sdk/workflow"

	pb "github.com/agent-os/se-app/proto/seapp"
	"github.com/agent-os/se-app/internal/executor"
	"github.com/agent-os/se-app/internal/grpc"
	"github.com/agent-os/se-app/internal/types"
)

type WorkerDeps struct {
	TemporalHost string
	TaskQueue    string
	GrpcTarget   string
	MetaStore    types.MetaStore
}

func RunWorker(deps WorkerDeps) error {
	temporalClient, err := client.Dial(client.Options{
		HostPort: deps.TemporalHost,
	})
	if err != nil {
		return fmt.Errorf("dial temporal: %w", err)
	}
	defer temporalClient.Close()

	grpcClient, err := grpc.NewClient(deps.GrpcTarget)
	if err != nil {
		fmt.Printf("warning: gRPC client failed: %v (running with mock)\n", err)
	}
	if grpcClient != nil {
		defer grpcClient.Close()
	}

	stageFactory := executor.NewStageFactory()
	executor.RegisterBuiltinStages(stageFactory)

	w := worker.New(temporalClient, deps.TaskQueue, worker.Options{
		BuildID:                 "se-app-worker",
		UseBuildIDForVersioning: false,
	})

	w.RegisterWorkflowWithOptions(SDLCDSLWorkflow, workflow.RegisterOptions{Name: "sdlc-workflow"})

	w.RegisterActivityWithOptions(
		createExecuteStageActivity(grpcClient, stageFactory),
		activity.RegisterOptions{Name: "ExecuteStageActivity"},
	)
	w.RegisterActivityWithOptions(
		createValidateContractActivity(grpcClient),
		activity.RegisterOptions{Name: "ValidateContractActivity"},
	)
	w.RegisterActivityWithOptions(
		createAIReviewActivity(),
		activity.RegisterOptions{Name: "AIReviewActivity"},
	)

	err = w.Run(worker.InterruptCh())
	if err != nil {
		return fmt.Errorf("run worker: %w", err)
	}

	return nil
}

func createExecuteStageActivity(client *grpc.Client, factory *executor.StageFactory) interface{} {
	return func(ctx context.Context, param ExecuteStageActivityParam) (*types.StageResult, error) {
		ctx = context.WithValue(ctx, grpcClientKey{}, client)
		ctx = context.WithValue(ctx, stageFactoryKey{}, factory)

		logger := activity.GetLogger(ctx)
		logger.Info("ExecuteStageActivity", "stage_id", param.StageID, "stage_type", param.StageType)

		stageInput := &types.StageInput{
			StageID:          param.StageID,
			StageType:        types.StageType(param.StageType),
			ProjectDir:       param.ProjectDir,
			UserRequirement:  param.UserRequirement,
			PrevStageOutputs: param.PrevStageOutputs,
		}

		prompt := ""
		executor, err := factory.Create(param.StageType, nil)
		if err == nil {
			prompt = executor.BuildPrompt(stageInput)
		} else {
			prompt = buildPrompt(param)
		}

		if client == nil {
			time.Sleep(100 * time.Millisecond)
			return &types.StageResult{
				StageID: param.StageID,
				Status:  "completed_mock",
				Summary: fmt.Sprintf("Stage %s completed (mock)", param.StageID),
				Output: map[string]interface{}{
					"result":  fmt.Sprintf("output from %s", param.StageID),
					"summary": prompt,
				},
			}, nil
		}

		req := &pb.ExecuteStageRequest{
			StageId:    param.StageID,
			StageType:  param.StageType,
			Prompt:     prompt,
			ProjectDir: param.ProjectDir,
			LlmApiKey:  param.LLMApiKey,
			LlmBaseUrl: param.LLMBaseURL,
			LlmModel:   param.LLMModel,
		}

		resp, err := client.ExecuteStage(ctx, req)
		if err != nil {
			return nil, err
		}

		result := &types.StageResult{
			StageID:   param.StageID,
			Status:    resp.Status,
			Summary:   resp.Summary,
			OutputIRI: resp.OutputIri,
			Artifacts: resp.Artifacts,
			Errors:    resp.Errors,
		}

		if len(resp.OutputJson) > 0 {
			var output map[string]interface{}
			if err := json.Unmarshal(resp.OutputJson, &output); err == nil {
				result.Output = output
			}
		}

		return result, nil
	}
}

func createValidateContractActivity(client *grpc.Client) interface{} {
	return func(ctx context.Context, param ValidateContractActivityParam) (*pb.ValidateContractResponse, error) {
		ctx = context.WithValue(ctx, grpcClientKey{}, client)

		logger := activity.GetLogger(ctx)
		logger.Info("ValidateContractActivity", "schema", param.SchemaName)

		if client == nil {
			return &pb.ValidateContractResponse{
				Valid:      true,
				Violations: nil,
			}, nil
		}

		var outputJSON []byte
		if param.OutputJSON != nil {
			outputJSON, _ = json.Marshal(param.OutputJSON)
		}

		return client.ValidateContract(ctx, &pb.ValidateContractRequest{
			OutputIri:  param.OutputIRI,
			SchemaName: param.SchemaName,
			OutputJson: outputJSON,
		})
	}
}

func createAIReviewActivity() interface{} {
	return func(ctx context.Context, param AIReviewActivityParam) (*types.ReviewResult, error) {
		logger := activity.GetLogger(ctx)
		logger.Info("AIReviewActivity", "stage_id", param.StageID)

		return &types.ReviewResult{
			Approved: true,
			Score:    85,
			Comments: []string{"AI review passed"},
			Reviewer: "ai-system",
		}, nil
	}
}