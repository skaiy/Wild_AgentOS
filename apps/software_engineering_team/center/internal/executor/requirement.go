package executor

import (
	"encoding/json"
	"fmt"

	"github.com/agent-os/se-center/internal/types"
)

type RequirementStage struct {
	config map[string]interface{}
}

func NewRequirementStage(config map[string]interface{}) (StageExecutor, error) {
	return &RequirementStage{config: config}, nil
}

func (s *RequirementStage) Type() types.StageType {
	return types.StageRequirement
}

func (s *RequirementStage) Name() string {
	return "requirement"
}

func (s *RequirementStage) BuildPrompt(input *types.StageInput) string {
	contextBlock := buildContextBlock(input)

	return fmt.Sprintf(`你是一位资深需求分析工程师。请根据以下信息进行需求分析。

## 用户需求

%s

%s## 需求分析要求

请以 JSON 格式输出需求分析结果，包含以下字段：
- "requirements": 功能需求列表，每条包含 "id", "title", "description", "priority"
- "non_functional": 非功能需求列表，每条包含 "title", "description"
- "constraints": 项目约束条件列表
- "risks": 潜在风险列表
- "summary": 需求概要总结

请确保输出为合法的 JSON 格式。`, input.UserRequirement, contextBlock)
}

func (s *RequirementStage) ParseOutput(rawJSON string) (map[string]interface{}, error) {
	cleaned := extractJSON(rawJSON)
	if cleaned == "" {
		return nil, fmt.Errorf("requirement: empty JSON output")
	}
	var result map[string]interface{}
	if err := json.Unmarshal([]byte(cleaned), &result); err != nil {
		return nil, fmt.Errorf("requirement: failed to parse JSON: %w", err)
	}
	return result, nil
}

func (s *RequirementStage) ValidateConfig() error {
	return nil
}