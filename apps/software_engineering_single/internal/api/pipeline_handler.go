package api

import (
	"encoding/json"
	"net/http"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
	"go.temporal.io/sdk/client"

	pb "github.com/agent-os/se-app/proto/seapp"
	"github.com/agent-os/se-app/internal/types"
	"github.com/agent-os/se-app/internal/workflow/pipeline"
)

type StartPipelineRequest struct {
	ProjectName     string `json:"project_name" binding:"required"`
	ProjectDir      string `json:"project_dir"`
	UserRequirement string `json:"user_requirement" binding:"required"`
	Stages          []pipeline.StageBlock `json:"stages,omitempty"`
}

func (svc *Service) StartPipeline(c *gin.Context) {
	var req StartPipelineRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	projectID := uuid.New().String()
	taskID := uuid.New().String()
	now := time.Now()

	project := &types.ProjectMeta{
		ProjectID:   projectID,
		ProjectName: req.ProjectName,
		Description: req.UserRequirement,
		Status:      types.ProjectStatusRunning,
		CreatedAt:   now,
		UpdatedAt:   now,
	}
	if err := svc.MetaStore.CreateProject(project); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	stages := req.Stages
	if len(stages) == 0 {
		stages = defaultStages()
	}

	dsl := pipeline.SDLCDSL{
		Version: "1.0",
		Pipeline: pipeline.PipelineBlock{
			Name:        req.ProjectName,
			Description: req.UserRequirement,
			Stages:      stages,
		},
	}

	workflowOpts := client.StartWorkflowOptions{
		ID:        "sdlc-pipeline-" + taskID,
		TaskQueue: svc.TaskQueue,
	}

	exec, err := svc.TemporalClient.ExecuteWorkflow(c.Request.Context(), workflowOpts, "sdlc-workflow", dsl)
	if err != nil {
		svc.MetaStore.UpdateTaskStatus(taskID, types.TaskStatusFailed, "")
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	task := &types.TaskMeta{
		TaskID:       taskID,
		ProjectID:    projectID,
		PipelineName: req.ProjectName,
		Status:       types.TaskStatusRunning,
		CurrentStage: stages[0].ID,
		WorkflowID:   exec.GetID(),
		RunID:        exec.GetRunID(),
		StartedAt:    now,
	}
	if err := svc.MetaStore.CreateTask(task); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	for i, stage := range stages {
		svc.MetaStore.SaveStageInstance(taskID, &types.StageInstanceMeta{
			StageID:   stage.ID,
			Name:      stage.Name,
			StageType: types.StageType(stage.Type),
			Status:    types.StageStatusPending,
			Order:     i,
			StartedAt: now,
		})
	}

	stageMeta, _ := json.Marshal(&types.StageInstanceMeta{
		StageID: stages[0].ID,
		Status:  types.StageStatusRunning,
	})

	svc.Hub.Broadcast(projectID, Message{
		Type:    "stage_update",
		Payload: stageMeta,
	})

	c.JSON(http.StatusOK, gin.H{
		"project_id":  projectID,
		"task_id":     taskID,
		"workflow_id": exec.GetID(),
		"status":      "started",
	})
}

func defaultStages() []pipeline.StageBlock {
	return []pipeline.StageBlock{
		{ID: "requirement", Name: "需求分析", Type: "requirement", Timeout: "5m"},
		{ID: "design", Name: "系统设计", Type: "design", Timeout: "5m", AIReview: true, ContractSchema: "se:DesignShape"},
		{ID: "coding", Name: "编码实现", Type: "coding", Timeout: "10m"},
		{ID: "testing", Name: "测试验证", Type: "testing", Timeout: "5m", AIReview: true},
	}
}

func (svc *Service) GetProjectGraph(c *gin.Context) {
	projectID := c.Param("id")

	project, err := svc.MetaStore.GetProject(projectID)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "project not found"})
		return
	}

	tasks, err := svc.MetaStore.ListTasks(projectID)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	var frontendData interface{}
	if svc.GRPC != nil {
		flattenReq := &pb.FlattenRequest{
			Iri:       projectID,
			FrameType: "summary_only",
		}

		flattenResp, err := svc.GRPC.FlattenToFrontend(c.Request.Context(), flattenReq)
		if err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}

		if err := json.Unmarshal([]byte(flattenResp.FrontendJson), &frontendData); err != nil {
			frontendData = flattenResp.FrontendJson
		}
	}

	c.JSON(http.StatusOK, gin.H{
		"project": project,
		"tasks":   tasks,
		"graph":   frontendData,
	})
}

func (svc *Service) GetProjectSnapshot(c *gin.Context) {
	projectID := c.Param("id")
	taskID := c.Query("task_id")

	project, err := svc.MetaStore.GetProject(projectID)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "project not found"})
		return
	}

	if taskID == "" {
		tasks, err := svc.MetaStore.ListTasks(projectID)
		if err != nil {
			c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
			return
		}
		if len(tasks) == 0 {
			c.JSON(http.StatusNotFound, gin.H{"error": "no tasks found"})
			return
		}
		taskID = tasks[0].TaskID
	}

	task, err := svc.MetaStore.GetTask(taskID)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "task not found"})
		return
	}

	stages, err := svc.MetaStore.ListStageInstances(taskID)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	if stages == nil {
		stages = []*types.StageInstanceMeta{}
	}

	c.JSON(http.StatusOK, gin.H{
		"project": project,
		"task":    task,
		"stages":  stages,
	})
}