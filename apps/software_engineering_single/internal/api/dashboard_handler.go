package api

import (
	"net/http"
	"strings"

	"github.com/gin-gonic/gin"

	"github.com/agent-os/se-app/internal/types"
)

type UpdateProjectRequest struct {
	Name        *string `json:"name,omitempty"`
	Description *string `json:"description,omitempty"`
}

func (svc *Service) UpdateProject(c *gin.Context) {
	projectID := c.Param("id")

	var req UpdateProjectRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	name := ""
	if req.Name != nil {
		name = *req.Name
	}
	desc := ""
	if req.Description != nil {
		desc = *req.Description
	}

	if err := svc.MetaStore.UpdateProject(projectID, name, desc); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	project, err := svc.MetaStore.GetProject(projectID)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	c.JSON(http.StatusOK, gin.H{"project": project})
}

type UpdateTaskRequest struct {
	PipelineName *string `json:"pipeline_name,omitempty"`
}

func (svc *Service) UpdateTask(c *gin.Context) {
	taskID := c.Param("taskId")

	var req UpdateTaskRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	task, err := svc.MetaStore.GetTask(taskID)
	if err != nil {
		if strings.Contains(err.Error(), "not found") {
			c.JSON(http.StatusNotFound, gin.H{"error": "task not found"})
			return
		}
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	if req.PipelineName != nil {
		task.PipelineName = *req.PipelineName
	}

	if err := svc.MetaStore.UpdateTaskStatus(task.TaskID, task.Status, task.CurrentStage); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	c.JSON(http.StatusOK, gin.H{"task": task})
}

func (svc *Service) ListAllTasks(c *gin.Context) {
	statusFilter := c.Query("status")
	status := c.Query("project_id")

	var tasks []*types.TaskMeta
	var err error

	if status != "" {
		tasks, err = svc.MetaStore.ListTasks(status)
	} else if statusFilter != "" {
		taskStatus := types.TaskStatus(statusFilter)
		tasks, err = svc.MetaStore.SearchTasksByStatus(taskStatus)
	} else {
		tasks, err = svc.MetaStore.ListAllTasks()
	}

	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	c.JSON(http.StatusOK, gin.H{"tasks": tasks})
}

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

	stages, sErr := svc.MetaStore.ListAllStageInstances()
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

type PipelineTrend struct {
	Date    string `json:"date"`
	Success int    `json:"success"`
	Failed  int    `json:"failed"`
	Running int    `json:"running"`
}

func (svc *Service) GetPipelineTrends(c *gin.Context) {
	tasks, err := svc.MetaStore.ListAllTasks()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	trendsMap := make(map[string]*PipelineTrend)

	for _, t := range tasks {
		date := "unknown"
		if !t.StartedAt.IsZero() {
			date = t.StartedAt.Format("2006-01-02")
		}

		if _, exists := trendsMap[date]; !exists {
			trendsMap[date] = &PipelineTrend{Date: date}
		}

		switch t.Status {
		case types.TaskStatusCompleted:
			trendsMap[date].Success++
		case types.TaskStatusFailed:
			trendsMap[date].Failed++
		case types.TaskStatusRunning:
			trendsMap[date].Running++
		}
	}

	var trends []PipelineTrend
	for _, t := range trendsMap {
		trends = append(trends, *t)
	}

	if trends == nil {
		trends = []PipelineTrend{}
	}

	c.JSON(http.StatusOK, gin.H{"trends": trends})
}