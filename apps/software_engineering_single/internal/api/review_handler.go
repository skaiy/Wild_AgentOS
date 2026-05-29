package api

import (
	"log"
	"net/http"
	"strings"

	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
	"go.temporal.io/sdk/client"

	pb "github.com/agent-os/se-app/proto/seapp"
	"github.com/agent-os/se-app/internal/types"
	pipelinepkg "github.com/agent-os/se-app/internal/workflow/pipeline"
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
	var req SubmitReviewRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
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

			dsl := pipelinepkg.SDLCDSL{
				Version: "1.0",
				Pipeline: pipelinepkg.PipelineBlock{
					Name:        "resume-" + workflowID,
					Description: "resumed by human review",
					Stages:      []pipelinepkg.StageBlock{},
					Options:     &pipelinepkg.OptionsBlock{},
				},
			}

			_, wErr := svc.TemporalClient.SignalWithStartWorkflow(c.Request.Context(), workflowID, "human-review-signal", signal, client.StartWorkflowOptions{
				ID:        workflowID,
				TaskQueue: svc.TaskQueue,
			}, "sdlc-workflow", dsl)
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

	grpcReq := &pb.SubmitApprovalRequest{
		RequestId:  reviewID,
		WorkflowId: workflowID,
		StageId:    req.StageID,
		Approved:   req.Approved,
		Comments:   comments,
		Reviewer:   req.Reviewer,
	}

	if svc.GRPC != nil {
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
					"task_id":    task.TaskID,
					"project_id": task.ProjectID,
					"stage_id":   inst.StageID,
					"stage_name": inst.Name,
					"workflow_id": task.WorkflowID,
					"started_at": inst.StartedAt,
				})
			}
		}
	}

	if pending == nil {
		pending = []gin.H{}
	}

	c.JSON(http.StatusOK, gin.H{"reviews": pending})
}

func (svc *Service) GetReviewHistory(c *gin.Context) {
	stageID := c.Param("stageId")
	projectID := c.Query("project_id")

	var tasks []*types.TaskMeta
	var err error

	if projectID != "" {
		tasks, err = svc.MetaStore.ListTasks(projectID)
	} else if stageID != "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "project_id query parameter is required"})
		return
	} else {
		c.JSON(http.StatusBadRequest, gin.H{"error": "project_id query parameter is required"})
		return
	}

	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	var history []gin.H
	for _, task := range tasks {
		instances, lErr := svc.MetaStore.ListStageInstances(task.TaskID)
		if lErr != nil {
			continue
		}
		for _, inst := range instances {
			if stageID != "" && inst.StageID != stageID {
				continue
			}
			if inst.AiReviewPassed != nil || inst.HumanReviewPassed != nil {
				item := gin.H{
					"task_id":  task.TaskID,
					"stage_id": inst.StageID,
					"status":   string(inst.Status),
				}
				if inst.HumanReviewPassed != nil {
					item["human_review_passed"] = *inst.HumanReviewPassed
				}
				if inst.AiReviewPassed != nil {
					item["ai_review_passed"] = *inst.AiReviewPassed
				}
				history = append(history, item)
			}
		}
	}

	if history == nil {
		history = []gin.H{}
	}

	c.JSON(http.StatusOK, gin.H{"history": history})
}