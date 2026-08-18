package api

import (
	"fmt"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/gin-gonic/gin"
	"github.com/stretchr/testify/assert"

	"github.com/agent-os/se-center/internal/config"
	"github.com/agent-os/se-center/internal/grpc"
	"github.com/agent-os/se-center/internal/types"
)

type mockMetaStore struct {
	types.MetaStore
}

func newMockService() *Service {
	cfg := &config.Config{
		Server: config.ServerConfig{Port: 8080, Host: "0.0.0.0"},
		LLM: config.LLMConfig{
			Provider: "openai",
			Model:    "gpt-4",
			BaseURL:  "https://api.openai.com",
		},
	}
	return &Service{
		Config:         cfg,
		MetaStore:      &mockMetaStore{},
		GRPC:           &grpc.Client{},
		TemporalClient: nil,
		Hub:            NewHub(),
		TaskQueue:      "test-queue",
	}
}

func TestHealthCheck(t *testing.T) {
	svc := newMockService()
	router := SetupRouter(svc)

	w := httptest.NewRecorder()
	req, _ := http.NewRequest("GET", "/health", nil)
	router.ServeHTTP(w, req)

	assert.Equal(t, 200, w.Code)
	assert.Contains(t, w.Body.String(), `"status":"ok"`)
}

func TestRouteRegistration(t *testing.T) {
	svc := newMockService()
	router := SetupRouter(svc)

	type routeCheck struct {
		method string
		path   string
		code   int
	}

	routes := []routeCheck{
		// 健康检查
		{"GET", "/health", 200},

		// 项目管理
		{"POST", "/api/v1/projects", 400},
		{"GET", "/api/v1/projects/test-id", 404},
		{"GET", "/api/v1/projects", 200},
		{"PUT", "/api/v1/projects/test-id", 400},
		{"DELETE", "/api/v1/projects/test-id", 500},

		// 任务管理
		{"POST", "/api/v1/projects/test-id/tasks", 400},
		{"GET", "/api/v1/projects/test-id/tasks", 200},
		{"GET", "/api/v1/tasks", 200},
		{"GET", "/api/v1/tasks/test-id", 404},
		{"PUT", "/api/v1/tasks/test-id", 400},
		{"POST", "/api/v1/tasks/test-id/retry", 500},
		{"POST", "/api/v1/tasks/test-id/rollback", 500},

		// 管线
		{"POST", "/api/v1/pipelines", 503},
		{"GET", "/api/v1/pipelines/test-id", 200},

		// 阶段
		{"GET", "/api/v1/tasks/test-id/stages", 404},
		{"GET", "/api/v1/tasks/test-id/stages/stage-1", 404},

		// 审查
		{"POST", "/api/v1/reviews/stage-1/submit", 503},
		{"GET", "/api/v1/reviews/pending", 200},

		// 仪表盘
		{"GET", "/api/v1/stats", 200},
		{"GET", "/api/v1/activity", 200},

		// 图谱
		{"GET", "/api/v1/projects/test-id/graph", 404},
		{"GET", "/api/v1/projects/test-id/snapshot", 404},

		// 配置
		{"GET", "/api/v1/config/llm", 200},
		{"PUT", "/api/v1/config/llm", 400},

		// 全局图谱同步
		{"POST", "/api/v1/graph/sync", 400},
		{"GET", "/api/v1/graph/context", 200},

		// WebSocket
		{"GET", "/ws", 400},
	}

	for _, rt := range routes {
		t.Run(fmt.Sprintf("%s %s", rt.method, rt.path), func(t *testing.T) {
			w := httptest.NewRecorder()
			req, _ := http.NewRequest(rt.method, rt.path, nil)
			router.ServeHTTP(w, req)
			assert.Equal(t, rt.code, w.Code,
				"expected %d for %s %s, got %d: %s",
				rt.code, rt.method, rt.path, w.Code, w.Body.String())
		})
	}
}

func TestGinMode(t *testing.T) {
	assert.Equal(t, gin.TestMode, gin.Mode())
}

// --- mock MetaStore implementations ---

func (m *mockMetaStore) CreateProject(meta *types.ProjectMeta) error {
	if meta.ProjectID == "" {
		return fmt.Errorf("project_id required")
	}
	return nil
}

func (m *mockMetaStore) GetProject(projectID string) (*types.ProjectMeta, error) {
	return nil, fmt.Errorf("not found")
}

func (m *mockMetaStore) ListProjects(filter map[string]interface{}) ([]*types.ProjectMeta, error) {
	return []*types.ProjectMeta{}, nil
}

func (m *mockMetaStore) UpdateProject(projectID string, name, description string) error {
	return fmt.Errorf("mock error")
}

func (m *mockMetaStore) UpdateProjectStatus(projectID string, status types.ProjectStatus) error {
	return nil
}

func (m *mockMetaStore) DeleteProject(projectID string) error {
	return fmt.Errorf("mock error")
}

func (m *mockMetaStore) CreateTask(meta *types.TaskMeta) error {
	return fmt.Errorf("mock error")
}

func (m *mockMetaStore) GetTask(taskID string) (*types.TaskMeta, error) {
	return nil, fmt.Errorf("not found")
}

func (m *mockMetaStore) ListTasks(projectID string) ([]*types.TaskMeta, error) {
	return []*types.TaskMeta{}, nil
}

func (m *mockMetaStore) ListAllTasks() ([]*types.TaskMeta, error) {
	return []*types.TaskMeta{}, nil
}

func (m *mockMetaStore) UpdateTaskStatus(taskID string, status types.TaskStatus, currentStage string) error {
	return fmt.Errorf("mock error")
}

func (m *mockMetaStore) UpdateTaskWorkflow(taskID string, workflowID, runID string) error {
	return nil
}

func (m *mockMetaStore) SaveStageInstance(taskID string, meta *types.StageInstanceMeta) error {
	return nil
}

func (m *mockMetaStore) UpdateStageInstanceStatus(taskID, stageID string, status types.StageInstanceStatus) error {
	return nil
}

func (m *mockMetaStore) GetStageInstance(taskID, stageID string) (*types.StageInstanceMeta, error) {
	return nil, fmt.Errorf("not found")
}

func (m *mockMetaStore) ListStageInstances(taskID string) ([]*types.StageInstanceMeta, error) {
	return nil, fmt.Errorf("not found")
}

func (m *mockMetaStore) ListAllStageInstances(offset, limit int) ([]*types.StageInstanceMeta, error) {
	return []*types.StageInstanceMeta{}, nil
}

func (m *mockMetaStore) SearchTasksByStatus(status types.TaskStatus) ([]*types.TaskMeta, error) {
	return []*types.TaskMeta{}, nil
}

func (m *mockMetaStore) GetWorkflowSnapshot(projectID, taskID string) (*types.WorkflowSnapshot, error) {
	return nil, fmt.Errorf("not found")
}

func init() {
	gin.SetMode(gin.TestMode)
}