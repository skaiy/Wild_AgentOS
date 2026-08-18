package executor

import (
	"encoding/json"
	"testing"

	"github.com/agent-os/se-center/internal/types"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestStageFactory_RegisterAndCreate(t *testing.T) {
	factory := NewStageFactory()

	factory.Register("test", func(config map[string]interface{}) (StageExecutor, error) {
		return &RequirementStage{config: config}, nil
	})

	executor, err := factory.Create("test", nil)
	require.NoError(t, err)
	assert.NotNil(t, executor)
	assert.Equal(t, types.StageRequirement, executor.Type())
}

func TestStageFactory_CreateUnknown(t *testing.T) {
	factory := NewStageFactory()
	_, err := factory.Create("nonexistent", nil)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "unknown stage executor")
}

func TestStageFactory_ListRegistered(t *testing.T) {
	factory := NewStageFactory()
	factory.Register("a", NewRequirementStage)
	factory.Register("b", NewDesignStage)

	names := factory.ListRegistered()
	assert.ElementsMatch(t, []string{"a", "b"}, names)
}

func TestBuiltinStageRegistration(t *testing.T) {
	factory := NewStageFactory()
	RegisterBuiltinStages(factory)

	expected := []string{"requirement", "design", "coding", "testing", "review", "cicd", "deploy"}
	names := factory.ListRegistered()
	assert.ElementsMatch(t, expected, names)

	for _, name := range expected {
		executor, err := factory.Create(name, nil)
		require.NoError(t, err, "should create executor for %s", name)
		assert.NotNil(t, executor)
	}
}

func TestRequirementStage_BuildPrompt(t *testing.T) {
	s := &RequirementStage{}
	input := &types.StageInput{
		UserRequirement: "构建一个在线商店系统",
		PrevStageOutputs: map[string]interface{}{
			"design": map[string]interface{}{
				"architecture": "微服务架构",
			},
		},
	}
	prompt := s.BuildPrompt(input)
	assert.Contains(t, prompt, "构建一个在线商店系统")
	assert.Contains(t, prompt, "前置阶段输出")
	assert.Contains(t, prompt, "design 阶段输出")
	assert.Contains(t, prompt, "需求分析要求")
}

func TestDesignStage_BuildPrompt(t *testing.T) {
	s := &DesignStage{}
	input := &types.StageInput{
		UserRequirement: "构建一个在线商店系统",
	}
	prompt := s.BuildPrompt(input)
	assert.Contains(t, prompt, "构建一个在线商店系统")
	assert.Contains(t, prompt, "设计要求")
}

func TestCodingStage_BuildPrompt(t *testing.T) {
	s := &CodingStage{}
	input := &types.StageInput{
		UserRequirement: "构建一个在线商店系统",
	}
	prompt := s.BuildPrompt(input)
	assert.Contains(t, prompt, "构建一个在线商店系统")
	assert.Contains(t, prompt, "编码要求")
}

func TestTestingStage_BuildPrompt(t *testing.T) {
	s := &TestingStage{}
	input := &types.StageInput{
		UserRequirement: "构建一个在线商店系统",
	}
	prompt := s.BuildPrompt(input)
	assert.Contains(t, prompt, "构建一个在线商店系统")
	assert.Contains(t, prompt, "测试要求")
}

func TestReviewStage_BuildPrompt(t *testing.T) {
	s := &ReviewStage{}
	input := &types.StageInput{
		UserRequirement: "构建一个在线商店系统",
	}
	prompt := s.BuildPrompt(input)
	assert.Contains(t, prompt, "构建一个在线商店系统")
	assert.Contains(t, prompt, "审查要求")
}

func TestCICDStage_BuildPrompt(t *testing.T) {
	s := &CICDStage{}
	input := &types.StageInput{
		UserRequirement: "构建一个在线商店系统",
		PrevStageOutputs: map[string]interface{}{
			"testing": map[string]interface{}{
				"test_cases": "100 个测试用例",
			},
		},
	}
	prompt := s.BuildPrompt(input)
	assert.Contains(t, prompt, "构建一个在线商店系统")
	assert.Contains(t, prompt, "前置阶段输出")
	assert.Contains(t, prompt, "testing 阶段输出")
	assert.Contains(t, prompt, "CI/CD 要求")

	assert.NotContains(t, prompt, "%!s")
	assert.NotContains(t, prompt, "MISSING")
}

func TestDeployStage_BuildPrompt(t *testing.T) {
	s := &DeployStage{}
	input := &types.StageInput{
		UserRequirement: "构建一个在线商店系统",
		PrevStageOutputs: map[string]interface{}{
			"cicd": map[string]interface{}{
				"ci_pipeline": "Github Actions",
			},
		},
	}
	prompt := s.BuildPrompt(input)
	assert.Contains(t, prompt, "构建一个在线商店系统")
	assert.Contains(t, prompt, "前置阶段输出")
	assert.Contains(t, prompt, "cicd 阶段输出")
	assert.Contains(t, prompt, "部署要求")

	assert.NotContains(t, prompt, "%!s")
	assert.NotContains(t, prompt, "MISSING")
}

func TestGenericStage_BuildPrompt_ContextBlock(t *testing.T) {
	s := &genericStage{
		stageType:      types.StageCICD,
		stageName:      "cicd",
		promptTemplate: "需求: %s\n\n%s",
	}

	prompt := s.BuildPrompt(&types.StageInput{
		UserRequirement: "测试需求",
		PrevStageOutputs: map[string]interface{}{
			"prev": map[string]interface{}{"key": "val"},
		},
	})
	assert.Contains(t, prompt, "测试需求")
	assert.Contains(t, prompt, "前置阶段输出")
	assert.Contains(t, prompt, "prev 阶段输出")
}

func TestBuildContextBlock_Empty(t *testing.T) {
	block := buildContextBlock(nil)
	assert.Equal(t, "", block)

	block = buildContextBlock(&types.StageInput{})
	assert.Equal(t, "", block)

	block = buildContextBlock(&types.StageInput{
		PrevStageOutputs: map[string]interface{}{},
	})
	assert.Equal(t, "", block)
}

func TestBuildContextBlock_WithOutputs(t *testing.T) {
	block := buildContextBlock(&types.StageInput{
		PrevStageOutputs: map[string]interface{}{
			"design": map[string]interface{}{
				"architecture": "微服务",
			},
		},
	})
	assert.Contains(t, block, "前置阶段输出")
	assert.Contains(t, block, "design 阶段输出")
	assert.Contains(t, block, "微服务")
}

func TestParseOutput_ValidJSON(t *testing.T) {
	s := &RequirementStage{}
	result, err := s.ParseOutput(`{"key": "value", "num": 42}`)
	require.NoError(t, err)
	assert.Equal(t, "value", result["key"])
	assert.Equal(t, float64(42), result["num"])
}

func TestParseOutput_JSONInMarkdown(t *testing.T) {
	s := &RequirementStage{}
	raw := "```json\n{\"name\": \"test\"}\n```"
	result, err := s.ParseOutput(raw)
	require.NoError(t, err)
	assert.Equal(t, "test", result["name"])
}

func TestParseOutput_EmptyInput(t *testing.T) {
	s := &RequirementStage{}
	_, err := s.ParseOutput("")
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "empty JSON output")

	_, err = s.ParseOutput("   ")
	assert.Error(t, err)
}

func TestParseOutput_InvalidJSON(t *testing.T) {
	s := &RequirementStage{}
	_, err := s.ParseOutput("{invalid}")
	assert.Error(t, err)
}

func TestParseOutput_NestedJSON(t *testing.T) {
	s := &DesignStage{}
	raw := `{"outer": {"inner": [1, 2, 3]}}`
	result, err := s.ParseOutput(raw)
	require.NoError(t, err)
	outer, ok := result["outer"].(map[string]interface{})
	require.True(t, ok)
	assert.Equal(t, []interface{}{float64(1), float64(2), float64(3)}, outer["inner"])
}

func TestExtractJSON_Simple(t *testing.T) {
	result := extractJSON(`{"a": 1}`)
	assert.Equal(t, `{"a": 1}`, result)
}

func TestExtractJSON_WithPrefixText(t *testing.T) {
	result := extractJSON("这是一个 JSON：\n```\n{\"name\": \"test\"}\n```")
	assert.Equal(t, `{"name": "test"}`, result)
}

func TestExtractJSON_NoJSON(t *testing.T) {
	result := extractJSON("纯文本，没有 JSON")
	assert.Equal(t, "纯文本，没有 JSON", result)
}

func TestExtractJSON_EmptyString(t *testing.T) {
	result := extractJSON("")
	assert.Equal(t, "", result)
}

func TestExtractJSON_NestedBraces(t *testing.T) {
	result := extractJSON(`{"a": {"b": {"c": [1,2,3]}}, "d": "e"}`)
	assert.Equal(t, `{"a": {"b": {"c": [1,2,3]}}, "d": "e"}`, result)
}

func TestExtractJSON_JsonInJsonBlock(t *testing.T) {
	result := extractJSON("```json\n{\"key\": \"value\"}\n```")
	assert.Equal(t, `{"key": "value"}`, result)
}

func TestExtractJSON_MultipleBracesInString(t *testing.T) {
	result := extractJSON(`{"msg": "包含 { 和 } 字符"}`)
	assert.Equal(t, `{"msg": "包含 { 和 } 字符"}`, result)
}

func TestExtractJSON_WithEscapedQuotes(t *testing.T) {
	result := extractJSON(`{"msg": "包含 \"引号\" 字符"}`)
	assert.Equal(t, `{"msg": "包含 \"引号\" 字符"}`, result)
}

func TestTrimLeadingWhitespace(t *testing.T) {
	assert.Equal(t, "hello", trimLeadingWhitespace("  \n\thello"))
	assert.Equal(t, "   ", trimLeadingWhitespace("   "))
	assert.Equal(t, "a", trimLeadingWhitespace("a"))
}

func TestRequirementStage_TypeAndName(t *testing.T) {
	s := &RequirementStage{}
	assert.Equal(t, types.StageRequirement, s.Type())
	assert.Equal(t, "requirement", s.Name())
}

func TestDesignStage_TypeAndName(t *testing.T) {
	s := &DesignStage{}
	assert.Equal(t, types.StageDesign, s.Type())
	assert.Equal(t, "design", s.Name())
}

func TestCodingStage_TypeAndName(t *testing.T) {
	s := &CodingStage{}
	assert.Equal(t, types.StageCoding, s.Type())
	assert.Equal(t, "coding", s.Name())
}

func TestTestingStage_TypeAndName(t *testing.T) {
	s := &TestingStage{}
	assert.Equal(t, types.StageTesting, s.Type())
	assert.Equal(t, "testing", s.Name())
}

func TestReviewStage_TypeAndName(t *testing.T) {
	s := &ReviewStage{}
	assert.Equal(t, types.StageReview, s.Type())
	assert.Equal(t, "review", s.Name())
}

func TestCICDStage_TypeAndName(t *testing.T) {
	s := &CICDStage{}
	assert.Equal(t, types.StageCICD, s.Type())
	assert.Equal(t, "cicd", s.Name())
}

func TestDeployStage_TypeAndName(t *testing.T) {
	s := &DeployStage{}
	assert.Equal(t, types.StageDeploy, s.Type())
	assert.Equal(t, "deploy", s.Name())
}

func TestGenericStage_TypeAndName(t *testing.T) {
	s := &genericStage{
		stageType: types.StageCICD,
		stageName: "cicd",
	}
	assert.Equal(t, types.StageCICD, s.Type())
	assert.Equal(t, "cicd", s.Name())
}

func TestAllStageExecutors_ImplementsInterface(t *testing.T) {
	executors := []StageExecutor{
		&RequirementStage{},
		&DesignStage{},
		&CodingStage{},
		&TestingStage{},
		&ReviewStage{},
		&CICDStage{},
		&DeployStage{},
		&genericStage{},
	}

	for _, ex := range executors {
		assert.NotPanics(t, func() {
			_ = ex.Type()
			_ = ex.Name()
		}, "%T should not panic on basic methods", ex)
	}
}

func TestStageInputWithoutPrevious(t *testing.T) {
	executors := []StageExecutor{
		&RequirementStage{},
		&DesignStage{},
		&CodingStage{},
		&TestingStage{},
		&ReviewStage{},
		&CICDStage{},
		&DeployStage{},
	}

	input := &types.StageInput{
		UserRequirement: "测试需求",
	}

	for _, ex := range executors {
		prompt := ex.BuildPrompt(input)
		assert.Contains(t, prompt, "测试需求", "%T should contain user requirement", ex)
		assert.NotContains(t, prompt, "前置阶段输出", "%T should not include context block when none provided", ex)

		_, err := ex.ParseOutput(`{"result": "ok"}`)
		assert.NoError(t, err, "%T should parse JSON", ex)

		err = ex.ValidateConfig()
		assert.NoError(t, err, "%T ValidateConfig should return nil", ex)
	}
}

func TestAllExecutor_ParseOutputRoundTrip(t *testing.T) {
	executors := []StageExecutor{
		&RequirementStage{},
		&DesignStage{},
		&CodingStage{},
		&TestingStage{},
		&ReviewStage{},
		&CICDStage{},
		&DeployStage{},
	}

	expected := map[string]interface{}{
		"string_key": "value",
		"number_key": float64(42),
		"nested": map[string]interface{}{
			"inner": "data",
		},
	}
	rawJSON, err := json.Marshal(expected)
	require.NoError(t, err)

	for _, ex := range executors {
		result, err := ex.ParseOutput(string(rawJSON))
		require.NoError(t, err, "%T should parse round-tripped JSON", ex)
		assert.Equal(t, "value", result["string_key"])
		assert.Equal(t, float64(42), result["number_key"])

		nested, ok := result["nested"].(map[string]interface{})
		require.True(t, ok)
		assert.Equal(t, "data", nested["inner"])
	}
}

func TestGenericStage_ParseOutputWithPrefix(t *testing.T) {
	s := &genericStage{
		stageName: "cicd",
	}
	result, err := s.ParseOutput("思考过程...\n```json\n{\"plan\": \"deploy\"}\n```")
	require.NoError(t, err)
	assert.Equal(t, "deploy", result["plan"])
}

func TestGenericStage_ValidateConfig(t *testing.T) {
	s := &genericStage{
		config: map[string]interface{}{"key": "value"},
	}
	err := s.ValidateConfig()
	assert.NoError(t, err)
}

func TestBuildContextBlock_StringValues(t *testing.T) {
	block := buildContextBlock(&types.StageInput{
		PrevStageOutputs: map[string]interface{}{
			"analysis": "简单字符串输出",
		},
	})
	assert.Contains(t, block, "analysis 阶段输出")
	assert.Contains(t, block, "简单字符串输出")
}

func TestGenericStage_BuildPrompt_NilInput(t *testing.T) {
	s := &genericStage{
		stageType:      types.StageCICD,
		stageName:      "cicd",
		promptTemplate: "template: %s%s",
	}
	prompt := s.BuildPrompt(nil)
	assert.Equal(t, "template: ", prompt)
}

func TestGenericStage_BuildPrompt_EmptyInput(t *testing.T) {
	s := &genericStage{
		stageType:      types.StageCICD,
		stageName:      "cicd",
		promptTemplate: "template: %s\n%s",
	}
	prompt := s.BuildPrompt(&types.StageInput{})
	assert.Equal(t, "template: \n", prompt)
}

func TestCreateWithConfig(t *testing.T) {
	factory := NewStageFactory()
	RegisterBuiltinStages(factory)

	config := map[string]interface{}{
		"model": "gpt-4",
		"temperature": 0.7,
	}

	executor, err := factory.Create("coding", config)
	require.NoError(t, err)
	assert.NotNil(t, executor)

	prompt := executor.BuildPrompt(&types.StageInput{
		UserRequirement: "测试",
	})
	assert.Contains(t, prompt, "测试")
}

func TestExtractJSON_OnlyBraces(t *testing.T) {
	result := extractJSON("{}")
	assert.Equal(t, "{}", result)
}

func TestExtractJSON_SingleChar(t *testing.T) {
	result := extractJSON("{a}")
	assert.Equal(t, "{a}", result)
}

func TestExtractJSON_UnmatchedBraces(t *testing.T) {
	result := extractJSON("{{}}")
	assert.Equal(t, "{{}}", result)
}