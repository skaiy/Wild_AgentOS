package workflow

import (
	"testing"
	"time"

	"github.com/stretchr/testify/suite"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/workflow"
)

type HumanReviewTestSuite struct {
	suite.Suite
	testsuite.WorkflowTestSuite
}

func (s *HumanReviewTestSuite) Test_HumanReviewSignalReceived() {
	env := s.NewTestWorkflowEnvironment()

	env.RegisterDelayedCallback(func() {
		env.SignalWorkflow(HumanReviewSignalName, HumanReviewSignalPayload{
			StageID:  "design",
			Approved: true,
			Comments: []string{"设计方案通过"},
			Reviewer: "alice",
		})
	}, 0)

	var result *HumanReviewSignalPayload
	env.ExecuteWorkflow(func(ctx workflow.Context) error {
		var err error
		result, err = waitForHumanReview(ctx, "design")
		return err
	})

	s.True(env.IsWorkflowCompleted())
	s.NoError(env.GetWorkflowError())
	s.NotNil(result)
	s.True(result.Approved)
	s.Equal("design", result.StageID)
	s.Equal("alice", result.Reviewer)
	s.Contains(result.Comments, "设计方案通过")
}

func (s *HumanReviewTestSuite) Test_HumanReviewRejected() {
	env := s.NewTestWorkflowEnvironment()

	env.RegisterDelayedCallback(func() {
		env.SignalWorkflow(HumanReviewSignalName, HumanReviewSignalPayload{
			StageID:  "coding",
			Approved: false,
			Comments: []string{"代码需要重构"},
			Reviewer: "bob",
		})
	}, 0)

	var result *HumanReviewSignalPayload
	env.ExecuteWorkflow(func(ctx workflow.Context) error {
		var err error
		result, err = waitForHumanReview(ctx, "coding")
		return err
	})

	s.True(env.IsWorkflowCompleted())
	s.NoError(env.GetWorkflowError())
	s.NotNil(result)
	s.False(result.Approved)
	s.Equal("coding", result.StageID)
}

func (s *HumanReviewTestSuite) Test_HumanReviewTimeout() {
	env := s.NewTestWorkflowEnvironment()

	env.SetTestTimeout(30 * time.Second)

	var result *HumanReviewSignalPayload
	env.ExecuteWorkflow(func(ctx workflow.Context) error {
		var err error
		result, err = waitForHumanReview(ctx, "testing")
		return err
	})

	s.True(env.IsWorkflowCompleted())
	s.NoError(env.GetWorkflowError())
	s.NotNil(result)
	s.True(result.Approved)
	s.Equal("testing", result.StageID)
	s.Contains(result.Comments[0], "自动通过")
}

func (s *HumanReviewTestSuite) Test_HumanReviewMultipleSignals() {
	env := s.NewTestWorkflowEnvironment()

	env.RegisterDelayedCallback(func() {
		env.SignalWorkflow(HumanReviewSignalName, HumanReviewSignalPayload{
			StageID:  "deploy",
			Approved: false,
			Comments: []string{"驳回"},
		})
	}, 0)

	env.RegisterDelayedCallback(func() {
		env.SignalWorkflow(HumanReviewSignalName, HumanReviewSignalPayload{
			StageID:  "deploy",
			Approved: true,
			Comments: []string{"通过"},
		})
	}, time.Millisecond*50)

	var result *HumanReviewSignalPayload
	env.ExecuteWorkflow(func(ctx workflow.Context) error {
		var err error
		result, err = waitForHumanReview(ctx, "deploy")
		return err
	})

	s.True(env.IsWorkflowCompleted())
	s.NoError(env.GetWorkflowError())
	s.NotNil(result)
	s.False(result.Approved)
}

func TestHumanReviewTestSuite(t *testing.T) {
	suite.Run(t, new(HumanReviewTestSuite))
}