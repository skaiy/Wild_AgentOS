package api

import (
	"net/http"
	"strconv"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/google/uuid"

	"github.com/agent-os/se-center/internal/types"
)

type CreateProjectRequest struct {
	ProjectName string `json:"project_name" binding:"required"`
	Description string `json:"description"`
}

func (svc *Service) CreateProject(c *gin.Context) {
	var req CreateProjectRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	project := &types.ProjectMeta{
		ProjectID:   "proj_" + uuid.New().String(),
		ProjectName: req.ProjectName,
		Description: req.Description,
		Status:      types.ProjectStatusInit,
		CreatedAt:   time.Now(),
		UpdatedAt:   time.Now(),
	}

	if err := svc.MetaStore.CreateProject(project); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	c.JSON(http.StatusOK, project)
}

func (svc *Service) GetProject(c *gin.Context) {
	id := c.Param("id")
	project, err := svc.MetaStore.GetProject(id)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "project not found"})
		return
	}
	c.JSON(http.StatusOK, project)
}

func (svc *Service) ListProjects(c *gin.Context) {
	filter := make(map[string]interface{})
	if status := c.Query("status"); status != "" {
		filter["status"] = status
	}
	projects, err := svc.MetaStore.ListProjects(filter)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	if projects == nil {
		projects = []*types.ProjectMeta{}
	}

	limitStr := c.Query("limit")
	offsetStr := c.Query("offset")
	if limitStr != "" || offsetStr != "" {
		limit := len(projects)
		offset := 0
		if limitStr != "" {
			if v, err := strconv.Atoi(limitStr); err == nil && v > 0 {
				limit = v
			}
		}
		if offsetStr != "" {
			if v, err := strconv.Atoi(offsetStr); err == nil && v >= 0 {
				offset = v
			}
		}
		if offset >= len(projects) {
			projects = []*types.ProjectMeta{}
		} else {
			end := offset + limit
			if end > len(projects) {
				end = len(projects)
			}
			projects = projects[offset:end]
		}
	}

	c.JSON(http.StatusOK, gin.H{"projects": projects})
}

func (svc *Service) DeleteProject(c *gin.Context) {
	id := c.Param("id")
	if err := svc.MetaStore.DeleteProject(id); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"status": "deleted"})
}

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

type CreateTaskRequest struct {
	TaskID       string `json:"task_id,omitempty"`
	PipelineName string `json:"pipeline_name" binding:"required"`
}

func (svc *Service) CreateTask(c *gin.Context) {
	projectID := c.Param("id")
	var req CreateTaskRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	taskID := req.TaskID
	if taskID == "" {
		taskID = "task_" + uuid.New().String()
	}

	task := &types.TaskMeta{
		TaskID:       taskID,
		ProjectID:    projectID,
		PipelineName: req.PipelineName,
		Status:       types.TaskStatusPending,
		StartedAt:    time.Now(),
	}

	if err := svc.MetaStore.CreateTask(task); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	c.JSON(http.StatusOK, task)
}

func (svc *Service) ListTasks(c *gin.Context) {
	projectID := c.Param("id")
	tasks, err := svc.MetaStore.ListTasks(projectID)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	if tasks == nil {
		tasks = []*types.TaskMeta{}
	}
	c.JSON(http.StatusOK, gin.H{"tasks": tasks})
}

func (svc *Service) GetTask(c *gin.Context) {
	taskID := c.Param("id")
	task, err := svc.MetaStore.GetTask(taskID)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "task not found"})
		return
	}
	c.JSON(http.StatusOK, task)
}

func (svc *Service) RetryTask(c *gin.Context) {
	taskID := c.Param("id")
	err := svc.MetaStore.UpdateTaskStatus(taskID, types.TaskStatusPending, "")
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"status": "retrying", "task_id": taskID})
}

func (svc *Service) RollbackTask(c *gin.Context) {
	taskID := c.Param("id")
	err := svc.MetaStore.UpdateTaskStatus(taskID, types.TaskStatusRolledBack, "")
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"status": "rolled_back", "task_id": taskID})
}

type UpdateTaskRequest struct {
	PipelineName *string `json:"pipeline_name,omitempty"`
}

func (svc *Service) UpdateTask(c *gin.Context) {
	taskID := c.Param("id")

	var req UpdateTaskRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	task, err := svc.MetaStore.GetTask(taskID)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "task not found"})
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
	projectID := c.Query("project_id")

	var tasks []*types.TaskMeta
	var err error

	if projectID != "" {
		tasks, err = svc.MetaStore.ListTasks(projectID)
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

func (svc *Service) ListStageResults(c *gin.Context) {
	taskID := c.Param("id")
	stages, err := svc.MetaStore.ListStageInstances(taskID)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "stage results not found"})
		return
	}
	if stages == nil {
		stages = []*types.StageInstanceMeta{}
	}
	c.JSON(http.StatusOK, gin.H{"stages": stages})
}

func (svc *Service) GetStageResult(c *gin.Context) {
	taskID := c.Param("id")
	stageID := c.Param("stageId")
	stage, err := svc.MetaStore.GetStageInstance(taskID, stageID)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "stage result not found"})
		return
	}
	c.JSON(http.StatusOK, stage)
}

func (svc *Service) GetPipelineResult(c *gin.Context) {
	projectID := c.Param("id")
	tasks, err := svc.MetaStore.ListTasks(projectID)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "pipeline not found"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"project_id": projectID, "tasks": tasks})
}