package executor

import (
	"encoding/json"
	"fmt"

	"github.com/agent-os/se-center/internal/types"
)

type DeployStage struct {
	config map[string]interface{}
}

func NewDeployStage(config map[string]interface{}) (StageExecutor, error) {
	return &DeployStage{config: config}, nil
}

func (s *DeployStage) Type() types.StageType {
	return types.StageDeploy
}

func (s *DeployStage) Name() string {
	return "deploy"
}

func (s *DeployStage) BuildPrompt(input *types.StageInput) string {
	contextBlock := buildContextBlock(input)

	return fmt.Sprintf(`你是一位运维工程师。请根据前置阶段的输出制定部署方案。

## 用户需求

%s

%s## 部署要求

请以 JSON 格式输出部署方案，包含以下字段：
- "deployment_plan": 部署计划
- "rollout_strategy": 发布策略（灰度/蓝绿/金丝雀）
- "health_checks": 健康检查配置
- "backup_plan": 备份和恢复方案
- "monitoring_setup": 监控配置`, input.UserRequirement, contextBlock)
}

func (s *DeployStage) ParseOutput(rawJSON string) (map[string]interface{}, error) {
	cleaned := extractJSON(rawJSON)
	if cleaned == "" {
		return nil, fmt.Errorf("deploy: empty JSON output")
	}
	var result map[string]interface{}
	if err := json.Unmarshal([]byte(cleaned), &result); err != nil {
		return nil, fmt.Errorf("deploy: failed to parse JSON: %w", err)
	}
	return result, nil
}

func (s *DeployStage) ValidateConfig() error {
	return nil
}