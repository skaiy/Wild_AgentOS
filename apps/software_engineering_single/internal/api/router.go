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
		projects := v1.Group("/projects")
		{
			projects.POST("", svc.CreateProject)
			projects.GET("/:id", svc.GetProject)
			projects.PUT("/:id", svc.UpdateProject)
			projects.GET("", svc.ListProjects)
			projects.DELETE("/:id", svc.DeleteProject)
		}

		tasks := v1.Group("/tasks")
		{
			tasks.GET("", svc.ListAllTasks)
			tasks.GET("/:taskId", svc.GetTask)
			tasks.PUT("/:taskId", svc.UpdateTask)
			tasks.POST("/:taskId/retry", svc.RetryTask)
			tasks.POST("/:taskId/rollback", svc.RollbackTask)
			projects.POST("/:id/tasks", svc.CreateTask)
			projects.GET("/:id/tasks", svc.ListTasks)
		}

		pipelines := v1.Group("/pipelines")
		{
			pipelines.POST("", svc.StartPipeline)
			pipelines.GET("/:id", svc.GetPipelineResult)
		}

		taskStages := v1.Group("/tasks")
		{
			taskStages.GET("/:taskId/stages", svc.ListStageResults)
			taskStages.GET("/:taskId/stages/:stageId", svc.GetStageResult)
		}

		reviews := v1.Group("/reviews")
		{
			reviews.POST("/:stageId/submit", svc.SubmitReview)
			reviews.GET("/pending", svc.ListPendingReviews)
			reviews.GET("/:stageId/history", svc.GetReviewHistory)
		}

		chat := v1.Group("/chat")
		{
			chat.POST("", svc.ChatHandler)
			chat.POST("/sync", svc.ChatSyncHandler)
			chat.POST("/legacy", svc.ChatStreamLegacyHandler)
		}

		config := v1.Group("/config")
		{
			config.GET("/llm", svc.GetLLMConfig)
			config.POST("/llm", svc.UpdateLLMConfig)
			config.POST("/validate", svc.ValidateConfig)
		}

		system := v1.Group("/system")
		{
			system.GET("/status", svc.GetSystemStatus)
			system.GET("/health", svc.GetSystemHealth)
			system.GET("/resources", svc.GetSystemResources)
			system.GET("/active-tasks", svc.GetActiveTasks)
		}

		logs := v1.Group("/logs")
		{
			logs.GET("/system", svc.GetSystemLogs)
			logs.GET("/stage/:taskId/:stageId", svc.GetStageLogs)
			logs.GET("/agent-os", svc.GetAgentOSLogs)
		}

		v1.GET("/projects/:id/graph", svc.GetProjectGraph)
		v1.GET("/projects/:id/snapshot", svc.GetProjectSnapshot)

		v1.GET("/stats", svc.GetStats)
		v1.GET("/stats/pipeline-trends", svc.GetPipelineTrends)
		v1.GET("/activity", svc.GetActivity)
	}

	r.GET("/ws", svc.HandleWebSocket)

	return r
}