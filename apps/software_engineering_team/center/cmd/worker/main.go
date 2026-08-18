package main

import (
	"fmt"
	"log"

	"github.com/agent-os/se-center/internal/config"
)

func main() {
	cfg, err := config.Load("config.yaml")
	if err != nil {
		log.Fatalf("failed to load config: %v", err)
	}
	fmt.Printf("center worker starting, temporal: %s\n", cfg.Temporal.Host)
}