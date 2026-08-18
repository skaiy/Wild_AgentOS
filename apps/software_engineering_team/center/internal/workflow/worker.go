package workflow

import (
	"fmt"

	"go.temporal.io/sdk/activity"
	"go.temporal.io/sdk/client"
	"go.temporal.io/sdk/worker"
	"go.temporal.io/sdk/workflow"

	"github.com/agent-os/se-center/internal/config"
	"github.com/agent-os/se-center/internal/grpc"
	"github.com/agent-os/se-center/internal/types"
)

type WorkerDeps struct {
	TemporalHost string
	TaskQueue    string
	GRPC         *grpc.Client
	MetaStore    types.MetaStore
	Config       *config.Config
}

func RunWorker(deps WorkerDeps) error {
	c, err := client.Dial(client.Options{
		HostPort: deps.TemporalHost,
	})
	if err != nil {
		return fmt.Errorf("temporal dial: %w", err)
	}
	defer c.Close()

	w := worker.New(c, deps.TaskQueue, worker.Options{
		BuildID:                 "center-worker-v1",
		UseBuildIDForVersioning: false,
	})

	w.RegisterWorkflowWithOptions(SDLCDSLWorkflow, workflow.RegisterOptions{
		Name: "sdlc-workflow",
	})

	w.RegisterActivityWithOptions(
		createExecuteStageActivity(deps.GRPC, deps.MetaStore, deps.Config),
		activity.RegisterOptions{Name: "ExecuteStage"},
	)
	w.RegisterActivityWithOptions(
		createValidateContractActivity(deps.GRPC),
		activity.RegisterOptions{Name: "ValidateContract"},
	)
	w.RegisterActivityWithOptions(
		createAIReviewActivity(deps.GRPC, deps.Config),
		activity.RegisterOptions{Name: "AIReview"},
	)
	w.RegisterActivityWithOptions(
		createDispatchTaskActivity(deps.MetaStore),
		activity.RegisterOptions{Name: "DispatchTask"},
	)

	return w.Run(worker.InterruptCh())
}