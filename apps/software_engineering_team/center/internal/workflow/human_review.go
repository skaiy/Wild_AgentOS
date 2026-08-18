package workflow

import (
	"time"

	"go.temporal.io/sdk/workflow"
)

type HumanReviewSignalPayload struct {
	StageID  string   `json:"stage_id"`
	Approved bool     `json:"approved"`
	Comments []string `json:"comments"`
	Reviewer string   `json:"reviewer"`
	TaskID   string   `json:"task_id"`
}

func waitForHumanReview(ctx workflow.Context, stageID string) (*HumanReviewSignalPayload, error) {
	logger := workflow.GetLogger(ctx)
	logger.Info("等待人工审查", "stage_id", stageID)

	var received bool
	var signal HumanReviewSignalPayload

	selector := workflow.NewSelector(ctx)
	signalChan := workflow.GetSignalChannel(ctx, HumanReviewSignalName)

	selector.AddReceive(signalChan, func(c workflow.ReceiveChannel, _ bool) {
		c.Receive(ctx, &signal)
		received = true
		logger.Info("人工审查信号已接收", "approved", signal.Approved)
	})

	timerCtx, cancel := workflow.NewDisconnectedContext(ctx)
	defer cancel()
	timer := workflow.NewTimer(timerCtx, 24*time.Hour)

	selector.AddFuture(timer, func(f workflow.Future) {
		logger.Warn("人工审查超时，默认通过")
		received = true
		signal = HumanReviewSignalPayload{
			StageID:  stageID,
			Approved: true,
			Comments: []string{"审查超时，自动通过"},
		}
	})

	for !received {
		selector.Select(ctx)
	}

	return &signal, nil
}