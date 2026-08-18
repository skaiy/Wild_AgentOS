package main

import (
	"context"
	"fmt"
	"log"
	"time"

	"github.com/agent-os/se-center/internal/agent"
	"github.com/agent-os/se-center/internal/api"
	"github.com/agent-os/se-center/internal/config"
	grpcclient "github.com/agent-os/se-center/internal/grpc"
	"github.com/agent-os/se-center/internal/store"
	"go.temporal.io/sdk/client"
)

func main() {
	const configPath = "config.yaml"

	cfg, err := config.Load(configPath)
	if err != nil {
		log.Fatalf("failed to load config: %v", err)
	}

	metaStore, err := store.NewSQLiteMetaStore(cfg.Meta.DSN)
	if err != nil {
		log.Fatalf("failed to init MetaStore: %v", err)
	}
	log.Printf("MetaStore connected: %s", cfg.Meta.DSN)

	var temporalClient client.Client
	temporalAddr := fmt.Sprintf("%s:%d", cfg.Temporal.Host, cfg.Temporal.Port)
	temporalClient, err = client.Dial(client.Options{
		HostPort: temporalAddr,
	})
	if err != nil {
		log.Printf("WARNING: Temporal client connection failed (running without Temporal): %v", err)
		temporalClient = nil
	} else {
		log.Printf("Temporal client connected to %s", temporalAddr)
	}

	var grpcCli *grpcclient.Client
	grpcTarget := fmt.Sprintf("%s:%d", cfg.GRPC.Host, cfg.GRPC.Port)
	grpcCli, err = grpcclient.NewClient(grpcTarget)
	if err != nil {
		log.Printf("WARNING: gRPC client connection failed (running without gRPC): %v", err)
		grpcCli = nil
	} else {
		log.Printf("gRPC client connected to %s", grpcTarget)
	}

	svc := api.NewService(cfg, configPath, metaStore, grpcCli, temporalClient, cfg.Temporal.TaskQueue)

	agentStore := agent.NewInMemoryAgentStore()
	svc.AgentManager = agent.NewAgentManagerWithConfig(agentStore, 10*time.Second, 5*time.Second)

	svc.AgentManager.StartTimeoutScanner(context.Background())
	defer svc.AgentManager.StopTimeoutScanner()

	router := api.SetupRouter(svc)

	addr := fmt.Sprintf("%s:%d", cfg.Server.Host, cfg.Server.Port)
	log.Printf("center server starting on %s", addr)
	if err := router.Run(addr); err != nil {
		log.Fatalf("failed to start server: %v", err)
	}
}