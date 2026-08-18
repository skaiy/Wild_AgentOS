package workflow

import (
	"fmt"
	"time"

	"go.temporal.io/sdk/workflow"
)

type CallbackSignalPayload struct {
	TaskID    string `json:"task_id"`
	StageID   string `json:"stage_id"`
	Status    string `json:"status"`
	Summary   string `json:"summary"`
	OutputIRI string `json:"output_iri"`
	Error     string `json:"error,omitempty"`
}

func waitForEdgeCallback(ctx workflow.Context, taskID, stageID string) (*CallbackSignalPayload, error) {
	logger := workflow.GetLogger(ctx)
	logger.Info("等待边缘回调", "task_id", taskID, "stage_id", stageID)

	var received bool
	var result CallbackSignalPayload

	signalName := fmt.Sprintf("callback-%s-%s", taskID, stageID)
	signalChan := workflow.GetSignalChannel(ctx, signalName)

	selector := workflow.NewSelector(ctx)
	selector.AddReceive(signalChan, func(c workflow.ReceiveChannel, _ bool) {
		c.Receive(ctx, &result)
		received = true
	})

	timerCtx, cancel := workflow.NewDisconnectedContext(ctx)
	defer cancel()
	timer := workflow.NewTimer(timerCtx, 24*time.Hour)

	selector.AddFuture(timer, func(f workflow.Future) {
		logger.Warn("边缘回调超时")
		received = true
		result = CallbackSignalPayload{
			TaskID:  taskID,
			StageID: stageID,
			Status:  "timeout",
		}
	})

	for !received {
		selector.Select(ctx)
	}

	return &result, nil
}