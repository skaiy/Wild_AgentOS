package workflow

import (
	"testing"
	"time"

	"github.com/stretchr/testify/suite"
	"go.temporal.io/sdk/testsuite"
	"go.temporal.io/sdk/workflow"
)

type CallbackTestSuite struct {
	suite.Suite
	testsuite.WorkflowTestSuite
}

func (s *CallbackTestSuite) Test_CallbackSignalReceived() {
	env := s.NewTestWorkflowEnvironment()

	taskID := "task-001"
	stageID := "req"

	signalName := "callback-task-001-req"

	env.RegisterDelayedCallback(func() {
		env.SignalWorkflow(signalName, CallbackSignalPayload{
			TaskID:    taskID,
			StageID:   stageID,
			Status:    "completed",
			Summary:   "需求分析完成",
			OutputIRI: "iri://output/req-001",
		})
	}, 0)

	var result *CallbackSignalPayload
	env.ExecuteWorkflow(func(ctx workflow.Context) error {
		var err error
		result, err = waitForEdgeCallback(ctx, taskID, stageID)
		return err
	})

	s.True(env.IsWorkflowCompleted())
	s.NoError(env.GetWorkflowError())
	s.NotNil(result)
	s.Equal(taskID, result.TaskID)
	s.Equal(stageID, result.StageID)
	s.Equal("completed", result.Status)
	s.Equal("需求分析完成", result.Summary)
	s.Equal("iri://output/req-001", result.OutputIRI)
}

func (s *CallbackTestSuite) Test_CallbackSignalWithError() {
	env := s.NewTestWorkflowEnvironment()

	taskID := "task-002"
	stageID := "coding"

	signalName := "callback-task-002-coding"

	env.RegisterDelayedCallback(func() {
		env.SignalWorkflow(signalName, CallbackSignalPayload{
			TaskID:  taskID,
			StageID: stageID,
			Status:  "failed",
			Error:   "编译错误: undefined reference",
		})
	}, 0)

	var result *CallbackSignalPayload
	env.ExecuteWorkflow(func(ctx workflow.Context) error {
		var err error
		result, err = waitForEdgeCallback(ctx, taskID, stageID)
		return err
	})

	s.True(env.IsWorkflowCompleted())
	s.NoError(env.GetWorkflowError())
	s.NotNil(result)
	s.Equal("failed", result.Status)
	s.Contains(result.Error, "编译错误")
}

func (s *CallbackTestSuite) Test_CallbackSignalTimeout() {
	env := s.NewTestWorkflowEnvironment()

	env.SetTestTimeout(30 * time.Second)

	taskID := "task-003"
	stageID := "testing"

	var result *CallbackSignalPayload
	env.ExecuteWorkflow(func(ctx workflow.Context) error {
		var err error
		result, err = waitForEdgeCallback(ctx, taskID, stageID)
		return err
	})

	s.True(env.IsWorkflowCompleted())
	s.NoError(env.GetWorkflowError())
	s.NotNil(result)
	s.Equal(taskID, result.TaskID)
	s.Equal(stageID, result.StageID)
	s.Equal("timeout", result.Status)
}

func (s *CallbackTestSuite) Test_CallbackSignalDifferentStages() {
	env := s.NewTestWorkflowEnvironment()

	taskID := "task-004"

	signalName1 := "callback-task-004-req"
	signalName2 := "callback-task-004-design"

	env.RegisterDelayedCallback(func() {
		env.SignalWorkflow(signalName1, CallbackSignalPayload{
			TaskID: taskID, StageID: "req", Status: "completed",
		})
	}, 0)

	env.RegisterDelayedCallback(func() {
		env.SignalWorkflow(signalName2, CallbackSignalPayload{
			TaskID: taskID, StageID: "design", Status: "completed",
		})
	}, time.Millisecond*50)

	var result1, result2 *CallbackSignalPayload
	env.ExecuteWorkflow(func(ctx workflow.Context) error {
		var err error
		result1, err = waitForEdgeCallback(ctx, taskID, "req")
		if err != nil {
			return err
		}
		result2, err = waitForEdgeCallback(ctx, taskID, "design")
		return err
	})

	s.True(env.IsWorkflowCompleted())
	s.NoError(env.GetWorkflowError())
	s.NotNil(result1)
	s.NotNil(result2)
	s.Equal("completed", result1.Status)
	s.Equal("completed", result2.Status)
}

func TestCallbackTestSuite(t *testing.T) {
	suite.Run(t, new(CallbackTestSuite))
}