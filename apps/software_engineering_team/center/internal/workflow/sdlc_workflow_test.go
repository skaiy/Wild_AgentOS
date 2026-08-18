package workflow

import (
	"context"
	"testing"

	"github.com/stretchr/testify/suite"
	"go.temporal.io/sdk/testsuite"

	"github.com/agent-os/se-center/internal/types"
	"github.com/agent-os/se-center/internal/workflow/pipeline"
)

type testActivities struct{}

func (a *testActivities) ExecuteStage(_ context.Context, params ExecuteStageParams) (*types.StageResult, error) {
	return &types.StageResult{
		StageID: params.StageID,
		Status:  "completed",
		Summary: params.StageID + "_done",
	}, nil
}

func (a *testActivities) AIReview(_ context.Context, params AIReviewParams) (*types.ReviewResult, error) {
	return &types.ReviewResult{
		Approved: true,
		Score:    90,
		Comments: []string{"approved"},
	}, nil
}

type SDLCWorkflowTestSuite struct {
	suite.Suite
	testsuite.WorkflowTestSuite
}

func (s *SDLCWorkflowTestSuite) Test_FullPipeline_Success() {
	env := s.NewTestWorkflowEnvironment()

	env.RegisterWorkflow(SDLCDSLWorkflow)
	env.RegisterActivity(&testActivities{})

	dsl := pipeline.SDCDSL{
		Name: "test-pipeline",
		Stages: []pipeline.StageBlock{
			{ID: "req", StageType: "requirement", AIReview: true, HumanReview: false},
			{ID: "design", StageType: "design", AIReview: true, HumanReview: false},
			{ID: "coding", StageType: "coding", AIReview: true, HumanReview: false},
		},
		State: pipeline.WorkflowState{
			CurrentStageIdx: 0,
			PrevOutputs:     nil,
		},
	}

	env.ExecuteWorkflow(SDLCDSLWorkflow, dsl)

	s.True(env.IsWorkflowCompleted())
	s.NoError(env.GetWorkflowError())

	var result string
	err := env.GetWorkflowResult(&result)
	s.NoError(err)
	s.Equal("completed", result)
}

func (s *SDLCWorkflowTestSuite) Test_ResumeFromMiddle() {
	env := s.NewTestWorkflowEnvironment()

	env.RegisterWorkflow(SDLCDSLWorkflow)
	env.RegisterActivity(&testActivities{})

	prevOutputs := map[string]pipeline.StageResult{
		"req": {StageID: "req", Output: "req_output", Status: "completed"},
	}

	dsl := pipeline.SDCDSL{
		Name: "resume-pipeline",
		Stages: []pipeline.StageBlock{
			{ID: "req", StageType: "requirement"},
			{ID: "design", StageType: "design"},
			{ID: "coding", StageType: "coding"},
		},
		State: pipeline.WorkflowState{
			CurrentStageIdx: 1,
			PrevOutputs:     prevOutputs,
		},
	}

	env.ExecuteWorkflow(SDLCDSLWorkflow, dsl)

	s.True(env.IsWorkflowCompleted())
	s.NoError(env.GetWorkflowError())

	var result string
	err := env.GetWorkflowResult(&result)
	s.NoError(err)
	s.Equal("completed", result)
}

func (s *SDLCWorkflowTestSuite) Test_HumanReviewRejected() {
	env := s.NewTestWorkflowEnvironment()

	env.RegisterWorkflow(SDLCDSLWorkflow)
	env.RegisterActivity(&testActivities{})

	dsl := pipeline.SDCDSL{
		Name: "rejected-pipeline",
		Stages: []pipeline.StageBlock{
			{ID: "req", StageType: "requirement", HumanReview: true},
		},
		State: pipeline.WorkflowState{
			CurrentStageIdx: 0,
			PrevOutputs:     nil,
		},
	}

	env.RegisterDelayedCallback(func() {
		env.SignalWorkflow(HumanReviewSignalName, HumanReviewSignalPayload{
			StageID:  "req",
			Approved: false,
			Comments: []string{"需要修改"},
		})
	}, 0)

	env.ExecuteWorkflow(SDLCDSLWorkflow, dsl)

	s.True(env.IsWorkflowCompleted())
	s.Error(env.GetWorkflowError())
}

func TestSDLCWorkflowTestSuite(t *testing.T) {
	suite.Run(t, new(SDLCWorkflowTestSuite))
}