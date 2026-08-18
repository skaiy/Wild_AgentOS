package executor

import (
	"encoding/json"
	"fmt"

	"github.com/agent-os/se-center/internal/types"
)

type TestingStage struct {
	config map[string]interface{}
}

func NewTestingStage(config map[string]interface{}) (StageExecutor, error) {
	return &TestingStage{config: config}, nil
}

func (s *TestingStage) Type() types.StageType {
	return types.StageTesting
}

func (s *TestingStage) Name() string {
	return "testing"
}

func (s *TestingStage) BuildPrompt(input *types.StageInput) string {
	contextBlock := buildContextBlock(input)

	return fmt.Sprintf(`你是一位资深测试工程师。请根据需求分析和编码结果编写测试方案。

## 用户需求

%s

%s## 测试要求

请以 JSON 格式输出测试方案，包含以下字段：
- "test_cases": 测试用例列表，每条包含 "id", "title", "description", "expected_result", "type"
- "unit_tests": 单元测试清单及覆盖范围说明
- "integration_tests": 集成测试方案
- "e2e_tests": 端到端测试场景
- "test_automation": 自动化测试策略
- "quality_metrics": 质量标准和质量门禁

请确保输出为合法的 JSON 格式。`, input.UserRequirement, contextBlock)
}

func (s *TestingStage) ParseOutput(rawJSON string) (map[string]interface{}, error) {
	cleaned := extractJSON(rawJSON)
	if cleaned == "" {
		return nil, fmt.Errorf("testing: empty JSON output")
	}
	var result map[string]interface{}
	if err := json.Unmarshal([]byte(cleaned), &result); err != nil {
		return nil, fmt.Errorf("testing: failed to parse JSON: %w", err)
	}
	return result, nil
}

func (s *TestingStage) ValidateConfig() error {
	return nil
}