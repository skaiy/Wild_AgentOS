package api

import (
	"net/http"
	"time"

	"github.com/gin-gonic/gin"
)

type LogEntry struct {
	Timestamp string `json:"timestamp"`
	Level     string `json:"level"`
	Message   string `json:"message"`
	Source    string `json:"source"`
}

func (svc *Service) GetSystemLogs(c *gin.Context) {
	level := c.Query("level")
	_ = level

	logs := []LogEntry{
		{
			Timestamp: time.Now().Add(-5 * time.Minute).Format(time.RFC3339),
			Level:     "info",
			Message:   "Center server started successfully",
			Source:    "system",
		},
		{
			Timestamp: time.Now().Add(-4 * time.Minute).Format(time.RFC3339),
			Level:     "info",
			Message:   "API routes registered: 29 endpoints",
			Source:    "system",
		},
	}

	c.JSON(http.StatusOK, gin.H{"logs": logs})
}

func (svc *Service) GetStageLogs(c *gin.Context) {
	taskID := c.Param("taskId")
	stageID := c.Param("stageId")

	logs := []LogEntry{
		{
			Timestamp: time.Now().Format(time.RFC3339),
			Level:     "info",
			Message:   "Stage " + stageID + " started for task " + taskID,
			Source:    "stage",
		},
	}

	c.JSON(http.StatusOK, gin.H{"logs": logs})
}

func (svc *Service) GetAgentOSLogs(c *gin.Context) {
	since := c.Query("since")
	_ = since

	logs := []LogEntry{
		{
			Timestamp: time.Now().Add(-10 * time.Minute).Format(time.RFC3339),
			Level:     "info",
			Message:   "Agent OS kernel connected",
			Source:    "agent-os",
		},
		{
			Timestamp: time.Now().Add(-5 * time.Minute).Format(time.RFC3339),
			Level:     "info",
			Message:   "Knowledge graph sync completed",
			Source:    "agent-os",
		},
	}

	c.JSON(http.StatusOK, gin.H{"logs": logs})
}