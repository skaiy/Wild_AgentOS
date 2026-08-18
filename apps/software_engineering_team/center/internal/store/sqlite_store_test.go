package store

import (
	"database/sql"
	"testing"
	"time"

	"github.com/agent-os/se-center/internal/types"
	"github.com/google/uuid"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func newTestStore(t *testing.T) *SQLiteMetaStore {
	t.Helper()
	s, err := NewSQLiteMetaStore("file::memory:?cache=shared")
	require.NoError(t, err)
	t.Cleanup(func() { s.Close() })
	return s
}

func makeProjectMeta(id, name string) *types.ProjectMeta {
	return &types.ProjectMeta{
		ProjectID:   id,
		ProjectName: name,
		Description: "test project",
		Status:      types.ProjectStatusInit,
		Tags:        []string{"test"},
		Extras:      map[string]interface{}{"key": "value"},
	}
}

func makeTaskMeta(taskID, projectID, pipelineName string) *types.TaskMeta {
	return &types.TaskMeta{
		TaskID:       taskID,
		ProjectID:    projectID,
		PipelineName: pipelineName,
		Status:       types.TaskStatusPending,
	}
}

func makeStageMeta(taskID, stageID string, stageType types.StageType, order int) *types.StageInstanceMeta {
	return &types.StageInstanceMeta{
		StageID:   stageID,
		TaskID:    taskID,
		StageType: stageType,
		Name:      string(stageType),
		Status:    types.StageStatusPending,
		Order:     order,
	}
}

func TestCreateAndGetProject(t *testing.T) {
	s := newTestStore(t)
	p := makeProjectMeta(uuid.New().String(), "test-project")

	err := s.CreateProject(p)
	require.NoError(t, err)

	got, err := s.GetProject(p.ProjectID)
	require.NoError(t, err)
	require.NotNil(t, got)

	assert.Equal(t, p.ProjectID, got.ProjectID)
	assert.Equal(t, p.ProjectName, got.ProjectName)
	assert.Equal(t, p.Status, got.Status)
	assert.Equal(t, p.Tags, got.Tags)
	assert.Equal(t, p.Extras, got.Extras)
	assert.False(t, got.CreatedAt.IsZero())
	assert.False(t, got.UpdatedAt.IsZero())
}

func TestUpdateProject(t *testing.T) {
	s := newTestStore(t)
	p := makeProjectMeta(uuid.New().String(), "test-project")
	require.NoError(t, s.CreateProject(p))

	err := s.UpdateProject(p.ProjectID, "updated-name", "updated-desc")
	require.NoError(t, err)

	got, err := s.GetProject(p.ProjectID)
	require.NoError(t, err)
	assert.Equal(t, "updated-name", got.ProjectName)
	assert.Equal(t, "updated-desc", got.Description)
}

func TestUpdateProjectStatus(t *testing.T) {
	s := newTestStore(t)
	p := makeProjectMeta(uuid.New().String(), "test-project")
	require.NoError(t, s.CreateProject(p))

	err := s.UpdateProjectStatus(p.ProjectID, types.ProjectStatusRunning)
	require.NoError(t, err)

	got, err := s.GetProject(p.ProjectID)
	require.NoError(t, err)
	assert.Equal(t, types.ProjectStatusRunning, got.Status)
}

func TestListProjects(t *testing.T) {
	s := newTestStore(t)

	p1 := makeProjectMeta(uuid.New().String(), "project-alpha")
	p1.Status = types.ProjectStatusRunning
	p2 := makeProjectMeta(uuid.New().String(), "project-beta")
	p2.Status = types.ProjectStatusInit
	p3 := makeProjectMeta(uuid.New().String(), "project-gamma")
	p3.Status = types.ProjectStatusRunning

	require.NoError(t, s.CreateProject(p1))
	require.NoError(t, s.CreateProject(p2))
	require.NoError(t, s.CreateProject(p3))

	t.Run("list all", func(t *testing.T) {
		projects, err := s.ListProjects(map[string]interface{}{})
		require.NoError(t, err)
		assert.Len(t, projects, 3)
	})

	t.Run("filter by status", func(t *testing.T) {
		projects, err := s.ListProjects(map[string]interface{}{
			"status": types.ProjectStatusRunning,
		})
		require.NoError(t, err)
		assert.Len(t, projects, 2)
	})

	t.Run("filter by name", func(t *testing.T) {
		projects, err := s.ListProjects(map[string]interface{}{
			"name": "alpha",
		})
		require.NoError(t, err)
		assert.Len(t, projects, 1)
		assert.Equal(t, "project-alpha", projects[0].ProjectName)
	})

	t.Run("filter by status and name", func(t *testing.T) {
		projects, err := s.ListProjects(map[string]interface{}{
			"status": types.ProjectStatusRunning,
			"name":   "project",
		})
		require.NoError(t, err)
		assert.Len(t, projects, 2)
	})

	t.Run("filter by project_id", func(t *testing.T) {
		projects, err := s.ListProjects(map[string]interface{}{
			"project_id": p1.ProjectID,
		})
		require.NoError(t, err)
		assert.Len(t, projects, 1)
		assert.Equal(t, p1.ProjectID, projects[0].ProjectID)
	})
}

func TestDeleteProjectCascade(t *testing.T) {
	s := newTestStore(t)
	projectID := uuid.New().String()
	taskID := uuid.New().String()

	p := makeProjectMeta(projectID, "to-delete")
	require.NoError(t, s.CreateProject(p))

	task := makeTaskMeta(taskID, projectID, "pipeline")
	require.NoError(t, s.CreateTask(task))

	stage := makeStageMeta(taskID, uuid.New().String(), types.StageDesign, 1)
	require.NoError(t, s.SaveStageInstance(taskID, stage))

	err := s.DeleteProject(projectID)
	require.NoError(t, err)

	_, err = s.GetProject(projectID)
	assert.ErrorIs(t, err, sql.ErrNoRows)

	_, err = s.GetTask(taskID)
	assert.ErrorIs(t, err, sql.ErrNoRows)
}

func TestCreateAndGetTask(t *testing.T) {
	s := newTestStore(t)
	projectID := uuid.New().String()
	taskID := uuid.New().String()

	require.NoError(t, s.CreateProject(makeProjectMeta(projectID, "proj")))
	require.NoError(t, s.CreateTask(makeTaskMeta(taskID, projectID, "pipeline")))

	task, err := s.GetTask(taskID)
	require.NoError(t, err)
	assert.Equal(t, taskID, task.TaskID)
	assert.Equal(t, projectID, task.ProjectID)
	assert.Equal(t, types.TaskStatusPending, task.Status)
	assert.Empty(t, task.Stages)
}

func TestUpdateTaskStatus(t *testing.T) {
	s := newTestStore(t)
	projectID := uuid.New().String()
	taskID := uuid.New().String()

	require.NoError(t, s.CreateProject(makeProjectMeta(projectID, "proj")))
	require.NoError(t, s.CreateTask(makeTaskMeta(taskID, projectID, "pipeline")))

	err := s.UpdateTaskStatus(taskID, types.TaskStatusRunning, "stage-1")
	require.NoError(t, err)

	task, err := s.GetTask(taskID)
	require.NoError(t, err)
	assert.Equal(t, types.TaskStatusRunning, task.Status)
	assert.Equal(t, "stage-1", task.CurrentStage)
	assert.Nil(t, task.CompletedAt)

	err = s.UpdateTaskStatus(taskID, types.TaskStatusCompleted, "")
	require.NoError(t, err)

	task, err = s.GetTask(taskID)
	require.NoError(t, err)
	assert.Equal(t, types.TaskStatusCompleted, task.Status)
	assert.NotNil(t, task.CompletedAt)
}

func TestUpdateTaskWorkflow(t *testing.T) {
	s := newTestStore(t)
	projectID := uuid.New().String()
	taskID := uuid.New().String()

	require.NoError(t, s.CreateProject(makeProjectMeta(projectID, "proj")))
	require.NoError(t, s.CreateTask(makeTaskMeta(taskID, projectID, "pipeline")))

	err := s.UpdateTaskWorkflow(taskID, "wf-123", "run-456")
	require.NoError(t, err)

	task, err := s.GetTask(taskID)
	require.NoError(t, err)
	assert.Equal(t, "wf-123", task.WorkflowID)
	assert.Equal(t, "run-456", task.RunID)
}

func TestListTasks(t *testing.T) {
	s := newTestStore(t)
	projectID := uuid.New().String()
	require.NoError(t, s.CreateProject(makeProjectMeta(projectID, "proj")))

	for i := 0; i < 3; i++ {
		task := makeTaskMeta(uuid.New().String(), projectID, "pipeline")
		task.Status = types.TaskStatusRunning
		require.NoError(t, s.CreateTask(task))
	}

	tasks, err := s.ListTasks(projectID)
	require.NoError(t, err)
	assert.Len(t, tasks, 3)
	for _, task := range tasks {
		assert.Equal(t, types.TaskStatusRunning, task.Status)
	}
}

func TestStageInstanceCRUD(t *testing.T) {
	s := newTestStore(t)
	projectID := uuid.New().String()
	taskID := uuid.New().String()

	require.NoError(t, s.CreateProject(makeProjectMeta(projectID, "proj")))
	require.NoError(t, s.CreateTask(makeTaskMeta(taskID, projectID, "pipeline")))

	stage := makeStageMeta(taskID, "s1", types.StageRequirement, 1)
	require.NoError(t, s.SaveStageInstance(taskID, stage))

	got, err := s.GetStageInstance(taskID, "s1")
	require.NoError(t, err)
	assert.Equal(t, "s1", got.StageID)
	assert.Equal(t, types.StageStatusPending, got.Status)

	err = s.UpdateStageInstanceStatus(taskID, "s1", types.StageStatusRunning)
	require.NoError(t, err)

	got, err = s.GetStageInstance(taskID, "s1")
	require.NoError(t, err)
	assert.Equal(t, types.StageStatusRunning, got.Status)

	err = s.UpdateStageInstanceStatus(taskID, "s1", types.StageStatusCompleted)
	require.NoError(t, err)

	got, err = s.GetStageInstance(taskID, "s1")
	require.NoError(t, err)
	assert.Equal(t, types.StageStatusCompleted, got.Status)
	assert.NotNil(t, got.CompletedAt)
}

func TestListStageInstances(t *testing.T) {
	s := newTestStore(t)
	projectID := uuid.New().String()
	taskID := uuid.New().String()

	require.NoError(t, s.CreateProject(makeProjectMeta(projectID, "proj")))
	require.NoError(t, s.CreateTask(makeTaskMeta(taskID, projectID, "pipeline")))

	stages := []*types.StageInstanceMeta{
		makeStageMeta(taskID, "s1", types.StageRequirement, 1),
		makeStageMeta(taskID, "s2", types.StageDesign, 2),
		makeStageMeta(taskID, "s3", types.StageCoding, 3),
	}
	for _, st := range stages {
		require.NoError(t, s.SaveStageInstance(taskID, st))
	}

	got, err := s.ListStageInstances(taskID)
	require.NoError(t, err)
	assert.Len(t, got, 3)
	assert.Equal(t, "s1", got[0].StageID)
	assert.Equal(t, "s2", got[1].StageID)
	assert.Equal(t, "s3", got[2].StageID)
}

func TestListAllStageInstancesPagination(t *testing.T) {
	s := newTestStore(t)
	projectID := uuid.New().String()
	taskID := uuid.New().String()

	require.NoError(t, s.CreateProject(makeProjectMeta(projectID, "proj")))
	require.NoError(t, s.CreateTask(makeTaskMeta(taskID, projectID, "pipeline")))

	for i := 0; i < 10; i++ {
		stage := makeStageMeta(taskID, uuid.New().String(), types.StageCoding, i)
		time.Sleep(time.Millisecond) // ensure different started_at
		require.NoError(t, s.SaveStageInstance(taskID, stage))
	}

	t.Run("first page", func(t *testing.T) {
		stages, err := s.ListAllStageInstances(0, 3)
		require.NoError(t, err)
		assert.Len(t, stages, 3)
	})

	t.Run("second page", func(t *testing.T) {
		stages, err := s.ListAllStageInstances(3, 3)
		require.NoError(t, err)
		assert.Len(t, stages, 3)
	})

	t.Run("last page with fewer items", func(t *testing.T) {
		stages, err := s.ListAllStageInstances(9, 3)
		require.NoError(t, err)
		assert.Len(t, stages, 1)
	})

	t.Run("negative offset treated as zero", func(t *testing.T) {
		stages, err := s.ListAllStageInstances(-1, 5)
		require.NoError(t, err)
		assert.Len(t, stages, 5)
	})

	t.Run("default limit when zero", func(t *testing.T) {
		stages, err := s.ListAllStageInstances(0, 0)
		require.NoError(t, err)
		assert.Len(t, stages, 10)
	})
}

func TestListAllTasksNPlusOneOptimization(t *testing.T) {
	s := newTestStore(t)
	projectID := uuid.New().String()
	require.NoError(t, s.CreateProject(makeProjectMeta(projectID, "proj")))

	// Create 5 tasks, each with 3 stages
	for i := 0; i < 5; i++ {
		taskID := uuid.New().String()
		require.NoError(t, s.CreateTask(makeTaskMeta(taskID, projectID, "pipeline")))

		for j := 0; j < 3; j++ {
			stage := makeStageMeta(taskID, uuid.New().String(), types.StageCoding, j)
			require.NoError(t, s.SaveStageInstance(taskID, stage))
		}
	}

	// The old approach would do 1 query for tasks + 5 queries for stages = 6 queries.
	// With LEFT JOIN it should only take 1 query.
	tasks, err := s.ListAllTasks()
	require.NoError(t, err)
	assert.Len(t, tasks, 5)

	for _, task := range tasks {
		assert.Len(t, task.Stages, 3, "task %s should have 3 stages", task.TaskID)

		// Verify order is correct
		for k := 1; k < len(task.Stages); k++ {
			assert.GreaterOrEqual(t, task.Stages[k].Order, task.Stages[k-1].Order,
				"stages should be ordered by order_idx")
		}
	}
}

func TestSearchTasksByStatus(t *testing.T) {
	s := newTestStore(t)
	projectID := uuid.New().String()
	require.NoError(t, s.CreateProject(makeProjectMeta(projectID, "proj")))

	runningIDs := make(map[string]bool)
	for i := 0; i < 3; i++ {
		taskID := uuid.New().String()
		task := makeTaskMeta(taskID, projectID, "pipeline")
		task.Status = types.TaskStatusRunning
		require.NoError(t, s.CreateTask(task))
		runningIDs[taskID] = true
	}

	pendingID := uuid.New().String()
	require.NoError(t, s.CreateTask(makeTaskMeta(pendingID, projectID, "pipeline")))

	tasks, err := s.SearchTasksByStatus(types.TaskStatusRunning)
	require.NoError(t, err)
	assert.Len(t, tasks, 3)
	for _, task := range tasks {
		assert.True(t, runningIDs[task.TaskID])
	}
}

func TestGetWorkflowSnapshot(t *testing.T) {
	s := newTestStore(t)
	projectID := uuid.New().String()
	taskID := uuid.New().String()

	require.NoError(t, s.CreateProject(makeProjectMeta(projectID, "proj")))
	require.NoError(t, s.CreateTask(makeTaskMeta(taskID, projectID, "pipeline")))

	stages := []*types.StageInstanceMeta{
		makeStageMeta(taskID, "s1", types.StageRequirement, 1),
		makeStageMeta(taskID, "s2", types.StageDesign, 2),
		makeStageMeta(taskID, "s3", types.StageCoding, 3),
	}
	for _, st := range stages {
		require.NoError(t, s.SaveStageInstance(taskID, st))
	}

	require.NoError(t, s.UpdateStageInstanceStatus(taskID, "s1", types.StageStatusCompleted))
	require.NoError(t, s.UpdateStageInstanceStatus(taskID, "s2", types.StageStatusRunning))

	snapshot, err := s.GetWorkflowSnapshot(projectID, taskID)
	require.NoError(t, err)
	assert.Equal(t, taskID, snapshot.TaskID)
	assert.Equal(t, projectID, snapshot.ProjectID)
	assert.InDelta(t, 33.33, snapshot.Progress, 0.1)
	assert.Len(t, snapshot.Timeline, 3)
}

func TestEmptyResults(t *testing.T) {
	s := newTestStore(t)

	t.Run("ListProjects empty", func(t *testing.T) {
		projects, err := s.ListProjects(map[string]interface{}{})
		require.NoError(t, err)
		assert.Empty(t, projects)
	})

	t.Run("ListAllTasks empty", func(t *testing.T) {
		tasks, err := s.ListAllTasks()
		require.NoError(t, err)
		assert.Empty(t, tasks)
	})

	t.Run("ListAllStageInstances empty", func(t *testing.T) {
		stages, err := s.ListAllStageInstances(0, 10)
		require.NoError(t, err)
		assert.Empty(t, stages)
	})
}