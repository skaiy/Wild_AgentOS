package api

import (
	"fmt"
	"net/http"
	"time"

	"github.com/gin-gonic/gin"
)

type logEntry struct {
	Timestamp string `json:"timestamp"`
	Level     string `json:"level"`
	Source    string `json:"source"`
	Message   string `json:"message"`
	StageID   string `json:"stage_id,omitempty"`
	TaskID    string `json:"task_id,omitempty"`
}

func generateMockLogs(count int, source string, filterStageID string, filterTaskID string) []logEntry {
	levels := []string{"INFO", "WARN", "ERROR", "DEBUG"}
	sources := []string{"pipeline", "temporal", "grpc", "api", "worker"}
	messages := map[string][]string{
		"pipeline": {
			"Stage started: requirement analysis",
			"Stage completed: design review",
			"Pipeline execution initiated",
			"Workflow task dispatched",
			"Stage transition: coding -> testing",
			"Contract validation passed",
			"Human review requested for stage",
		},
		"temporal": {
			"Workflow task scheduled",
			"Activity task completed",
			"Workflow execution started",
			"Timer fired for retry policy",
			"Signal received: human-review-signal",
		},
		"grpc": {
			"FlattenToFrontend call succeeded",
			"SubmitHumanApproval called",
			"gRPC connection established",
			"Stream closed by remote peer",
		},
		"api": {
			"GET /api/v1/projects 200",
			"POST /api/v1/pipelines 201",
			"GET /api/v1/stats 200",
			"WebSocket connection opened",
		},
		"worker": {
			"Processing stage: coding",
			"AI review completed for stage",
			"Artifact generated: source code",
			"Running test suite",
		},
	}

	entries := make([]logEntry, 0, count)
	now := time.Now()

	for i := 0; i < count; i++ {
		src := source
		if src == "" {
			src = sources[i%len(sources)]
		}

		srcMessages := messages[src]
		if srcMessages == nil {
			srcMessages = messages["pipeline"]
		}

		msg := srcMessages[i%len(srcMessages)]
		level := levels[i%len(levels)]

		entry := logEntry{
			Timestamp: now.Add(-time.Duration(i) * 30 * time.Second).Format(time.RFC3339),
			Level:     level,
			Source:    src,
			Message:   msg,
		}

		if filterStageID != "" {
			entry.StageID = filterStageID
		}
		if filterTaskID != "" {
			entry.TaskID = filterTaskID
		}

		entries = append(entries, entry)
	}

	return entries
}

func (svc *Service) GetSystemLogs(c *gin.Context) {
	limit := 100
	level := c.Query("level")
	source := c.Query("source")

	logs := generateMockLogs(limit, "", "", "")

	var filtered []logEntry
	for _, entry := range logs {
		if level != "" && entry.Level != level {
			continue
		}
		if source != "" && entry.Source != source {
			continue
		}
		filtered = append(filtered, entry)
	}

	if filtered == nil {
		filtered = []logEntry{}
	}

	c.JSON(http.StatusOK, gin.H{
		"logs":  filtered,
		"total": len(filtered),
	})
}

func (svc *Service) GetStageLogs(c *gin.Context) {
	taskID := c.Param("taskId")
	stageID := c.Param("stageId")

	logs := generateMockLogs(50, "pipeline", stageID, taskID)

	if logs == nil {
		logs = []logEntry{}
	}

	c.JSON(http.StatusOK, gin.H{
		"task_id":  taskID,
		"stage_id": stageID,
		"logs":     logs,
		"total":    len(logs),
	})
}

func (svc *Service) GetAgentOSLogs(c *gin.Context) {
	limit := 100
	level := c.Query("level")

	sources := []string{"agent-os", "orchestrator", "scheduler", "monitor"}
	mockMessages := map[string][]string{
		"agent-os": {
			"Agent OS started successfully",
			"Agent OS shutting down",
			"Health check passed",
			"Configuration reloaded",
			"Module registered: pipeline-orchestrator",
		},
		"orchestrator": {
			"Pipeline orchestration started",
			"Stage dependency resolved",
			"Parallel stage execution initiated",
			"Orchestrator state persisted",
		},
		"scheduler": {
			"Task scheduled for execution",
			"Schedule interval updated",
			"Task queue drained",
			"Cron trigger fired",
		},
		"monitor": {
			"Resource usage snapshot taken",
			"Threshold alert: CPU > 80%",
			"System metrics collected",
			"Monitoring interval: 30s",
		},
	}

	levels := []string{"INFO", "WARN", "INFO", "DEBUG", "ERROR"}
	entries := make([]logEntry, 0, limit)
	now := time.Now()

	for i := 0; i < limit; i++ {
		src := sources[i%len(sources)]
		srcMessages := mockMessages[src]
		msg := srcMessages[i%len(srcMessages)]
		lvl := levels[i%len(levels)]

		entry := logEntry{
			Timestamp: now.Add(-time.Duration(i) * 60 * time.Second).Format(time.RFC3339),
			Level:     lvl,
			Source:    src,
			Message:   fmt.Sprintf("[%s] %s", src, msg),
		}
		entries = append(entries, entry)
	}

	var filtered []logEntry
	for _, entry := range entries {
		if level != "" && entry.Level != level {
			continue
		}
		filtered = append(filtered, entry)
	}

	if filtered == nil {
		filtered = []logEntry{}
	}

	c.JSON(http.StatusOK, gin.H{
		"logs":  filtered,
		"total": len(filtered),
	})
}