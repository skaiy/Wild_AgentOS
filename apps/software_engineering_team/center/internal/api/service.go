package api

import (
	"context"

	"github.com/agent-os/se-center/internal/agent"
	"github.com/agent-os/se-center/internal/config"
	"github.com/agent-os/se-center/internal/graph"
	"github.com/agent-os/se-center/internal/grpc"
	"github.com/agent-os/se-center/internal/types"
	"go.temporal.io/sdk/client"
)

type AgentManager interface {
	Register(info *agent.AgentInfo) error
	Heartbeat(agentID string) error
	GetOnlineAgents() ([]*agent.AgentInfo, error)
	MatchAgent(capability string) *agent.AgentInfo
	StartTimeoutScanner(ctx context.Context)
	StopTimeoutScanner()
}

type GraphManager interface {
	SyncFromEdge(ctx context.Context, req graph.SyncRequest) (*graph.SyncResponse, error)
}

type Service struct {
	Config         *config.Config
	ConfigPath     string
	MetaStore      types.MetaStore
	GRPC           *grpc.Client
	TemporalClient client.Client
	Hub            *Hub
	TaskQueue      string
	AgentManager   AgentManager
	GraphManager   GraphManager
}

func NewService(cfg *config.Config, configPath string, metaStore types.MetaStore, grpcClient *grpc.Client, temporalClient client.Client, taskQueue string) *Service {
	return &Service{
		Config:         cfg,
		ConfigPath:     configPath,
		MetaStore:      metaStore,
		GRPC:           grpcClient,
		TemporalClient: temporalClient,
		Hub:            NewHub(),
		TaskQueue:      taskQueue,
	}
}

func (svc *Service) Close() {
	if svc.TemporalClient != nil {
		svc.TemporalClient.Close()
	}
	if svc.GRPC != nil {
		svc.GRPC.Close()
	}
}