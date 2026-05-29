package api

import (
	"github.com/agent-os/se-app/internal/config"
	"github.com/agent-os/se-app/internal/grpc"
	"github.com/agent-os/se-app/internal/types"
	"go.temporal.io/sdk/client"
)

type Service struct {
	Config         *config.Config
	MetaStore      types.MetaStore
	GRPC           *grpc.Client
	TemporalClient client.Client
	Hub            *Hub
	TaskQueue      string
}

func NewService(cfg *config.Config, metaStore types.MetaStore, grpcClient *grpc.Client, temporalClient client.Client, taskQueue string) *Service {
	return &Service{
		Config:         cfg,
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
