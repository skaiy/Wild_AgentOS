package api

import (
	"net/http"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/google/uuid"

	"github.com/agent-os/se-app/internal/types"
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
	taskID := c.Param("taskId")
	task, err := svc.MetaStore.GetTask(taskID)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "task not found"})
		return
	}
	c.JSON(http.StatusOK, task)
}

func (svc *Service) RetryTask(c *gin.Context) {
	taskID := c.Param("taskId")
	err := svc.MetaStore.UpdateTaskStatus(taskID, types.TaskStatusPending, "")
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"status": "retrying", "task_id": taskID})
}

func (svc *Service) RollbackTask(c *gin.Context) {
	taskID := c.Param("taskId")
	err := svc.MetaStore.UpdateTaskStatus(taskID, types.TaskStatusRolledBack, "")
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"status": "rolled_back", "task_id": taskID})
}

type StartPipelineResult struct{}

func (svc *Service) GetPipelineResult(c *gin.Context) {
	projectID := c.Param("id")
	tasks, err := svc.MetaStore.ListTasks(projectID)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "pipeline not found"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"project_id": projectID, "tasks": tasks})
}

func (svc *Service) ListStageResults(c *gin.Context) {
	taskID := c.Param("taskId")
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
	taskID := c.Param("taskId")
	stageID := c.Param("stageId")
	stage, err := svc.MetaStore.GetStageInstance(taskID, stageID)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "stage result not found"})
		return
	}
	c.JSON(http.StatusOK, stage)
}