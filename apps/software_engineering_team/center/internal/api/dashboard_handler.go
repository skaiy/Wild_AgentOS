package api

import (
	"net/http"

	"github.com/gin-gonic/gin"

	"github.com/agent-os/se-center/internal/config"
	"github.com/agent-os/se-center/internal/types"
)

func (svc *Service) GetStats(c *gin.Context) {
	projects, pErr := svc.MetaStore.ListProjects(nil)
	if pErr != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": pErr.Error()})
		return
	}

	tasks, tErr := svc.MetaStore.ListAllTasks()
	if tErr != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": tErr.Error()})
		return
	}

	stages, sErr := svc.MetaStore.ListAllStageInstances(0, 1000)
	if sErr != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": sErr.Error()})
		return
	}

	projectCount := len(projects)
	taskCount := len(tasks)
	runningTasks := 0
	completedTasks := 0
	failedTasks := 0
	pendingReviews := 0

	for _, t := range tasks {
		switch t.Status {
		case types.TaskStatusRunning:
			runningTasks++
		case types.TaskStatusCompleted:
			completedTasks++
		case types.TaskStatusFailed:
			failedTasks++
		}
	}

	for _, s := range stages {
		if s.HumanReviewPassed == nil && s.Status == types.StageStatusRunning {
			pendingReviews++
		}
	}

	c.JSON(http.StatusOK, gin.H{
		"project_count":   projectCount,
		"task_count":      taskCount,
		"running_tasks":   runningTasks,
		"completed_tasks": completedTasks,
		"failed_tasks":    failedTasks,
		"pending_reviews": pendingReviews,
	})
}

func (svc *Service) GetActivity(c *gin.Context) {
	tasks, tErr := svc.MetaStore.ListAllTasks()
	if tErr != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": tErr.Error()})
		return
	}

	var activities []gin.H

	for i, t := range tasks {
		if i >= 20 {
			break
		}
		item := gin.H{
			"type":         "task",
			"task_id":      t.TaskID,
			"project_id":   t.ProjectID,
			"pipeline":     t.PipelineName,
			"status":       t.Status,
			"stage":        t.CurrentStage,
			"started_at":   t.StartedAt,
			"completed_at": t.CompletedAt,
		}
		if t.Error != "" {
			item["error"] = t.Error
		}
		activities = append(activities, item)
	}

	if activities == nil {
		activities = []gin.H{}
	}

	c.JSON(http.StatusOK, gin.H{"activities": activities})
}

func (svc *Service) GetLLMConfig(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{
		"provider": svc.Config.LLM.Provider,
		"model":    svc.Config.LLM.Model,
		"base_url": svc.Config.LLM.BaseURL,
	})
}

type UpdateLLMConfigRequest struct {
	Provider *string `json:"provider,omitempty"`
	Model    *string `json:"model,omitempty"`
	BaseURL  *string `json:"base_url,omitempty"`
	APIKey   *string `json:"api_key,omitempty"`
}

func (svc *Service) UpdateLLMConfig(c *gin.Context) {
	var req UpdateLLMConfigRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	if req.Provider != nil {
		svc.Config.LLM.Provider = *req.Provider
	}
	if req.Model != nil {
		svc.Config.LLM.Model = *req.Model
	}
	if req.BaseURL != nil {
		svc.Config.LLM.BaseURL = *req.BaseURL
	}
	if req.APIKey != nil {
		svc.Config.LLM.APIKey = *req.APIKey
	}

	if err := config.Save(svc.Config, svc.ConfigPath); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "persist config: " + err.Error()})
		return
	}

	c.JSON(http.StatusOK, gin.H{
		"provider": svc.Config.LLM.Provider,
		"model":    svc.Config.LLM.Model,
		"base_url": svc.Config.LLM.BaseURL,
	})
}