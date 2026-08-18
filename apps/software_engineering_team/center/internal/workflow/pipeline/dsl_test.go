package pipeline

import (
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestSDLCDSL_Validate_Valid(t *testing.T) {
	dsl := &SDCDSL{
		Name: "test-pipeline",
		Stages: []StageBlock{
			{ID: "req", StageType: "requirement", Name: "需求分析"},
			{ID: "design", StageType: "design", Name: "设计"},
		},
	}
	err := dsl.Validate()
	assert.NoError(t, err)
}

func TestSDLCDSL_Validate_EmptyName(t *testing.T) {
	dsl := &SDCDSL{
		Name:   "",
		Stages: []StageBlock{{ID: "s1", StageType: "test"}},
	}
	err := dsl.Validate()
	assert.EqualError(t, err, "DSL name is required")
}

func TestSDLCDSL_Validate_EmptyStages(t *testing.T) {
	dsl := &SDCDSL{
		Name:   "pipeline",
		Stages: []StageBlock{},
	}
	err := dsl.Validate()
	assert.EqualError(t, err, "DSL stages list is empty")
}

func TestSDLCDSL_Validate_NilStages(t *testing.T) {
	dsl := &SDCDSL{
		Name: "pipeline",
	}
	err := dsl.Validate()
	assert.EqualError(t, err, "DSL stages list is empty")
}

func TestSDLCDSL_Validate_MissingStageID(t *testing.T) {
	dsl := &SDCDSL{
		Name: "pipeline",
		Stages: []StageBlock{
			{ID: "", StageType: "requirement"},
		},
	}
	err := dsl.Validate()
	assert.EqualError(t, err, "stage 0: id is required")
}

func TestSDLCDSL_Validate_MissingStageType(t *testing.T) {
	dsl := &SDCDSL{
		Name: "pipeline",
		Stages: []StageBlock{
			{ID: "s1", StageType: ""},
		},
	}
	err := dsl.Validate()
	assert.EqualError(t, err, "stage 0: stage_type is required")
}

func TestSDLCDSL_Validate_MultipleStagesError(t *testing.T) {
	dsl := &SDCDSL{
		Name: "pipeline",
		Stages: []StageBlock{
			{ID: "s1", StageType: "requirement"},
			{ID: "", StageType: "design"},
		},
	}
	err := dsl.Validate()
	assert.EqualError(t, err, "stage 1: id is required")
}