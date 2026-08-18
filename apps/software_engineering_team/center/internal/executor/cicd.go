package executor

import (
	"encoding/json"
	"fmt"

	"github.com/agent-os/se-center/internal/types"
)

type CICDStage struct {
	config map[string]interface{}
}

func NewCICDStage(config map[string]interface{}) (StageExecutor, error) {
	return &CICDStage{config: config}, nil
}

func (s *CICDStage) Type() types.StageType {
	return types.StageCICD
}

func (s *CICDStage) Name() string {
	return "cicd"
}

func (s *CICDStage) BuildPrompt(input *types.StageInput) string {
	contextBlock := buildContextBlock(input)

	return fmt.Sprintf(`你是一位 DevOps 工程师。请根据前置阶段的输出设计 CI/CD 流水线。

## 用户需求

%s

%s## CI/CD 要求

请以 JSON 格式输出 CI/CD 配置方案，包含以下字段：
- "ci_pipeline": CI 流水线配置
- "cd_pipeline": CD 流水线配置
- "environments": 环境管理方案
- "monitoring": 监控和告警方案
- "rollback_strategy": 回滚策略`, input.UserRequirement, contextBlock)
}

func (s *CICDStage) ParseOutput(rawJSON string) (map[string]interface{}, error) {
	cleaned := extractJSON(rawJSON)
	if cleaned == "" {
		return nil, fmt.Errorf("cicd: empty JSON output")
	}
	var result map[string]interface{}
	if err := json.Unmarshal([]byte(cleaned), &result); err != nil {
		return nil, fmt.Errorf("cicd: failed to parse JSON: %w", err)
	}
	return result, nil
}

func (s *CICDStage) ValidateConfig() error {
	return nil
}