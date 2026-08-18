package main

import (
	"fmt"
	"log"
	"net/http"
	"os"
	"os/signal"
	"syscall"

	"go.temporal.io/sdk/client"

	"github.com/agent-os/se-app/internal/api"
	"github.com/agent-os/se-app/internal/config"
	"github.com/agent-os/se-app/internal/grpc"
	"github.com/agent-os/se-app/internal/store"
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
	log.Printf("metastore initialized: %s", cfg.MetaStore.DSN)

	grpcClient, err := grpc.NewClient(cfg.GRPC.Target)
	if err != nil {
		log.Printf("warning: gRPC client init failed: %v (running in degraded mode)", err)
	}
	defer func() {
		if grpcClient != nil {
			grpcClient.Close()
		}
	}()

	temporalClient, err := client.Dial(client.Options{
		HostPort: cfg.Temporal.HostPort,
	})
	if err != nil {
		log.Fatalf("init temporal client: %v", err)
	}
	defer temporalClient.Close()

	svc := api.NewService(cfg, metaStore, grpcClient, temporalClient, cfg.Temporal.TaskQueue)
	defer svc.Close()

	router := api.SetupRouter(svc)

	srv := &http.Server{
		Addr:    fmt.Sprintf(":%d", cfg.Server.Port),
		Handler: router,
	}

	go func() {
		log.Printf("server starting on :%d", cfg.Server.Port)
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Fatalf("server error: %v", err)
		}
	}()

	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)
	<-quit
	log.Println("server shutting down")

	srv.Close()
}