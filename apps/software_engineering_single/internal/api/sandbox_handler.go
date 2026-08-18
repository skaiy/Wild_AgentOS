package api

import (
	"net/http"

	"github.com/gin-gonic/gin"
)

func (svc *Service) CreateSandbox(c *gin.Context) {
	c.JSON(http.StatusServiceUnavailable, gin.H{"error": "sandbox feature is not available"})
}

func (svc *Service) ListSandboxes(c *gin.Context) {
	c.JSON(http.StatusServiceUnavailable, gin.H{"error": "sandbox feature is not available"})
}

func (svc *Service) GetSandbox(c *gin.Context) {
	c.JSON(http.StatusServiceUnavailable, gin.H{"error": "sandbox feature is not available"})
}

func (svc *Service) TerminateSandbox(c *gin.Context) {
	c.JSON(http.StatusServiceUnavailable, gin.H{"error": "sandbox feature is not available"})
}

func (svc *Service) ExecuteInSandbox(c *gin.Context) {
	c.JSON(http.StatusServiceUnavailable, gin.H{"error": "sandbox feature is not available"})
}

func (svc *Service) GetTaskSandbox(c *gin.Context) {
	c.JSON(http.StatusServiceUnavailable, gin.H{"error": "sandbox feature is not available"})
}
