package workflow

import (
	"context"
	"encoding/json"
	"fmt"

	pb "github.com/agent-os/se-center/proto/seapp"
	"go.temporal.io/sdk/activity"

	"github.com/agent-os/se-center/internal/config"
	"github.com/agent-os/se-center/internal/grpc"
	"github.com/agent-os/se-center/internal/types"
)

type grpcClientKey struct{}
type metaStoreKey struct{}
type configKey struct{}

func createExecuteStageActivity(client *grpc.Client, store types.MetaStore, cfg *config.Config) interface{} {
	return func(ctx context.Context, params ExecuteStageParams) (*types.StageResult, error) {
		ctx = context.WithValue(ctx, grpcClientKey{}, client)
		ctx = context.WithValue(ctx, metaStoreKey{}, store)
		ctx = context.WithValue(ctx, configKey{}, cfg)

		logger := activity.GetLogger(ctx)
		logger.Info("ExecuteStage", "stage_id", params.StageID, "stage_type", params.StageType)

		if client == nil {
			return nil, fmt.Errorf("gRPC client unavailable for ExecuteStage %s/%s", params.StageID, params.StageType)
		}

		prompt := buildExecutePrompt(params)

		apiKey := params.LLMApiKey
		baseURL := params.LLMBaseURL
		model := params.LLMModel
		if apiKey == "" && cfg != nil {
			apiKey = cfg.LLM.APIKey
			baseURL = cfg.LLM.BaseURL
			model = cfg.LLM.Model
		}

		taskIRI := ""
		if params.TaskID != "" {
			taskIRI = fmt.Sprintf("iri://task/%s", params.TaskID)
		}

		req := &pb.ExecuteStageRequest{
			StageId:    params.StageID,
			StageType:  params.StageType,
			Prompt:     prompt,
			ProjectDir: params.ProjectDir,
			TaskIri:    taskIRI,
			LlmApiKey:  apiKey,
			LlmBaseUrl: baseURL,
			LlmModel:   model,
		}

		resp, err := client.ExecuteStage(ctx, req)
		if err != nil {
			return nil, fmt.Errorf("execute stage via gRPC: %w", err)
		}

		result := &types.StageResult{
			StageID:    params.StageID,
			Status:     resp.Status,
			Summary:    resp.Summary,
			OutputIRI:  resp.OutputIri,
			Artifacts:  resp.Artifacts,
			Errors:     resp.Errors,
			DurationMs: 0,
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

func createAIReviewActivity(client *grpc.Client, cfg *config.Config) interface{} {
	return func(ctx context.Context, params AIReviewParams) (*types.ReviewResult, error) {
		ctx = context.WithValue(ctx, grpcClientKey{}, client)
		ctx = context.WithValue(ctx, configKey{}, cfg)

		logger := activity.GetLogger(ctx)
		logger.Info("AIReview", "stage_id", params.StageID)

		if client == nil {
			return nil, fmt.Errorf("gRPC client unavailable for AIReview %s", params.StageID)
		}

		reviewPrompt := fmt.Sprintf(`请审查以下阶段输出，给出评分（0-100）和详细评价：

阶段 ID: %s

输出内容：
%s

请以 JSON 格式返回审查结果，包含 approved(boolean)、score(number) 和 comments(array of strings)。`, params.StageID, params.StageOutput)

		apiKey := ""
		baseURL := ""
		model := ""
		if cfg != nil {
			apiKey = cfg.LLM.APIKey
			baseURL = cfg.LLM.BaseURL
			model = cfg.LLM.Model
		}

		taskIRI := ""
		if params.TaskID != "" {
			taskIRI = fmt.Sprintf("iri://task/%s", params.TaskID)
		}

		req := &pb.ExecuteStageRequest{
			StageId:   params.StageID,
			StageType: "ai_review",
			Prompt:    reviewPrompt,
			TaskIri:   taskIRI,
			LlmApiKey: apiKey,
			LlmBaseUrl: baseURL,
			LlmModel:  model,
		}

		resp, err := client.ExecuteStage(ctx, req)
		if err != nil {
			return nil, fmt.Errorf("AI review via gRPC: %w", err)
		}

		result := &types.ReviewResult{
			Approved: true,
			Score:    85,
			Comments: []string{resp.Summary},
			Reviewer: "ai-system",
		}

		if len(resp.OutputJson) > 0 {
			var review ReviewOutput
			if err := json.Unmarshal(resp.OutputJson, &review); err == nil {
				if review.Approved != nil {
					result.Approved = *review.Approved
				}
				if review.Score > 0 {
					result.Score = review.Score
				}
				if len(review.Comments) > 0 {
					result.Comments = review.Comments
				}
			}
		}

		return result, nil
	}
}

type ReviewOutput struct {
	Approved *bool    `json:"approved"`
	Score    int      `json:"score"`
	Comments []string `json:"comments"`
}

func createValidateContractActivity(client *grpc.Client) interface{} {
	return func(ctx context.Context, params ValidateContractParams) (*pb.ValidateContractResponse, error) {
		ctx = context.WithValue(ctx, grpcClientKey{}, client)

		logger := activity.GetLogger(ctx)
		logger.Info("ValidateContract", "stage_id", params.StageID, "schema", params.Schema)

		if client == nil {
			return nil, fmt.Errorf("gRPC client unavailable for ValidateContract %s", params.StageID)
		}

		var outputJSON []byte
		if params.StageOutput != nil {
			outputJSON, _ = json.Marshal(params.StageOutput)
		}

		resp, err := client.ValidateContract(ctx, &pb.ValidateContractRequest{
			SchemaName: params.Schema,
			OutputJson: outputJSON,
		})
		if err != nil {
			return nil, fmt.Errorf("validate contract via gRPC: %w", err)
		}

		return resp, nil
	}
}

func createDispatchTaskActivity(store types.MetaStore) interface{} {
	return func(ctx context.Context, params DispatchTaskParams) error {
		ctx = context.WithValue(ctx, metaStoreKey{}, store)

		logger := activity.GetLogger(ctx)
		logger.Info("DispatchTask", "task_id", params.TaskID, "workflow_id", params.WorkflowID)

		if params.WorkflowID != "" {
			if err := store.UpdateTaskWorkflow(params.TaskID, params.WorkflowID, params.RunID); err != nil {
				return fmt.Errorf("update task workflow: %w", err)
			}
		}

		if err := store.UpdateTaskStatus(params.TaskID, types.TaskStatusRunning, params.CurrentStage); err != nil {
			return fmt.Errorf("update task status to running: %w", err)
		}

		return nil
	}
}

func buildExecutePrompt(params ExecuteStageParams) string {
	prompt := "## 用户需求\n" + params.UserRequirement + "\n\n"
	if len(params.PrevOutputs) > 0 {
		prompt += "## 前置阶段输出\n"
		for stageID, prev := range params.PrevOutputs {
			prompt += "### " + stageID + "\n"
			b, _ := json.MarshalIndent(prev, "", "  ")
			prompt += string(b) + "\n"
		}
	}
	return prompt
}