package api

import (
	"net/http"

	"github.com/gin-gonic/gin"

	"github.com/agent-os/se-center/internal/graph"
)

// GraphSync — POST /api/v1/graph/sync
// 处理边缘节点推送的图谱变更
func (s *Service) GraphSync(c *gin.Context) {
	if s.GraphManager == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "graph manager not available"})
		return
	}

	var req graph.SyncRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	resp, err := s.GraphManager.SyncFromEdge(c.Request.Context(), req)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}

	if resp.Status == "rejected" {
		c.JSON(http.StatusConflict, resp)
		return
	}

	c.JSON(http.StatusOK, resp)
}

// GetGraphContext — GET /api/v1/graph/context
// 返回全局图谱上下文（简化版）
func (s *Service) GetGraphContext(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{
		"status": "ok",
		"note":   "graph context endpoint - delegates to Agent OS kernel",
	})
}