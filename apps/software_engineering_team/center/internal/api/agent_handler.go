package api

import (
	"encoding/json"
	"log"
	"net/http"
	"time"

	"github.com/gin-gonic/gin"

	"github.com/agent-os/se-center/internal/agent"
	"github.com/agent-os/se-center/internal/types"
)

type RegisterAgentRequest struct {
	AgentID      string   `json:"agent_id" binding:"required"`
	UserID       string   `json:"user_id" binding:"required"`
	Capabilities []string `json:"capabilities"`
	Version      string   `json:"version"`
}

func (svc *Service) RegisterAgent(c *gin.Context) {
	var req RegisterAgentRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	info := &agent.AgentInfo{
		AgentID:      req.AgentID,
		UserID:       req.UserID,
		Capabilities: req.Capabilities,
		Version:      req.Version,
	}

	if err := svc.AgentManager.Register(info); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	c.JSON(http.StatusOK, gin.H{
		"agent_id":  info.AgentID,
		"status":    info.Status,
		"created_at": info.RegisteredAt,
	})
}

type HeartbeatRequest struct {
	AgentID string `json:"agent_id" binding:"required"`
}

func (svc *Service) AgentHeartbeat(c *gin.Context) {
	var req HeartbeatRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	if err := svc.AgentManager.Heartbeat(req.AgentID); err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": err.Error()})
		return
	}

	c.JSON(http.StatusOK, gin.H{
		"agent_id":  req.AgentID,
		"status":   "ok",
	})
}

func (svc *Service) ListAgents(c *gin.Context) {
	agents, err := svc.AgentManager.GetOnlineAgents()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	if agents == nil {
		agents = []*agent.AgentInfo{}
	}

	c.JSON(http.StatusOK, gin.H{"agents": agents})
}

func (svc *Service) GetAvailableTasks(c *gin.Context) {
	capability := c.Query("capability")

	var tasks []*types.TaskMeta
	var err error

	if capability != "" {
		matched := svc.AgentManager.MatchAgent(capability)
		if matched == nil {
			c.JSON(http.StatusOK, gin.H{"tasks": []types.TaskMeta{}})
			return
		}
	}

	tasks, err = svc.MetaStore.SearchTasksByStatus(types.TaskStatusPending)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	if tasks == nil {
		tasks = []*types.TaskMeta{}
	}

	c.JSON(http.StatusOK, gin.H{"tasks": tasks})
}

type ClaimTaskRequest struct {
	AgentID    string `json:"agent_id" binding:"required"`
	WorkflowID string `json:"workflow_id"`
}

func (svc *Service) ClaimTask(c *gin.Context) {
	taskID := c.Param("id")

	if svc.TemporalClient == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "temporal client not available"})
		return
	}

	var req ClaimTaskRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	task, err := svc.MetaStore.GetTask(taskID)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "task not found"})
		return
	}

	if task.Status != types.TaskStatusPending {
		c.JSON(http.StatusConflict, gin.H{"error": "task is not in pending status"})
		return
	}

	workflowID := req.WorkflowID
	if workflowID == "" {
		workflowID = task.WorkflowID
	}

	signal := map[string]interface{}{
		"task_id":  taskID,
		"agent_id": req.AgentID,
		"claimed":  true,
		"time":     time.Now(),
	}

	err = svc.TemporalClient.SignalWorkflow(c.Request.Context(), workflowID, task.RunID, "task-claim-signal", signal)
	if err != nil {
		log.Printf("failed to signal workflow %s for claim: %v", workflowID, err)
		c.JSON(http.StatusServiceUnavailable, gin.H{
			"error":   "failed to signal workflow",
			"details": err.Error(),
		})
		return
	}

	_ = svc.MetaStore.UpdateTaskStatus(taskID, types.TaskStatusRunning, task.CurrentStage)

	svc.Hub.Broadcast(task.ProjectID, Message{
		Type:    "task_claimed",
		Payload: json.RawMessage(`{"task_id":"`+taskID+`","agent_id":"`+req.AgentID+`"}`),
	})

	c.JSON(http.StatusOK, gin.H{
		"task_id":     taskID,
		"agent_id":    req.AgentID,
		"status":      "claimed",
		"workflow_id": workflowID,
	})
}

type TaskCallbackRequest struct {
	AgentID   string                 `json:"agent_id" binding:"required"`
	StageID   string                 `json:"stage_id" binding:"required"`
	Status    string                 `json:"status" binding:"required"`
	Summary   string                 `json:"summary"`
	Output    map[string]interface{} `json:"output,omitempty"`
	Artifacts []string               `json:"artifacts,omitempty"`
	Errors    []string               `json:"errors,omitempty"`
	RunID     string                 `json:"run_id,omitempty"`
}

func (svc *Service) TaskCallback(c *gin.Context) {
	taskID := c.Param("id")

	if svc.TemporalClient == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "temporal client not available"})
		return
	}

	var req TaskCallbackRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	task, err := svc.MetaStore.GetTask(taskID)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": "task not found"})
		return
	}

	if req.StageID != "" {
		stageStatus := types.StageInstanceStatus(req.Status)
		_ = svc.MetaStore.UpdateStageInstanceStatus(taskID, req.StageID, stageStatus)
	}

	signal := map[string]interface{}{
		"task_id":   taskID,
		"agent_id":  req.AgentID,
		"stage_id":  req.StageID,
		"status":    req.Status,
		"summary":   req.Summary,
		"output":    req.Output,
		"artifacts": req.Artifacts,
		"errors":    req.Errors,
		"time":      time.Now(),
	}

	err = svc.TemporalClient.SignalWorkflow(c.Request.Context(), task.WorkflowID, req.RunID, "stage-callback-signal", signal)
	if err != nil {
		log.Printf("failed to signal workflow %s for callback: %v", task.WorkflowID, err)
		c.JSON(http.StatusServiceUnavailable, gin.H{
			"error":   "failed to signal workflow",
			"details": err.Error(),
		})
		return
	}

	if req.Status == "completed" || req.Status == "failed" {
		if req.Status == "failed" {
			_ = svc.MetaStore.UpdateTaskStatus(taskID, types.TaskStatusFailed, req.StageID)
		}
	}

	svc.NotifyStageUpdate(task.ProjectID, req.StageID, req.Status)

	c.JSON(http.StatusOK, gin.H{
		"task_id":  taskID,
		"stage_id": req.StageID,
		"status":   req.Status,
	})
}