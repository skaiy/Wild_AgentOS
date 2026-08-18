package executor

import (
	"fmt"
	"strings"

	"github.com/agent-os/se-center/internal/types"
)

type StageExecutor interface {
	Type() types.StageType
	Name() string
	BuildPrompt(input *types.StageInput) string
	ParseOutput(rawJSON string) (map[string]interface{}, error)
	ValidateConfig() error
}

type StageExecutorFactory func(config map[string]interface{}) (StageExecutor, error)

type StageFactory struct {
	registry map[string]StageExecutorFactory
}

func NewStageFactory() *StageFactory {
	return &StageFactory{
		registry: make(map[string]StageExecutorFactory),
	}
}

func (f *StageFactory) Register(name string, factory StageExecutorFactory) {
	f.registry[name] = factory
}

func (f *StageFactory) Create(name string, config map[string]interface{}) (StageExecutor, error) {
	factory, ok := f.registry[name]
	if !ok {
		return nil, fmt.Errorf("unknown stage executor: %s", name)
	}
	return factory(config)
}

func (f *StageFactory) ListRegistered() []string {
	names := make([]string, 0, len(f.registry))
	for name := range f.registry {
		names = append(names, name)
	}
	return names
}

func buildContextBlock(input *types.StageInput) string {
	if input == nil || len(input.PrevStageOutputs) == 0 {
		return ""
	}
	var b strings.Builder
	b.WriteString("## 前置阶段输出\n\n")
	for stageName, output := range input.PrevStageOutputs {
		b.WriteString(fmt.Sprintf("### %s 阶段输出\n", stageName))
		if m, ok := output.(map[string]interface{}); ok {
			for k, v := range m {
				b.WriteString(fmt.Sprintf("- **%s**: %v\n", k, v))
			}
		} else {
			b.WriteString(fmt.Sprintf("%v\n", output))
		}
		b.WriteString("\n")
	}
	return b.String()
}