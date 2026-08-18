package api

import (
	"github.com/gin-gonic/gin"
)

func SetupRouter(svc *Service) *gin.Engine {
	r := gin.Default()

	r.GET("/health", func(c *gin.Context) {
		c.JSON(200, gin.H{"status": "ok"})
	})

	v1 := r.Group("/api/v1")
	{
		v1.POST("/projects", svc.CreateProject)
		v1.GET("/projects/:id", svc.GetProject)
		v1.GET("/projects", svc.ListProjects)
		v1.PUT("/projects/:id", svc.UpdateProject)
		v1.DELETE("/projects/:id", svc.DeleteProject)

		v1.POST("/projects/:id/tasks", svc.CreateTask)
		v1.GET("/projects/:id/tasks", svc.ListTasks)
		v1.GET("/tasks", svc.ListAllTasks)
		v1.GET("/tasks/:id", svc.GetTask)
		v1.PUT("/tasks/:id", svc.UpdateTask)
		v1.POST("/tasks/:id/retry", svc.RetryTask)
		v1.POST("/tasks/:id/rollback", svc.RollbackTask)

		v1.POST("/pipelines", svc.StartPipeline)
		v1.GET("/pipelines/:id", svc.GetPipelineResult)

		v1.GET("/tasks/:id/stages", svc.ListStageResults)
		v1.GET("/tasks/:id/stages/:stageId", svc.GetStageResult)

		v1.POST("/reviews/:stageId/submit", svc.SubmitReview)
		v1.POST("/reviews/submit", svc.SubmitReviewGeneric)
		v1.GET("/reviews/pending", svc.ListPendingReviews)

		v1.GET("/stats", svc.GetStats)
		v1.GET("/activity", svc.GetActivity)

		v1.GET("/projects/:id/graph", svc.GetProjectGraph)
		v1.GET("/projects/:id/snapshot", svc.GetProjectSnapshot)

		v1.POST("/agents/register", svc.RegisterAgent)
		v1.POST("/agents/heartbeat", svc.AgentHeartbeat)
		v1.GET("/agents", svc.ListAgents)
		v1.GET("/agents/", svc.ListAgents)
		v1.GET("/tasks/available", svc.GetAvailableTasks)
		v1.POST("/tasks/:id/claim", svc.ClaimTask)
		v1.POST("/tasks/:id/callback", svc.TaskCallback)

		v1.GET("/system/health", svc.GetSystemHealth)
		v1.GET("/system/status", svc.GetSystemStatus)
		v1.GET("/system/resources", svc.GetSystemResources)
		v1.GET("/system/active-tasks", svc.GetActiveTasks)

		v1.GET("/logs/system", svc.GetSystemLogs)
		v1.GET("/logs/stage/:taskId/:stageId", svc.GetStageLogs)
		v1.GET("/logs/agent-os", svc.GetAgentOSLogs)

		v1.GET("/reviews/:stageId/history", svc.GetReviewHistory)

		v1.POST("/graph/sync", svc.GraphSync)
		v1.GET("/graph/context", svc.GetGraphContext)

		v1.GET("/config/llm", svc.GetLLMConfig)
		v1.PUT("/config/llm", svc.UpdateLLMConfig)
		v1.POST("/config/llm", svc.UpdateLLMConfig)
	}

	r.GET("/ws", svc.HandleWebSocket)

	return r
}