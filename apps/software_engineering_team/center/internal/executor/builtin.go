package executor

import (
	"encoding/json"
	"fmt"

	"github.com/agent-os/se-center/internal/types"
)

func RegisterBuiltinStages(factory *StageFactory) {
	factory.Register("requirement", NewRequirementStage)
	factory.Register("design", NewDesignStage)
	factory.Register("coding", NewCodingStage)
	factory.Register("testing", NewTestingStage)
	factory.Register("review", NewReviewStage)

	factory.Register("cicd", func(config map[string]interface{}) (StageExecutor, error) {
		return &genericStage{
			stageType: types.StageType("cicd"),
			stageName: "cicd",
			config:    config,
			promptTemplate: `你是一位 DevOps 工程师。请根据前置阶段的输出设计 CI/CD 流水线。

%s

%s## CI/CD 要求

请以 JSON 格式输出 CI/CD 配置方案，包含以下字段：
- "ci_pipeline": CI 流水线配置
- "cd_pipeline": CD 流水线配置
- "environments": 环境管理方案
- "monitoring": 监控和告警方案
- "rollback_strategy": 回滚策略`,
		}, nil
	})

	factory.Register("deploy", func(config map[string]interface{}) (StageExecutor, error) {
		return &genericStage{
			stageType: types.StageType("deploy"),
			stageName: "deploy",
			config:    config,
			promptTemplate: `你是一位运维工程师。请根据前置阶段的输出制定部署方案。

%s

%s## 部署要求

请以 JSON 格式输出部署方案，包含以下字段：
- "deployment_plan": 部署计划
- "rollout_strategy": 发布策略（灰度/蓝绿/金丝雀）
- "health_checks": 健康检查配置
- "backup_plan": 备份和恢复方案
- "monitoring_setup": 监控配置`,
		}, nil
	})
}

type genericStage struct {
	stageType      types.StageType
	stageName      string
	config         map[string]interface{}
	promptTemplate string
}

func (s *genericStage) Type() types.StageType {
	return s.stageType
}

func (s *genericStage) Name() string {
	return s.stageName
}

func (s *genericStage) BuildPrompt(input *types.StageInput) string {
	contextBlock := buildContextBlock(input)
	userReq := ""
	if input != nil {
		userReq = input.UserRequirement
	}
	return fmt.Sprintf(s.promptTemplate, userReq, contextBlock)
}

func (s *genericStage) ParseOutput(rawJSON string) (map[string]interface{}, error) {
	cleaned := extractJSON(rawJSON)
	if cleaned == "" {
		return nil, fmt.Errorf("%s: empty JSON output", s.stageName)
	}
	var result map[string]interface{}
	if err := json.Unmarshal([]byte(cleaned), &result); err != nil {
		return nil, fmt.Errorf("%s: failed to parse JSON: %w", s.stageName, err)
	}
	return result, nil
}

func (s *genericStage) ValidateConfig() error {
	return nil
}