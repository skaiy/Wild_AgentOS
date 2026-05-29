package store

import (
	"testing"
	"time"

	"github.com/agent-os/se-app/internal/types"
)

func newTestStore(t *testing.T) *SQLiteMetaStore {
	t.Helper()
	dbPath := t.TempDir() + "/test.db"
	dsn := "file:" + dbPath + "?cache=shared"
	s, err := NewSQLiteMetaStore(dsn)
	if err != nil {
		t.Fatalf("NewSQLiteMetaStore: %v", err)
	}
	t.Cleanup(func() {
		s.Close()
	})
	return s
}

func TestCreateAndGetProject(t *testing.T) {
	s := newTestStore(t)

	meta := &types.ProjectMeta{
		ProjectID:   "proj-001",
		ProjectName: "Test Project",
		Description: "A test project",
		Status:      types.ProjectStatusInit,
		Tags:        []string{"go", "test"},
		Extras:      map[string]interface{}{"key": "value"},
	}

	if err := s.CreateProject(meta); err != nil {
		t.Fatalf("CreateProject: %v", err)
	}

	got, err := s.GetProject("proj-001")
	if err != nil {
		t.Fatalf("GetProject: %v", err)
	}

	if got.ProjectID != "proj-001" {
		t.Errorf("ProjectID = %q, want %q", got.ProjectID, "proj-001")
	}
	if got.ProjectName != "Test Project" {
		t.Errorf("ProjectName = %q, want %q", got.ProjectName, "Test Project")
	}
	if got.Status != types.ProjectStatusInit {
		t.Errorf("Status = %q, want %q", got.Status, types.ProjectStatusInit)
	}
	if len(got.Tags) != 2 || got.Tags[0] != "go" {
		t.Errorf("Tags = %v, want [go test]", got.Tags)
	}
	if got.Extras["key"] != "value" {
		t.Errorf("Extras[key] = %v, want value", got.Extras["key"])
	}
	if got.CreatedAt.IsZero() {
		t.Error("CreatedAt should not be zero")
	}
}

func TestGetProjectNotFound(t *testing.T) {
	s := newTestStore(t)

	_, err := s.GetProject("nonexistent")
	if err == nil {
		t.Fatal("expected error for nonexistent project")
	}
}

func TestListProjects(t *testing.T) {
	s := newTestStore(t)

	projects := []*types.ProjectMeta{
		{ProjectID: "p1", ProjectName: "One", Status: types.ProjectStatusInit},
		{ProjectID: "p2", ProjectName: "Two", Status: types.ProjectStatusRunning},
		{ProjectID: "p3", ProjectName: "Three", Status: types.ProjectStatusInit},
	}
	for _, p := range projects {
		if err := s.CreateProject(p); err != nil {
			t.Fatalf("CreateProject %s: %v", p.ProjectID, err)
		}
	}

	all, err := s.ListProjects(map[string]interface{}{})
	if err != nil {
		t.Fatalf("ListProjects: %v", err)
	}
	if len(all) != 3 {
		t.Errorf("got %d projects, want 3", len(all))
	}

	filtered, err := s.ListProjects(map[string]interface{}{"status": "initialized"})
	if err != nil {
		t.Fatalf("ListProjects with filter: %v", err)
	}
	if len(filtered) != 2 {
		t.Errorf("got %d filtered projects, want 2", len(filtered))
	}
}

func TestUpdateProjectStatus(t *testing.T) {
	s := newTestStore(t)

	if err := s.CreateProject(&types.ProjectMeta{ProjectID: "p1", ProjectName: "P1"}); err != nil {
		t.Fatal(err)
	}

	if err := s.UpdateProjectStatus("p1", types.ProjectStatusRunning); err != nil {
		t.Fatalf("UpdateProjectStatus: %v", err)
	}

	got, err := s.GetProject("p1")
	if err != nil {
		t.Fatal(err)
	}
	if got.Status != types.ProjectStatusRunning {
		t.Errorf("Status = %q, want %q", got.Status, types.ProjectStatusRunning)
	}
}

func TestDeleteProject(t *testing.T) {
	s := newTestStore(t)

	if err := s.CreateProject(&types.ProjectMeta{ProjectID: "p1", ProjectName: "P1"}); err != nil {
		t.Fatal(err)
	}
	if err := s.CreateTask(&types.TaskMeta{TaskID: "t1", ProjectID: "p1", PipelineName: "pipe"}); err != nil {
		t.Fatal(err)
	}
	if err := s.SaveStageInstance("t1", &types.StageInstanceMeta{
		StageID:   "s1",
		StageType: types.StageCoding,
		Name:      "Code",
	}); err != nil {
		t.Fatal(err)
	}

	if err := s.DeleteProject("p1"); err != nil {
		t.Fatalf("DeleteProject: %v", err)
	}

	_, err := s.GetProject("p1")
	if err == nil {
		t.Error("expected error after delete")
	}

	tasks, err := s.ListTasks("p1")
	if err != nil {
		t.Fatal(err)
	}
	if len(tasks) != 0 {
		t.Errorf("expected 0 tasks, got %d", len(tasks))
	}
}

func TestCreateAndGetTask(t *testing.T) {
	s := newTestStore(t)

	if err := s.CreateProject(&types.ProjectMeta{ProjectID: "proj-1", ProjectName: "Proj"}); err != nil {
		t.Fatal(err)
	}

	now := time.Now().UTC()
	task := &types.TaskMeta{
		TaskID:       "task-001",
		ProjectID:    "proj-1",
		PipelineName: "default",
		Status:       types.TaskStatusPending,
		CurrentStage: "design",
		WorkflowID:   "wf-1",
		RunID:        "run-1",
		StartedAt:    now,
	}

	if err := s.CreateTask(task); err != nil {
		t.Fatalf("CreateTask: %v", err)
	}

	stage := &types.StageInstanceMeta{
		StageID:   "design-1",
		StageType: types.StageDesign,
		Name:      "Design Phase",
		Status:    types.StageStatusPending,
		Order:     0,
	}
	if err := s.SaveStageInstance("task-001", stage); err != nil {
		t.Fatalf("SaveStageInstance: %v", err)
	}

	got, err := s.GetTask("task-001")
	if err != nil {
		t.Fatalf("GetTask: %v", err)
	}

	if got.TaskID != "task-001" {
		t.Errorf("TaskID = %q", got.TaskID)
	}
	if got.ProjectID != "proj-1" {
		t.Errorf("ProjectID = %q", got.ProjectID)
	}
	if got.WorkflowID != "wf-1" {
		t.Errorf("WorkflowID = %q", got.WorkflowID)
	}
	if len(got.Stages) != 1 {
		t.Errorf("expected 1 stage, got %d", len(got.Stages))
	}
	if got.Stages[0].StageID != "design-1" {
		t.Errorf("StageID = %q", got.Stages[0].StageID)
	}
}

func TestListTasks(t *testing.T) {
	s := newTestStore(t)

	if err := s.CreateProject(&types.ProjectMeta{ProjectID: "proj-1", ProjectName: "Proj"}); err != nil {
		t.Fatal(err)
	}

	tasks := []*types.TaskMeta{
		{TaskID: "t1", ProjectID: "proj-1", PipelineName: "pipe1"},
		{TaskID: "t2", ProjectID: "proj-1", PipelineName: "pipe2"},
	}
	for _, task := range tasks {
		if err := s.CreateTask(task); err != nil {
			t.Fatalf("CreateTask %s: %v", task.TaskID, err)
		}
	}

	got, err := s.ListTasks("proj-1")
	if err != nil {
		t.Fatalf("ListTasks: %v", err)
	}
	if len(got) != 2 {
		t.Errorf("got %d tasks, want 2", len(got))
	}
}

func TestUpdateTaskStatus(t *testing.T) {
	s := newTestStore(t)

	if err := s.CreateProject(&types.ProjectMeta{ProjectID: "proj-1", ProjectName: "Proj"}); err != nil {
		t.Fatal(err)
	}
	if err := s.CreateTask(&types.TaskMeta{TaskID: "t1", ProjectID: "proj-1", PipelineName: "pipe"}); err != nil {
		t.Fatal(err)
	}

	if err := s.UpdateTaskStatus("t1", types.TaskStatusRunning, "coding"); err != nil {
		t.Fatalf("UpdateTaskStatus: %v", err)
	}

	got, err := s.GetTask("t1")
	if err != nil {
		t.Fatal(err)
	}
	if got.Status != types.TaskStatusRunning {
		t.Errorf("Status = %q, want %q", got.Status, types.TaskStatusRunning)
	}
	if got.CurrentStage != "coding" {
		t.Errorf("CurrentStage = %q, want coding", got.CurrentStage)
	}
	if got.CompletedAt != nil {
		t.Error("CompletedAt should be nil for running status")
	}

	if err := s.UpdateTaskStatus("t1", types.TaskStatusCompleted, ""); err != nil {
		t.Fatalf("UpdateTaskStatus to completed: %v", err)
	}
	got, err = s.GetTask("t1")
	if err != nil {
		t.Fatal(err)
	}
	if got.CompletedAt == nil {
		t.Error("CompletedAt should be set for completed status")
	}
}

func TestUpdateTaskWorkflow(t *testing.T) {
	s := newTestStore(t)

	if err := s.CreateProject(&types.ProjectMeta{ProjectID: "proj-1", ProjectName: "Proj"}); err != nil {
		t.Fatal(err)
	}
	if err := s.CreateTask(&types.TaskMeta{TaskID: "t1", ProjectID: "proj-1", PipelineName: "pipe"}); err != nil {
		t.Fatal(err)
	}

	if err := s.UpdateTaskWorkflow("t1", "wf-42", "run-99"); err != nil {
		t.Fatalf("UpdateTaskWorkflow: %v", err)
	}

	got, err := s.GetTask("t1")
	if err != nil {
		t.Fatal(err)
	}
	if got.WorkflowID != "wf-42" {
		t.Errorf("WorkflowID = %q", got.WorkflowID)
	}
	if got.RunID != "run-99" {
		t.Errorf("RunID = %q", got.RunID)
	}
}

func TestStageInstanceLifecycle(t *testing.T) {
	s := newTestStore(t)

	if err := s.CreateProject(&types.ProjectMeta{ProjectID: "proj-1", ProjectName: "Proj"}); err != nil {
		t.Fatal(err)
	}
	if err := s.CreateTask(&types.TaskMeta{TaskID: "t1", ProjectID: "proj-1", PipelineName: "pipe"}); err != nil {
		t.Fatal(err)
	}

	stage := &types.StageInstanceMeta{
		StageID:   "stage-1",
		StageType: types.StageCoding,
		Name:      "Coding Stage",
		Status:    types.StageStatusPending,
		Order:     1,
	}
	if err := s.SaveStageInstance("t1", stage); err != nil {
		t.Fatalf("SaveStageInstance: %v", err)
	}

	got, err := s.GetStageInstance("t1", "stage-1")
	if err != nil {
		t.Fatalf("GetStageInstance: %v", err)
	}
	if got.StageID != "stage-1" {
		t.Errorf("StageID = %q", got.StageID)
	}
	if got.StageType != types.StageCoding {
		t.Errorf("StageType = %q", got.StageType)
	}

	if err := s.UpdateStageInstanceStatus("t1", "stage-1", types.StageStatusRunning); err != nil {
		t.Fatalf("UpdateStageInstanceStatus: %v", err)
	}
	got, err = s.GetStageInstance("t1", "stage-1")
	if err != nil {
		t.Fatal(err)
	}
	if got.Status != types.StageStatusRunning {
		t.Errorf("Status = %q", got.Status)
	}
}

func TestListStageInstances(t *testing.T) {
	s := newTestStore(t)

	if err := s.CreateProject(&types.ProjectMeta{ProjectID: "proj-1", ProjectName: "Proj"}); err != nil {
		t.Fatal(err)
	}
	if err := s.CreateTask(&types.TaskMeta{TaskID: "t1", ProjectID: "proj-1", PipelineName: "pipe"}); err != nil {
		t.Fatal(err)
	}

	stages := []*types.StageInstanceMeta{
		{StageID: "s1", StageType: types.StageRequirement, Name: "Req", Order: 0},
		{StageID: "s2", StageType: types.StageDesign, Name: "Design", Order: 1},
		{StageID: "s3", StageType: types.StageCoding, Name: "Code", Order: 2},
	}
	for _, st := range stages {
		if err := s.SaveStageInstance("t1", st); err != nil {
			t.Fatalf("SaveStageInstance: %v", err)
		}
	}

	got, err := s.ListStageInstances("t1")
	if err != nil {
		t.Fatalf("ListStageInstances: %v", err)
	}
	if len(got) != 3 {
		t.Fatalf("expected 3 stages, got %d", len(got))
	}
	for i, st := range got {
		if st.Order != i {
			t.Errorf("stage[%d].Order = %d, want %d", i, st.Order, i)
		}
	}
}

func TestSearchTasksByStatus(t *testing.T) {
	s := newTestStore(t)

	if err := s.CreateProject(&types.ProjectMeta{ProjectID: "proj-1", ProjectName: "Proj"}); err != nil {
		t.Fatal(err)
	}

	tasks := []*types.TaskMeta{
		{TaskID: "t1", ProjectID: "proj-1", PipelineName: "pipe", Status: types.TaskStatusPending},
		{TaskID: "t2", ProjectID: "proj-1", PipelineName: "pipe", Status: types.TaskStatusRunning},
		{TaskID: "t3", ProjectID: "proj-1", PipelineName: "pipe", Status: types.TaskStatusPending},
	}
	for _, task := range tasks {
		if err := s.CreateTask(task); err != nil {
			t.Fatalf("CreateTask: %v", err)
		}
	}

	got, err := s.SearchTasksByStatus(types.TaskStatusPending)
	if err != nil {
		t.Fatalf("SearchTasksByStatus: %v", err)
	}
	if len(got) != 2 {
		t.Errorf("expected 2 pending tasks, got %d", len(got))
	}

	got, err = s.SearchTasksByStatus(types.TaskStatusRunning)
	if err != nil {
		t.Fatalf("SearchTasksByStatus: %v", err)
	}
	if len(got) != 1 {
		t.Errorf("expected 1 running task, got %d", len(got))
	}
}

func TestGetWorkflowSnapshot(t *testing.T) {
	s := newTestStore(t)

	if err := s.CreateProject(&types.ProjectMeta{ProjectID: "proj-1", ProjectName: "Proj"}); err != nil {
		t.Fatal(err)
	}
	if err := s.CreateTask(&types.TaskMeta{TaskID: "t1", ProjectID: "proj-1", PipelineName: "pipe", Status: types.TaskStatusRunning}); err != nil {
		t.Fatal(err)
	}

	startedAt := time.Now().UTC().Add(-10 * time.Minute)
	stages := []*types.StageInstanceMeta{
		{
			StageID: "s1", StageType: types.StageRequirement, Name: "Requirements",
			Status: types.StageStatusCompleted, Order: 0, DurationMs: 120000, StartedAt: startedAt,
		},
		{
			StageID: "s2", StageType: types.StageDesign, Name: "Design",
			Status: types.StageStatusRunning, Order: 1, StartedAt: startedAt.Add(2 * time.Minute),
		},
		{
			StageID: "s3", StageType: types.StageCoding, Name: "Coding",
			Status: types.StageStatusPending, Order: 2, StartedAt: startedAt.Add(4 * time.Minute),
		},
	}
	for _, st := range stages {
		if err := s.SaveStageInstance("t1", st); err != nil {
			t.Fatalf("SaveStageInstance: %v", err)
		}
	}

	snapshot, err := s.GetWorkflowSnapshot("proj-1", "t1")
	if err != nil {
		t.Fatalf("GetWorkflowSnapshot: %v", err)
	}

	if snapshot.TaskID != "t1" {
		t.Errorf("TaskID = %q", snapshot.TaskID)
	}
	if snapshot.Progress != 33.33333333333333 {
		t.Errorf("Progress = %f, want ~33.33", snapshot.Progress)
	}
	if len(snapshot.Timeline) != 3 {
		t.Fatalf("expected 3 timeline entries, got %d", len(snapshot.Timeline))
	}
	if snapshot.Timeline[0].StageID != "s1" {
		t.Errorf("Timeline[0].StageID = %q", snapshot.Timeline[0].StageID)
	}
	if snapshot.Timeline[0].DurationMs != 120000 {
		t.Errorf("Timeline[0].DurationMs = %d", snapshot.Timeline[0].DurationMs)
	}
	if snapshot.Timeline[1].Status != "running" {
		t.Errorf("Timeline[1].Status = %q", snapshot.Timeline[1].Status)
	}
}

func TestStageBoolPointers(t *testing.T) {
	s := newTestStore(t)

	if err := s.CreateProject(&types.ProjectMeta{ProjectID: "proj-1", ProjectName: "Proj"}); err != nil {
		t.Fatal(err)
	}
	if err := s.CreateTask(&types.TaskMeta{TaskID: "t1", ProjectID: "proj-1", PipelineName: "pipe"}); err != nil {
		t.Fatal(err)
	}

	trueVal := true
	falseVal := false

	stage := &types.StageInstanceMeta{
		StageID:          "s1",
		StageType:        types.StageReview,
		Name:             "Review",
		Status:           types.StageStatusCompleted,
		Order:            0,
		ContractValid:    &trueVal,
		AiReviewPassed:   &trueVal,
		HumanReviewPassed: &falseVal,
	}
	if err := s.SaveStageInstance("t1", stage); err != nil {
		t.Fatal(err)
	}

	got, err := s.GetStageInstance("t1", "s1")
	if err != nil {
		t.Fatal(err)
	}

	if got.ContractValid == nil || *got.ContractValid != true {
		t.Error("ContractValid should be true")
	}
	if got.AiReviewPassed == nil || *got.AiReviewPassed != true {
		t.Error("AiReviewPassed should be true")
	}
	if got.HumanReviewPassed == nil || *got.HumanReviewPassed != false {
		t.Error("HumanReviewPassed should be false")
	}

	nilStage := &types.StageInstanceMeta{
		StageID:   "s2",
		StageType: types.StageCoding,
		Name:      "Coding",
		Status:    types.StageStatusPending,
		Order:     1,
	}
	if err := s.SaveStageInstance("t1", nilStage); err != nil {
		t.Fatal(err)
	}
	gotNil, err := s.GetStageInstance("t1", "s2")
	if err != nil {
		t.Fatal(err)
	}
	if gotNil.ContractValid != nil {
		t.Error("ContractValid should be nil")
	}
	if gotNil.AiReviewPassed != nil {
		t.Error("AiReviewPassed should be nil")
	}
	if gotNil.HumanReviewPassed != nil {
		t.Error("HumanReviewPassed should be nil")
	}
}

func TestTaskCompletedAt(t *testing.T) {
	s := newTestStore(t)

	if err := s.CreateProject(&types.ProjectMeta{ProjectID: "proj-1", ProjectName: "Proj"}); err != nil {
		t.Fatal(err)
	}

	completedAt := time.Now().UTC().Add(-1 * time.Hour)
	task := &types.TaskMeta{
		TaskID:      "t1",
		ProjectID:   "proj-1",
		PipelineName: "pipe",
		Status:      types.TaskStatusCompleted,
		CompletedAt: &completedAt,
	}
	if err := s.CreateTask(task); err != nil {
		t.Fatal(err)
	}

	got, err := s.GetTask("t1")
	if err != nil {
		t.Fatal(err)
	}
	if got.CompletedAt == nil {
		t.Fatal("CompletedAt should not be nil")
	}
	if !got.CompletedAt.Equal(completedAt) {
		t.Errorf("CompletedAt = %v, want %v", got.CompletedAt, completedAt)
	}
}