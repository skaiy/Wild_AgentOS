package main

import (
	"log"
	"os"

	"github.com/agent-os/se-app/internal/config"
	"github.com/agent-os/se-app/internal/store"
	"github.com/agent-os/se-app/internal/workflow"
)

func main() {
	cfgPath := "config.yaml"
	if len(os.Args) > 1 {
		cfgPath = os.Args[1]
	}

	cfg, err := config.Load(cfgPath)
	if err != nil {
		log.Fatalf("load config: %v", err)
	}

	metaStore, err := store.NewSQLiteMetaStore(cfg.MetaStore.DSN)
	if err != nil {
		log.Fatalf("init metastore: %v", err)
	}

	log.Printf("worker starting: temporal=%s queue=%s grpc=%s", cfg.Temporal.HostPort, cfg.Temporal.TaskQueue, cfg.GRPC.Target)

	err = workflow.RunWorker(workflow.WorkerDeps{
		TemporalHost: cfg.Temporal.HostPort,
		TaskQueue:    cfg.Temporal.TaskQueue,
		GrpcTarget:   cfg.GRPC.Target,
		MetaStore:    metaStore,
	})
	if err != nil {
		log.Fatalf("worker error: %v", err)
	}
}