package api

import (
	"net/http"

	"github.com/gin-gonic/gin"

	"github.com/agent-os/se-app/internal/config"
)

func (svc *Service) GetLLMConfig(c *gin.Context) {
	if svc.Config == nil || svc.Config.LLM.ApiKey == "" {
		c.JSON(http.StatusOK, gin.H{
			"base_url": svc.Config.LLM.BaseURL,
			"model":    svc.Config.LLM.Model,
			"api_key":  "",
		})
		return
	}

	maskedKey := ""
	if len(svc.Config.LLM.ApiKey) > 8 {
		maskedKey = svc.Config.LLM.ApiKey[:4] + "****" + svc.Config.LLM.ApiKey[len(svc.Config.LLM.ApiKey)-4:]
	} else if len(svc.Config.LLM.ApiKey) > 0 {
		maskedKey = "****"
	}

	c.JSON(http.StatusOK, gin.H{
		"base_url": svc.Config.LLM.BaseURL,
		"model":    svc.Config.LLM.Model,
		"api_key":  maskedKey,
	})
}

type UpdateLLMConfigRequest struct {
	BaseURL *string `json:"base_url,omitempty"`
	Model   *string `json:"model,omitempty"`
	ApiKey  *string `json:"api_key,omitempty"`
}

func (svc *Service) UpdateLLMConfig(c *gin.Context) {
	var req UpdateLLMConfigRequest
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	if req.BaseURL != nil {
		svc.Config.LLM.BaseURL = *req.BaseURL
	}
	if req.Model != nil {
		svc.Config.LLM.Model = *req.Model
	}
	if req.ApiKey != nil {
		svc.Config.LLM.ApiKey = *req.ApiKey
	}

	maskedKey := ""
	if len(svc.Config.LLM.ApiKey) > 8 {
		maskedKey = svc.Config.LLM.ApiKey[:4] + "****" + svc.Config.LLM.ApiKey[len(svc.Config.LLM.ApiKey)-4:]
	} else if len(svc.Config.LLM.ApiKey) > 0 {
		maskedKey = "****"
	}

	c.JSON(http.StatusOK, gin.H{
		"base_url": svc.Config.LLM.BaseURL,
		"model":    svc.Config.LLM.Model,
		"api_key":  maskedKey,
	})
}

func (svc *Service) ValidateConfig(c *gin.Context) {
	var req config.Config
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	issues := make([]string, 0)

	if req.Temporal.HostPort == "" {
		issues = append(issues, "temporal.host_port is required")
	}
	if req.GRPC.Target == "" {
		issues = append(issues, "grpc.target is required")
	}
	if req.MetaStore.Driver == "" {
		issues = append(issues, "meta_store.driver is required")
	}
	if req.MetaStore.DSN == "" {
		issues = append(issues, "meta_store.dsn is required")
	}
	if req.Server.Port == 0 {
		issues = append(issues, "server.port is required")
	}
	if req.LLM.BaseURL == "" {
		issues = append(issues, "llm.base_url is required")
	}
	if req.LLM.Model == "" {
		issues = append(issues, "llm.model is required")
	}

	valid := len(issues) == 0

	c.JSON(http.StatusOK, gin.H{
		"valid":  valid,
		"issues": issues,
	})
}