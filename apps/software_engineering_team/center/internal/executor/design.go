package executor

import (
	"encoding/json"
	"fmt"

	"github.com/agent-os/se-center/internal/types"
)

type DesignStage struct {
	config map[string]interface{}
}

func NewDesignStage(config map[string]interface{}) (StageExecutor, error) {
	return &DesignStage{config: config}, nil
}

func (s *DesignStage) Type() types.StageType {
	return types.StageDesign
}

func (s *DesignStage) Name() string {
	return "design"
}

func (s *DesignStage) BuildPrompt(input *types.StageInput) string {
	contextBlock := buildContextBlock(input)

	return fmt.Sprintf(`你是一位资深软件架构师。请根据需求分析结果进行系统设计。

## 用户需求

%s

%s## 设计要求

请以 JSON 格式输出设计文档，包含以下字段：
- "architecture": 系统架构描述，包含 "pattern", "overview", "components" 列表
- "data_model": 数据模型设计，包含实体列表和关系说明
- "api_design": API 接口设计，包含端点列表
- "module_structure": 模块结构设计，包含目录结构建议
- "tech_stack": 技术栈选择及理由
- "design_decisions": 关键设计决策及权衡

请确保输出为合法的 JSON 格式。`, input.UserRequirement, contextBlock)
}

func (s *DesignStage) ParseOutput(rawJSON string) (map[string]interface{}, error) {
	cleaned := extractJSON(rawJSON)
	if cleaned == "" {
		return nil, fmt.Errorf("design: empty JSON output")
	}
	var result map[string]interface{}
	if err := json.Unmarshal([]byte(cleaned), &result); err != nil {
		return nil, fmt.Errorf("design: failed to parse JSON: %w", err)
	}
	return result, nil
}

func (s *DesignStage) ValidateConfig() error {
	return nil
}