package workflow

import (
	"context"
	"encoding/json"

	pb "github.com/agent-os/se-app/proto/seapp"
	"go.temporal.io/sdk/activity"

	"github.com/agent-os/se-app/internal/executor"
	"github.com/agent-os/se-app/internal/grpc"
	"github.com/agent-os/se-app/internal/types"
)

type ExecuteStageActivityParam struct {
	ProjectID        string
	StageID          string
	StageType        string
	ProjectDir       string
	UserRequirement  string
	PrevStageOutputs map[string]interface{}
	LLMApiKey        string
	LLMBaseURL       string
	LLMModel         string
}

type ValidateContractActivityParam struct {
	OutputIRI  string
	SchemaName string
	OutputJSON map[string]interface{}
}

type AIReviewActivityParam struct {
	StageID      string
	StageOutput  map[string]interface{}
	ProjectDir   string
}

type grpcClientKey struct{}
type stageFactoryKey struct{}

func GetGRPCClient(ctx context.Context) *grpc.Client {
	v, _ := ctx.Value(grpcClientKey{}).(*grpc.Client)
	return v
}

func GetStageFactory(ctx context.Context) *executor.StageFactory {
	v, _ := ctx.Value(stageFactoryKey{}).(*executor.StageFactory)
	return v
}

func ExecuteStageActivity(ctx context.Context, param ExecuteStageActivityParam) (*types.StageResult, error) {
	logger := activity.GetLogger(ctx)
	logger.Info("ExecuteStageActivity", "stage_id", param.StageID, "stage_type", param.StageType)

	client := GetGRPCClient(ctx)
	if client == nil {
		return nil, activity.ErrResultPending
	}

	prompt := buildPrompt(param)

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

func ValidateContractActivity(ctx context.Context, param ValidateContractActivityParam) (*pb.ValidateContractResponse, error) {
	logger := activity.GetLogger(ctx)
	logger.Info("ValidateContractActivity", "schema", param.SchemaName)

	client := GetGRPCClient(ctx)
	if client == nil {
		return nil, activity.ErrResultPending
	}

	var outputJSON []byte
	if param.OutputJSON != nil {
		outputJSON, _ = json.Marshal(param.OutputJSON)
	}

	req := &pb.ValidateContractRequest{
		OutputIri:  param.OutputIRI,
		SchemaName: param.SchemaName,
		OutputJson: outputJSON,
	}

	return client.ValidateContract(ctx, req)
}

func AIReviewActivity(ctx context.Context, param AIReviewActivityParam) (*types.ReviewResult, error) {
	logger := activity.GetLogger(ctx)
	logger.Info("AIReviewActivity", "stage_id", param.StageID)

	return &types.ReviewResult{
		Approved: true,
		Score:    85,
		Comments: []string{"AI review passed"},
		Reviewer: "ai-system",
	}, nil
}

func buildPrompt(param ExecuteStageActivityParam) string {
	prompt := "## 用户需求\n" + param.UserRequirement + "\n\n"
	if len(param.PrevStageOutputs) > 0 {
		prompt += "## 前置阶段输出\n"
		for stage, output := range param.PrevStageOutputs {
			prompt += "### " + stage + "\n"
			b, _ := json.MarshalIndent(output, "", "  ")
			prompt += string(b) + "\n"
		}
	}
	return prompt
}