package executor

import (
	"encoding/json"
	"fmt"

	"github.com/agent-os/se-center/internal/types"
)

type ReviewStage struct {
	config map[string]interface{}
}

func NewReviewStage(config map[string]interface{}) (StageExecutor, error) {
	return &ReviewStage{config: config}, nil
}

func (s *ReviewStage) Type() types.StageType {
	return types.StageReview
}

func (s *ReviewStage) Name() string {
	return "review"
}

func (s *ReviewStage) BuildPrompt(input *types.StageInput) string {
	contextBlock := buildContextBlock(input)

	return fmt.Sprintf(`你是一位资深代码审查工程师。请对所有前置阶段的输出进行全面审查。

## 用户需求

%s

%s## 审查要求

请以 JSON 格式输出审查结果，包含以下字段：
- "overall_assessment": 总体评估，包含 "score", "verdict", "summary"
- "issues": 发现的问题列表，每条包含 "severity", "category", "description", "suggestion"
- "strengths": 亮点和改进建议
- "security_review": 安全审查结果
- "performance_review": 性能审查结果
- "compliance_check": 规范符合性检查
- "final_recommendation": 最终建议（批准/需修改/拒绝）

请确保输出为合法的 JSON 格式。`, input.UserRequirement, contextBlock)
}

func (s *ReviewStage) ParseOutput(rawJSON string) (map[string]interface{}, error) {
	cleaned := extractJSON(rawJSON)
	if cleaned == "" {
		return nil, fmt.Errorf("review: empty JSON output")
	}
	var result map[string]interface{}
	if err := json.Unmarshal([]byte(cleaned), &result); err != nil {
		return nil, fmt.Errorf("review: failed to parse JSON: %w", err)
	}
	return result, nil
}

func (s *ReviewStage) ValidateConfig() error {
	return nil
}