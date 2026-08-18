package api

import (
	"fmt"
	"net/http"
	"os"
	"runtime"
	"syscall"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/agent-os/se-app/internal/types"
)

func (svc *Service) GetSystemStatus(c *gin.Context) {
	temporalStatus := "connected"
	if svc.TemporalClient == nil {
		temporalStatus = "disconnected"
	}

	grpcStatus := "connected"
	if svc.GRPC == nil {
		grpcStatus = "disconnected"
	}

	_, err := svc.TemporalClient.CheckHealth(c.Request.Context(), nil)
	if err != nil {
		temporalStatus = "unhealthy"
	}

	c.JSON(http.StatusOK, gin.H{
		"agent_os": gin.H{
			"version": "2.0.0",
			"uptime":  time.Since(time.Now()).String(),
			"status":  "running",
		},
		"temporal": gin.H{
			"status":   temporalStatus,
			"host":     svc.Config.Temporal.HostPort,
			"queue":    svc.Config.Temporal.TaskQueue,
			"workflow": svc.Config.Temporal.TaskQueue,
		},
		"grpc": gin.H{
			"status": grpcStatus,
			"target": svc.Config.GRPC.Target,
		},
		"server": gin.H{
			"port": svc.Config.Server.Port,
		},
	})
}

func (svc *Service) GetSystemHealth(c *gin.Context) {
	temporalOk := true
	if svc.TemporalClient != nil {
		_, err := svc.TemporalClient.CheckHealth(c.Request.Context(), nil)
		if err != nil {
			temporalOk = false
		}
	} else {
		temporalOk = false
	}

	grpcOk := svc.GRPC != nil
	dbOk := svc.MetaStore != nil

	allOk := temporalOk && grpcOk && dbOk

	statusCode := http.StatusOK
	statusText := "healthy"
	if !allOk {
		statusCode = http.StatusServiceUnavailable
		statusText = "unhealthy"
	}

	c.JSON(statusCode, gin.H{
		"status":    statusText,
		"timestamp": time.Now().UTC().Format(time.RFC3339),
		"checks": gin.H{
			"temporal": temporalOk,
			"grpc":     grpcOk,
			"database": dbOk,
		},
	})
}

func (svc *Service) GetSystemResources(c *gin.Context) {
	var memStats runtime.MemStats
	runtime.ReadMemStats(&memStats)

	cpuCount := runtime.NumCPU()
	goRoutines := runtime.NumGoroutine()

	totalMemory := uint64(0)
	availableMemory := uint64(0)
	memInfo, err := os.ReadFile("/proc/meminfo")
	if err == nil {
		fmt.Sscanf(string(memInfo), "MemTotal: %d kB\nMemFree: %d kB\nMemAvailable: %d kB", &totalMemory, &availableMemory, &availableMemory)
	}

	totalDisk := uint64(0)
	availableDisk := uint64(0)
	var stat syscall.Statfs_t
	if err := syscall.Statfs("/", &stat); err == nil {
		totalDisk = stat.Blocks * uint64(stat.Bsize)
		availableDisk = stat.Bavail * uint64(stat.Bsize)
	}

	c.JSON(http.StatusOK, gin.H{
		"cpu": gin.H{
			"cores":       cpuCount,
			"used_percent": 0.0,
			"go_routines": goRoutines,
		},
		"memory": gin.H{
			"total_bytes":     totalMemory * 1024,
			"available_bytes": availableMemory * 1024,
			"used_bytes":      (totalMemory - availableMemory) * 1024,
			"go_alloc_bytes":  memStats.Alloc,
		},
		"disk": gin.H{
			"total_bytes":     totalDisk,
			"available_bytes": availableDisk,
			"used_bytes":      totalDisk - availableDisk,
		},
	})
}

func (svc *Service) GetActiveTasks(c *gin.Context) {
	tasks, err := svc.MetaStore.ListAllTasks()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	var activeTasks []gin.H
	for _, t := range tasks {
		if t.Status == types.TaskStatusRunning || t.Status == types.TaskStatusPending || t.Status == types.TaskStatusPaused {
			item := gin.H{
				"task_id":     t.TaskID,
				"project_id":  t.ProjectID,
				"pipeline":    t.PipelineName,
				"status":      t.Status,
				"stage":       t.CurrentStage,
				"started_at":  t.StartedAt,
				"workflow_id": t.WorkflowID,
			}
			if t.Error != "" {
				item["error"] = t.Error
			}
			activeTasks = append(activeTasks, item)
		}
	}

	if activeTasks == nil {
		activeTasks = []gin.H{}
	}

	c.JSON(http.StatusOK, gin.H{
		"active_tasks": activeTasks,
		"total":        len(activeTasks),
	})
}