package api

import (
	"net/http"
	"runtime"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/agent-os/se-center/internal/types"
)

type SystemStatus struct {
	Status    string `json:"status"`
	Version   string `json:"version"`
	Uptime    string `json:"uptime"`
	StartTime string `json:"start_time"`
}

type SystemResources struct {
	CPUPercent  float64 `json:"cpu_percent"`
	MemoryUsed  int64   `json:"memory_used_mb"`
	MemoryTotal int64   `json:"memory_total_mb"`
	DiskUsed    int64   `json:"disk_used_gb"`
	DiskTotal   int64   `json:"disk_total_gb"`
	GoVersion   string  `json:"go_version"`
	Goroutines  int     `json:"goroutines"`
}

type HealthStatus struct {
	Status   string                 `json:"status"`
	Version  string                 `json:"version"`
	Uptime   string                 `json:"uptime"`
	Services map[string]interface{} `json:"services"`
}

var serverStartTime = time.Now()

func (svc *Service) GetSystemHealth(c *gin.Context) {
	uptime := time.Since(serverStartTime).String()

	services := map[string]interface{}{
		"meta_store": map[string]interface{}{
			"healthy": svc.MetaStore != nil,
		},
		"temporal": map[string]interface{}{
			"healthy": svc.TemporalClient != nil,
		},
		"grpc": map[string]interface{}{
			"healthy": svc.GRPC != nil,
		},
	}

	if svc.Config != nil {
		services["llm"] = map[string]interface{}{
			"healthy":  svc.Config.LLM.APIKey != "",
			"provider": svc.Config.LLM.Provider,
			"model":    svc.Config.LLM.Model,
		}
	} else {
		services["llm"] = map[string]interface{}{
			"healthy": false,
		}
	}

	c.JSON(http.StatusOK, HealthStatus{
		Status:   "ok",
		Version:  "1.0.0",
		Uptime:   uptime,
		Services: services,
	})
}

func (svc *Service) GetSystemStatus(c *gin.Context) {
	uptime := time.Since(serverStartTime).String()
	c.JSON(http.StatusOK, SystemStatus{
		Status:    "running",
		Version:   "1.0.0",
		Uptime:    uptime,
		StartTime: serverStartTime.Format(time.RFC3339),
	})
}

func (svc *Service) GetSystemResources(c *gin.Context) {
	var m runtime.MemStats
	runtime.ReadMemStats(&m)

	c.JSON(http.StatusOK, SystemResources{
		CPUPercent:  0.0,
		MemoryUsed:  int64(m.Alloc / 1024 / 1024),
		MemoryTotal: int64(m.TotalAlloc / 1024 / 1024),
		DiskUsed:    0,
		DiskTotal:   0,
		GoVersion:   runtime.Version(),
		Goroutines:  runtime.NumGoroutine(),
	})
}

func (svc *Service) GetActiveTasks(c *gin.Context) {
	tasks, err := svc.MetaStore.SearchTasksByStatus(types.TaskStatusRunning)
	if err != nil {
		c.JSON(http.StatusOK, gin.H{"activeTasks": []interface{}{}})
		return
	}
	if tasks == nil {
		c.JSON(http.StatusOK, gin.H{"activeTasks": []interface{}{}})
		return
	}
	c.JSON(http.StatusOK, gin.H{"activeTasks": tasks})
}