package executor

import (
	"encoding/json"
	"fmt"

	"github.com/agent-os/se-center/internal/types"
)

type CodingStage struct {
	config map[string]interface{}
}

func NewCodingStage(config map[string]interface{}) (StageExecutor, error) {
	return &CodingStage{config: config}, nil
}

func (s *CodingStage) Type() types.StageType {
	return types.StageCoding
}

func (s *CodingStage) Name() string {
	return "coding"
}

func (s *CodingStage) BuildPrompt(input *types.StageInput) string {
	contextBlock := buildContextBlock(input)

	return fmt.Sprintf(`你是一位高级软件开发工程师。请根据设计文档进行编码实现。

## 用户需求

%s

%s## 编码要求

请以 JSON 格式输出编码结果，包含以下字段：
- "files": 需创建或修改的文件列表，每条包含 "path", "content", "description"
- "dependencies": 依赖管理信息
- "build_instructions": 构建说明
- "implementation_notes": 实现要点和注意事项
- "test_strategy": 测试策略说明

请确保输出为合法的 JSON 格式。`, input.UserRequirement, contextBlock)
}

func (s *CodingStage) ParseOutput(rawJSON string) (map[string]interface{}, error) {
	cleaned := extractJSON(rawJSON)
	if cleaned == "" {
		return nil, fmt.Errorf("coding: empty JSON output")
	}
	var result map[string]interface{}
	if err := json.Unmarshal([]byte(cleaned), &result); err != nil {
		return nil, fmt.Errorf("coding: failed to parse JSON: %w", err)
	}
	return result, nil
}

func (s *CodingStage) ValidateConfig() error {
	return nil
}