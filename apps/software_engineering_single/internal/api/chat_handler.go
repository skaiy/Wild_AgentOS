package api

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
	pb "github.com/agent-os/se-app/proto/seapp"
	grpcclient "github.com/agent-os/se-app/internal/grpc"
)

type ChatRequest struct {
	Messages  []ChatMessage `json:"messages" binding:"required"`
	ProjectID string        `json:"project_id"`
}

type ChatMessage struct {
	Role    string `json:"role" binding:"required"`
	Content string `json:"content" binding:"required"`
}

type ChatResponse struct {
	Content   string `json:"content"`
	Status    string `json:"status"`
	Summary   string `json:"summary,omitempty"`
	OutputIRI string `json:"output_iri,omitempty"`
	StageID   string `json:"stage_id,omitempty"`
}

type SSEChunk struct {
	Content string `json:"content"`
	Done    bool   `json:"done"`
	Status  string `json:"status"`
}

func (svc *Service) ChatHandler(c *gin.Context) {
	var req ChatRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	if len(req.Messages) == 0 {
		c.JSON(http.StatusBadRequest, gin.H{"error": "messages cannot be empty"})
		return
	}

	llm := svc.Config.LLM
	if llm.BaseURL == "" || llm.ApiKey == "" || llm.Model == "" {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "LLM is not configured"})
		return
	}

	if svc.GRPC == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{
			"error": "Agent OS 内核未连接，对话功能不可用。请确保 Agent OS 内核服务正在运行。",
		})
		return
	}

	var conversationContext strings.Builder
	for _, msg := range req.Messages {
		roleLabel := "用户"
		if msg.Role == "assistant" {
			roleLabel = "助手"
		} else if msg.Role == "system" {
			roleLabel = "系统"
		}
		conversationContext.WriteString(fmt.Sprintf("%s: %s\n", roleLabel, msg.Content))
	}

	prompt := conversationContext.String()

	taskIRI := ""
	if req.ProjectID != "" {
		taskIRI = fmt.Sprintf("se:project:%s", req.ProjectID)
	}

	grpcReq := &pb.ChatStreamRequest{
		Prompt:     prompt,
		TaskIri:    taskIRI,
		LlmApiKey:  llm.ApiKey,
		LlmBaseUrl: llm.BaseURL,
		LlmModel:   llm.Model,
	}

	c.Header("Content-Type", "text/event-stream")
	c.Header("Cache-Control", "no-cache")
	c.Header("Connection", "keep-alive")
	c.Header("X-Accel-Buffering", "no")

	ctx := c.Request.Context()

	ch, err := svc.GRPC.ChatStream(ctx, grpcReq)
	if err != nil {
		sseChunk := SSEChunk{
			Content: fmt.Sprintf("Agent OS 内核调用失败: %v", err),
			Done:    true,
			Status:  "error",
		}
		data, _ := json.Marshal(sseChunk)
		c.SSEvent("chunk", string(data))
		c.Writer.Flush()
		return
	}

	for chunk := range ch {
		sseChunk := SSEChunk{
			Content: chunk.Content,
			Done:    chunk.Done,
			Status:  chunk.Status,
		}
		data, _ := json.Marshal(sseChunk)
		fmt.Fprintf(c.Writer, "data: %s\n\n", data)
		c.Writer.Flush()

		if chunk.Done {
			break
		}
	}
}

func (svc *Service) ChatSyncHandler(c *gin.Context) {
	var req ChatRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	if len(req.Messages) == 0 {
		c.JSON(http.StatusBadRequest, gin.H{"error": "messages cannot be empty"})
		return
	}

	llm := svc.Config.LLM
	if llm.BaseURL == "" || llm.ApiKey == "" || llm.Model == "" {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "LLM is not configured"})
		return
	}

	if svc.GRPC == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{
			"error": "Agent OS 内核未连接，对话功能不可用。请确保 Agent OS 内核服务正在运行。",
		})
		return
	}

	var conversationContext strings.Builder
	for _, msg := range req.Messages {
		roleLabel := "用户"
		if msg.Role == "assistant" {
			roleLabel = "助手"
		} else if msg.Role == "system" {
			roleLabel = "系统"
		}
		conversationContext.WriteString(fmt.Sprintf("%s: %s\n", roleLabel, msg.Content))
	}

	prompt := conversationContext.String()

	stageID := fmt.Sprintf("chat-%d", time.Now().UnixNano())
	taskIRI := ""
	if req.ProjectID != "" {
		taskIRI = fmt.Sprintf("se:project:%s", req.ProjectID)
	}

	grpcReq := &pb.ChatStreamRequest{
		Prompt:     prompt,
		TaskIri:    taskIRI,
		LlmApiKey:  llm.ApiKey,
		LlmBaseUrl: llm.BaseURL,
		LlmModel:   llm.Model,
	}

	ctx, cancel := context.WithTimeout(c.Request.Context(), 300*time.Second)
	defer cancel()

	ch, err := svc.GRPC.ChatStream(ctx, grpcReq)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{
			"error":  fmt.Sprintf("Agent OS 内核调用失败: %v", err),
			"detail": "请检查 Agent OS 内核服务是否正常运行",
		})
		return
	}

	var fullContent strings.Builder
	var finalStatus string

	for chunk := range ch {
		fullContent.WriteString(chunk.Content)
		if chunk.Done {
			finalStatus = chunk.Status
		}
	}

	if finalStatus == "" {
		finalStatus = "completed"
	}

	content := fullContent.String()
	if content == "" {
		content = "Agent 已处理您的请求，但未返回具体内容。"
	}

	chatResp := ChatResponse{
		Content: content,
		Status:  finalStatus,
		StageID: stageID,
	}

	c.JSON(http.StatusOK, chatResp)
}

func (svc *Service) ChatStreamLegacyHandler(c *gin.Context) {
	var req ChatRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	if len(req.Messages) == 0 {
		c.JSON(http.StatusBadRequest, gin.H{"error": "messages cannot be empty"})
		return
	}

	llm := svc.Config.LLM
	if llm.BaseURL == "" || llm.ApiKey == "" || llm.Model == "" {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "LLM is not configured"})
		return
	}

	if svc.GRPC == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{
			"error": "Agent OS 内核未连接，对话功能不可用。请确保 Agent OS 内核服务正在运行。",
		})
		return
	}

	var conversationContext strings.Builder
	for _, msg := range req.Messages {
		roleLabel := "用户"
		if msg.Role == "assistant" {
			roleLabel = "助手"
		} else if msg.Role == "system" {
			roleLabel = "系统"
		}
		conversationContext.WriteString(fmt.Sprintf("%s: %s\n", roleLabel, msg.Content))
	}

	prompt := conversationContext.String()

	stageID := fmt.Sprintf("chat-%d", time.Now().UnixNano())
	taskIRI := ""
	if req.ProjectID != "" {
		taskIRI = fmt.Sprintf("se:project:%s", req.ProjectID)
	}

	grpcReq := &pb.ExecuteStageRequest{
		StageId:    stageID,
		StageType:  "requirement",
		Prompt:     prompt,
		ProjectDir: "",
		TaskIri:    taskIRI,
		LlmApiKey:  llm.ApiKey,
		LlmBaseUrl: llm.BaseURL,
		LlmModel:   llm.Model,
	}

	ctx, cancel := context.WithTimeout(c.Request.Context(), 300*time.Second)
	defer cancel()

	resp, err := svc.GRPC.ExecuteStage(ctx, grpcReq)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{
			"error":  fmt.Sprintf("Agent OS 内核调用失败: %v", err),
			"detail": "请检查 Agent OS 内核服务是否正常运行",
		})
		return
	}

	content := resp.GetSummary()
	if content == "" && len(resp.GetOutputJson()) > 0 {
		var output map[string]interface{}
		if err := json.Unmarshal(resp.GetOutputJson(), &output); err == nil {
			if summary, ok := output["summary"].(string); ok && summary != "" {
				content = summary
			} else {
				formatted, err := json.MarshalIndent(output, "", "  ")
				if err == nil {
					content = string(formatted)
				}
			}
		}
	}
	if content == "" {
		content = "Agent 已处理您的请求，但未返回具体内容。"
	}

	if len(resp.GetErrors()) > 0 {
		content += fmt.Sprintf("\n\n⚠️ 执行过程中出现错误:\n%s", strings.Join(resp.GetErrors(), "\n"))
	}

	chatResp := ChatResponse{
		Content:   content,
		Status:    resp.GetStatus(),
		Summary:   resp.GetSummary(),
		OutputIRI: resp.GetOutputIri(),
		StageID:   stageID,
	}

	c.JSON(http.StatusOK, chatResp)
}

func init() {
	_ = grpcclient.ChatStreamChunk{}
}
