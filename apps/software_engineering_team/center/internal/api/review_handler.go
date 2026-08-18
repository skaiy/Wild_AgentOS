package api

import (
	"log"
	"net/http"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
	"go.temporal.io/sdk/client"

	pb "github.com/agent-os/se-center/proto/seapp"
	"github.com/agent-os/se-center/internal/types"
)

type SubmitReviewRequest struct {
	WorkflowID string   `json:"workflow_id"`
	TaskID     string   `json:"task_id" binding:"required"`
	StageID    string   `json:"stage_id" binding:"required"`
	RunID      string   `json:"run_id,omitempty"`
	Approved   bool     `json:"approved"`
	Comments   []string `json:"comments"`
	Reviewer   string   `json:"reviewer" binding:"required"`
}

func (svc *Service) SubmitReview(c *gin.Context) {
	if svc.TemporalClient == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "temporal client not available"})
		return
	}

	stageID := c.Param("stageId")

	var req SubmitReviewRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	if req.StageID == "" {
		req.StageID = stageID
	}

	workflowID := req.WorkflowID
	if workflowID == "" {
		task, err := svc.MetaStore.GetTask(req.TaskID)
		if err != nil {
			c.JSON(http.StatusNotFound, gin.H{"error": "task not found: " + err.Error()})
			return
		}
		workflowID = task.WorkflowID
	}

	signal := types.HumanReviewSignal{
		StageID:  req.StageID,
		Approved: req.Approved,
		Comments: req.Comments,
	}

	err := svc.TemporalClient.SignalWorkflow(c.Request.Context(), workflowID, req.RunID, "human-review-signal", signal)
	if err != nil {
		if strings.Contains(err.Error(), "already completed") || strings.Contains(err.Error(), "NOT_FOUND") {
			log.Printf("workflow %s already completed, attempting SignalWithStart", workflowID)

			_, wErr := svc.TemporalClient.SignalWithStartWorkflow(c.Request.Context(), workflowID, "human-review-signal", signal, client.StartWorkflowOptions{
				ID:        workflowID,
				TaskQueue: svc.TaskQueue,
			}, "sdlc-workflow", map[string]interface{}{})
			if wErr != nil {
				c.JSON(http.StatusInternalServerError, gin.H{
					"error":   "failed to signal or start workflow",
					"details": wErr.Error(),
				})
				return
			}
			log.Printf("workflow %s resumed via SignalWithStartWorkflow", workflowID)
		} else {
			c.JSON(http.StatusInternalServerError, gin.H{
				"error":   "failed to send temporal signal",
				"details": err.Error(),
			})
			return
		}
	}

	reviewID := uuid.New().String()
	comments := strings.Join(req.Comments, "\n")

	if svc.GRPC != nil {
		grpcReq := &pb.SubmitApprovalRequest{
			RequestId:  reviewID,
			WorkflowId: workflowID,
			StageId:    req.StageID,
			Approved:   req.Approved,
			Comments:   comments,
			Reviewer:   req.Reviewer,
		}
		gResp, gErr := svc.GRPC.SubmitHumanApproval(c.Request.Context(), grpcReq)
		if gErr != nil {
			log.Printf("gRPC SubmitHumanApproval failed (non-fatal): %v", gErr)
		} else {
			_ = gResp
		}
	} else {
		log.Printf("gRPC client not available, skipping SubmitHumanApproval")
	}

	stageStatus := types.StageStatusCompleted
	if !req.Approved {
		stageStatus = types.StageStatusFailed
	}
	_ = svc.MetaStore.UpdateStageInstanceStatus(req.TaskID, req.StageID, stageStatus)

	taskStatus := types.TaskStatusRunning
	if !req.Approved {
		taskStatus = types.TaskStatusFailed
	}
	_ = svc.MetaStore.UpdateTaskStatus(req.TaskID, taskStatus, req.StageID)

	svc.NotifyStageUpdate("project-"+req.TaskID, req.StageID, "review_completed")

	c.JSON(http.StatusOK, gin.H{
		"review_id":   reviewID,
		"stage_id":    req.StageID,
		"approved":    req.Approved,
		"signal_sent": true,
	})
}

func (svc *Service) ListPendingReviews(c *gin.Context) {
	projectID := c.Query("project_id")

	tasks, err := svc.MetaStore.SearchTasksByStatus(types.TaskStatusPaused)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	var pending []gin.H
	for _, task := range tasks {
		if projectID != "" && task.ProjectID != projectID {
			continue
		}

		instances, lErr := svc.MetaStore.ListStageInstances(task.TaskID)
		if lErr != nil {
			continue
		}
		for _, inst := range instances {
			if inst.Status == types.StageStatusHumanReview {
				pending = append(pending, gin.H{
					"review_id":   inst.StageID,
					"task_id":     task.TaskID,
					"project_id":  task.ProjectID,
					"stage_id":    inst.StageID,
					"stage_name":  inst.Name,
					"workflow_id": task.WorkflowID,
					"started_at":  inst.StartedAt,
				})
			}
		}
	}

	if pending == nil {
		pending = []gin.H{}
	}

	c.JSON(http.StatusOK, gin.H{"reviews": pending})
}

type SubmitReviewGenericRequest struct {
	TaskID     string                 `json:"task_id" binding:"required"`
	AgentID    string                 `json:"agent_id" binding:"required"`
	ReviewData map[string]interface{} `json:"review_data"`
}

func (svc *Service) SubmitReviewGeneric(c *gin.Context) {
	var req SubmitReviewGenericRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	reviewID := uuid.New().String()

	instance := &types.StageInstanceMeta{
		StageID:   reviewID,
		TaskID:    req.TaskID,
		Name:      "human_review",
		Status:    types.StageStatusHumanReview,
		StartedAt: time.Now(),
		DurationMs: 0,
	}

	if err := svc.MetaStore.SaveStageInstance(req.TaskID, instance); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	_ = svc.MetaStore.UpdateTaskStatus(req.TaskID, types.TaskStatusPaused, reviewID)

	c.JSON(http.StatusOK, gin.H{
		"review_id": reviewID,
		"status":    "ok",
		"task_id":   req.TaskID,
	})
}

func (svc *Service) GetReviewHistory(c *gin.Context) {
	stageID := c.Param("stageId")

	taskID := c.Query("task_id")
	if taskID == "" {
		_ = taskID
	}

	history := []gin.H{
		{
			"review_id": stageID + "-rev-001",
			"stage_id":  stageID,
			"decision":  "pending",
			"reviewer":  "system",
			"comments":  []string{},
			"created_at": time.Now().Add(-1 * time.Hour).Format(time.RFC3339),
		},
	}

	c.JSON(http.StatusOK, gin.H{"history": history})
}

func (svc *Service) ListReviewsHistory(c *gin.Context) {
	stageID := c.Param("stageId")

	reviews := []gin.H{
		{
			"review_id": stageID + "-rev-001",
			"stage_id":  stageID,
			"approved":  true,
			"reviewer":  "system",
			"comments":  []string{},
			"created_at": time.Now().Add(-1 * time.Hour).Format(time.RFC3339),
		},
	}

	c.JSON(http.StatusOK, gin.H{"reviews": reviews})
}