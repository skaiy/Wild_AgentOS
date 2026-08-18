package store

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"strings"
	"time"

	"github.com/agent-os/se-center/internal/types"
	_ "github.com/mattn/go-sqlite3"
)

type SQLiteMetaStore struct {
	db *sql.DB
}

func buildDSN(dsn string) string {
	params := "_journal_mode=WAL&_busy_timeout=5000"
	if strings.Contains(dsn, "?") {
		return dsn + "&" + params
	}
	return dsn + "?" + params
}

func NewSQLiteMetaStore(dsn string) (*SQLiteMetaStore, error) {
	db, err := sql.Open("sqlite3", buildDSN(dsn))
	if err != nil {
		return nil, fmt.Errorf("open sqlite: %w", err)
	}
	if err := db.Ping(); err != nil {
		return nil, fmt.Errorf("ping sqlite: %w", err)
	}
	s := &SQLiteMetaStore{db: db}
	if err := s.migrate(); err != nil {
		return nil, fmt.Errorf("migrate: %w", err)
	}
	return s, nil
}

func (s *SQLiteMetaStore) Close() error {
	return s.db.Close()
}

func (s *SQLiteMetaStore) migrate() error {
	schema := `
	CREATE TABLE IF NOT EXISTS projects (
		project_id   TEXT PRIMARY KEY,
		project_name TEXT NOT NULL,
		description  TEXT DEFAULT '',
		status       TEXT NOT NULL DEFAULT 'initialized',
		tags         TEXT DEFAULT '[]',
		extras       TEXT DEFAULT '{}',
		created_at   TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
		updated_at   TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
	);

	CREATE TABLE IF NOT EXISTS tasks (
		task_id       TEXT PRIMARY KEY,
		project_id    TEXT NOT NULL REFERENCES projects(project_id),
		pipeline_name TEXT NOT NULL DEFAULT '',
		status        TEXT NOT NULL DEFAULT 'pending',
		current_stage TEXT DEFAULT '',
		workflow_id   TEXT DEFAULT '',
		run_id        TEXT DEFAULT '',
		error         TEXT DEFAULT '',
		started_at    TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
		completed_at  TIMESTAMP,
		extras        TEXT DEFAULT '{}'
	);

	CREATE TABLE IF NOT EXISTS stage_instances (
		id                  INTEGER PRIMARY KEY AUTOINCREMENT,
		task_id             TEXT NOT NULL REFERENCES tasks(task_id),
		stage_id            TEXT NOT NULL,
		stage_type          TEXT NOT NULL,
		name                TEXT NOT NULL DEFAULT '',
		status              TEXT NOT NULL DEFAULT 'pending',
		order_idx           INTEGER NOT NULL DEFAULT 0,
		retry_count         INTEGER NOT NULL DEFAULT 0,
		duration_ms         INTEGER NOT NULL DEFAULT 0,
		contract_valid      INTEGER,
		ai_review_passed    INTEGER,
		human_review_passed INTEGER,
		output_iri          TEXT DEFAULT '',
		error               TEXT DEFAULT '',
		started_at          TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
		completed_at        TIMESTAMP
	);

	CREATE INDEX IF NOT EXISTS idx_tasks_project_id ON tasks(project_id);
	CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks(status);
	CREATE INDEX IF NOT EXISTS idx_stage_instances_task_id ON stage_instances(task_id);
	`
	_, err := s.db.Exec(schema)
	return err
}

func marshalJSON(v interface{}) string {
	b, err := json.Marshal(v)
	if err != nil {
		return "{}"
	}
	return string(b)
}

func unmarshalTags(s string) []string {
	var v []string
	if err := json.Unmarshal([]byte(s), &v); err != nil {
		return nil
	}
	return v
}

func unmarshalExtras(s string) map[string]interface{} {
	var v map[string]interface{}
	if err := json.Unmarshal([]byte(s), &v); err != nil {
		return nil
	}
	return v
}

func nullTimeToPtr(t sql.NullTime) *time.Time {
	if t.Valid {
		return &t.Time
	}
	return nil
}

func boolPtrToNullInt(v *bool) sql.NullInt64 {
	if v == nil {
		return sql.NullInt64{Valid: false}
	}
	if *v {
		return sql.NullInt64{Int64: 1, Valid: true}
	}
	return sql.NullInt64{Int64: 0, Valid: true}
}

func nullIntToBoolPtr(v sql.NullInt64) *bool {
	if !v.Valid {
		return nil
	}
	b := v.Int64 != 0
	return &b
}

func (s *SQLiteMetaStore) scanProject(scanner interface {
	Scan(dest ...interface{}) error
}) (*types.ProjectMeta, error) {
	var p types.ProjectMeta
	var tagsJSON, extrasJSON string
	err := scanner.Scan(
		&p.ProjectID, &p.ProjectName, &p.Description,
		&p.Status, &tagsJSON, &extrasJSON,
		&p.CreatedAt, &p.UpdatedAt,
	)
	if err != nil {
		return nil, err
	}
	p.Tags = unmarshalTags(tagsJSON)
	p.Extras = unmarshalExtras(extrasJSON)
	return &p, nil
}

func scanStageInstance(scanner interface {
	Scan(dest ...interface{}) error
}) (*types.StageInstanceMeta, error) {
	var st types.StageInstanceMeta
	var contractValid, aiReviewPassed, humanReviewPassed sql.NullInt64
	var completedAt sql.NullTime

	err := scanner.Scan(
		&st.TaskID, &st.StageID, &st.StageType, &st.Name, &st.Status,
		&st.Order, &st.RetryCount, &st.DurationMs,
		&contractValid, &aiReviewPassed, &humanReviewPassed,
		&st.OutputIRI, &st.Error, &st.StartedAt, &completedAt,
	)
	if err != nil {
		return nil, err
	}
	st.ContractValid = nullIntToBoolPtr(contractValid)
	st.AiReviewPassed = nullIntToBoolPtr(aiReviewPassed)
	st.HumanReviewPassed = nullIntToBoolPtr(humanReviewPassed)
	st.CompletedAt = nullTimeToPtr(completedAt)
	return &st, nil
}

func (s *SQLiteMetaStore) loadStages(taskID string) ([]types.StageInstanceMeta, error) {
	rows, err := s.db.Query(`
		SELECT task_id, stage_id, stage_type, name, status, order_idx, retry_count,
		       duration_ms, contract_valid, ai_review_passed, human_review_passed,
		       output_iri, error, started_at, completed_at
		FROM stage_instances
		WHERE task_id = ?
		ORDER BY order_idx ASC
	`, taskID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var stages []types.StageInstanceMeta
	for rows.Next() {
		st, err := scanStageInstance(rows)
		if err != nil {
			return nil, err
		}
		stages = append(stages, *st)
	}
	if stages == nil {
		return []types.StageInstanceMeta{}, rows.Err()
	}
	return stages, rows.Err()
}

func scanTask(scanner interface {
	Scan(dest ...interface{}) error
}) (*types.TaskMeta, error) {
	var t types.TaskMeta
	var completedAt sql.NullTime
	var extrasJSON string

	err := scanner.Scan(
		&t.TaskID, &t.ProjectID, &t.PipelineName,
		&t.Status, &t.CurrentStage, &t.WorkflowID,
		&t.RunID, &t.Error, &t.StartedAt,
		&completedAt, &extrasJSON,
	)
	if err != nil {
		return nil, err
	}
	t.CompletedAt = nullTimeToPtr(completedAt)
	t.Extras = unmarshalExtras(extrasJSON)
	return &t, nil
}

func (s *SQLiteMetaStore) scanFullTask(scanner interface {
	Scan(dest ...interface{}) error
}) (*types.TaskMeta, error) {
	t, err := scanTask(scanner)
	if err != nil {
		return nil, err
	}
	stages, err := s.loadStages(t.TaskID)
	if err != nil {
		return nil, err
	}
	t.Stages = stages
	return t, nil
}

// scanTaskWithStage scans a single row from a LEFT JOIN of tasks and stage_instances.
// It returns the task fields and optionally a stage instance.
func scanTaskWithStage(scanner interface {
	Scan(dest ...interface{}) error
}) (task *types.TaskMeta, stage *types.StageInstanceMeta, err error) {
	var t types.TaskMeta
	var completedAt sql.NullTime
	var extrasJSON string

	var siTaskID sql.NullString
	var siStageID, siStageType, siName, siStatus, siOutputIRI, siError sql.NullString
	var siOrderIdx, siRetryCount sql.NullInt64
	var siDurationMs sql.NullInt64
	var siContractValid, siAiReviewPassed, siHumanReviewPassed sql.NullInt64
	var siStartedAt, siCompletedAt sql.NullTime

	err = scanner.Scan(
		&t.TaskID, &t.ProjectID, &t.PipelineName,
		&t.Status, &t.CurrentStage, &t.WorkflowID,
		&t.RunID, &t.Error, &t.StartedAt,
		&completedAt, &extrasJSON,
		&siTaskID, &siStageID, &siStageType, &siName, &siStatus,
		&siOrderIdx, &siRetryCount, &siDurationMs,
		&siContractValid, &siAiReviewPassed, &siHumanReviewPassed,
		&siOutputIRI, &siError, &siStartedAt, &siCompletedAt,
	)
	if err != nil {
		return nil, nil, err
	}

	t.CompletedAt = nullTimeToPtr(completedAt)
	t.Extras = unmarshalExtras(extrasJSON)
	task = &t

	if siStageID.Valid {
		stage = &types.StageInstanceMeta{
			TaskID:            t.TaskID,
			StageID:           siStageID.String,
			StageType:         types.StageType(siStageType.String),
			Name:              siName.String,
			Status:            types.StageInstanceStatus(siStatus.String),
			Order:             int(siOrderIdx.Int64),
			RetryCount:        int(siRetryCount.Int64),
			DurationMs:        siDurationMs.Int64,
			ContractValid:     nullIntToBoolPtr(siContractValid),
			AiReviewPassed:    nullIntToBoolPtr(siAiReviewPassed),
			HumanReviewPassed: nullIntToBoolPtr(siHumanReviewPassed),
			OutputIRI:         siOutputIRI.String,
			Error:             siError.String,
			StartedAt:         siStartedAt.Time,
			CompletedAt:       nullTimeToPtr(siCompletedAt),
		}
	}

	return task, stage, nil
}

func (s *SQLiteMetaStore) CreateProject(meta *types.ProjectMeta) error {
	now := time.Now().UTC()
	if meta.CreatedAt.IsZero() {
		meta.CreatedAt = now
	}
	if meta.UpdatedAt.IsZero() {
		meta.UpdatedAt = now
	}
	if meta.Tags == nil {
		meta.Tags = []string{}
	}
	if meta.Extras == nil {
		meta.Extras = map[string]interface{}{}
	}

	_, err := s.db.Exec(`
		INSERT INTO projects (project_id, project_name, description, status, tags, extras, created_at, updated_at)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?)
	`, meta.ProjectID, meta.ProjectName, meta.Description, meta.Status,
		marshalJSON(meta.Tags), marshalJSON(meta.Extras),
		meta.CreatedAt, meta.UpdatedAt)
	return err
}

func (s *SQLiteMetaStore) GetProject(projectID string) (*types.ProjectMeta, error) {
	row := s.db.QueryRow(`
		SELECT project_id, project_name, description, status, tags, extras, created_at, updated_at
		FROM projects WHERE project_id = ?
	`, projectID)
	return s.scanProject(row)
}

func (s *SQLiteMetaStore) ListProjects(filter map[string]interface{}) ([]*types.ProjectMeta, error) {
	query := `SELECT project_id, project_name, description, status, tags, extras, created_at, updated_at FROM projects`
	var args []interface{}
	var conditions []string

	if status, ok := filter["status"]; ok {
		conditions = append(conditions, "status = ?")
		args = append(args, status)
	}
	if name, ok := filter["name"]; ok {
		conditions = append(conditions, "project_name LIKE ?")
		args = append(args, "%"+name.(string)+"%")
	}
	if projectID, ok := filter["project_id"]; ok {
		conditions = append(conditions, "project_id = ?")
		args = append(args, projectID)
	}

	if len(conditions) > 0 {
		query += " WHERE " + strings.Join(conditions, " AND ")
	}
	query += " ORDER BY created_at DESC"

	rows, err := s.db.Query(query, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var projects []*types.ProjectMeta
	for rows.Next() {
		p, err := s.scanProject(rows)
		if err != nil {
			return nil, err
		}
		projects = append(projects, p)
	}
	if projects == nil {
		return []*types.ProjectMeta{}, rows.Err()
	}
	return projects, rows.Err()
}

func (s *SQLiteMetaStore) UpdateProjectStatus(projectID string, status types.ProjectStatus) error {
	_, err := s.db.Exec(`
		UPDATE projects SET status = ?, updated_at = ? WHERE project_id = ?
	`, status, time.Now().UTC(), projectID)
	return err
}

func (s *SQLiteMetaStore) UpdateProject(projectID string, name, description string) error {
	_, err := s.db.Exec(`
		UPDATE projects SET project_name = COALESCE(NULLIF(?, ''), project_name),
		                    description = COALESCE(NULLIF(?, ''), description),
		                    updated_at = ?
		WHERE project_id = ?
	`, name, description, time.Now().UTC(), projectID)
	return err
}

func (s *SQLiteMetaStore) DeleteProject(projectID string) error {
	tx, err := s.db.Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback()

	if _, err := tx.Exec(`DELETE FROM stage_instances WHERE task_id IN (SELECT task_id FROM tasks WHERE project_id = ?)`, projectID); err != nil {
		return err
	}
	if _, err := tx.Exec(`DELETE FROM tasks WHERE project_id = ?`, projectID); err != nil {
		return err
	}
	if _, err := tx.Exec(`DELETE FROM projects WHERE project_id = ?`, projectID); err != nil {
		return err
	}
	return tx.Commit()
}

func (s *SQLiteMetaStore) CreateTask(meta *types.TaskMeta) error {
	now := time.Now().UTC()
	if meta.StartedAt.IsZero() {
		meta.StartedAt = now
	}
	if meta.Extras == nil {
		meta.Extras = map[string]interface{}{}
	}

	var completedAt interface{}
	if meta.CompletedAt != nil {
		completedAt = *meta.CompletedAt
	}

	_, err := s.db.Exec(`
		INSERT INTO tasks (task_id, project_id, pipeline_name, status, current_stage,
		                   workflow_id, run_id, error, started_at, completed_at, extras)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
	`, meta.TaskID, meta.ProjectID, meta.PipelineName, meta.Status,
		meta.CurrentStage, meta.WorkflowID, meta.RunID, meta.Error,
		meta.StartedAt, completedAt, marshalJSON(meta.Extras))
	return err
}

func (s *SQLiteMetaStore) GetTask(taskID string) (*types.TaskMeta, error) {
	row := s.db.QueryRow(`
		SELECT task_id, project_id, pipeline_name, status, current_stage,
		       workflow_id, run_id, error, started_at, completed_at, extras
		FROM tasks WHERE task_id = ?
	`, taskID)
	return s.scanFullTask(row)
}

func (s *SQLiteMetaStore) ListTasks(projectID string) ([]*types.TaskMeta, error) {
	rows, err := s.db.Query(`
		SELECT task_id, project_id, pipeline_name, status, current_stage,
		       workflow_id, run_id, error, started_at, completed_at, extras
		FROM tasks WHERE project_id = ?
		ORDER BY started_at DESC
	`, projectID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var tasks []*types.TaskMeta
	for rows.Next() {
		t, err := scanTask(rows)
		if err != nil {
			return nil, err
		}
		tasks = append(tasks, t)
	}
	if tasks == nil {
		return []*types.TaskMeta{}, rows.Err()
	}
	return tasks, rows.Err()
}

// ListAllTasks uses a single LEFT JOIN query (N+1 optimized) to load all tasks
// with their stages in one round trip.
func (s *SQLiteMetaStore) ListAllTasks() ([]*types.TaskMeta, error) {
	rows, err := s.db.Query(`
		SELECT
			t.task_id, t.project_id, t.pipeline_name,
			t.status, t.current_stage, t.workflow_id,
			t.run_id, t.error, t.started_at, t.completed_at, t.extras,
			si.task_id, si.stage_id, si.stage_type, si.name, si.status,
			si.order_idx, si.retry_count, si.duration_ms,
			si.contract_valid, si.ai_review_passed, si.human_review_passed,
			si.output_iri, si.error, si.started_at, si.completed_at
		FROM tasks t
		LEFT JOIN stage_instances si ON si.task_id = t.task_id
		ORDER BY t.started_at DESC, si.order_idx ASC
	`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	taskMap := make(map[string]*types.TaskMeta)
	var taskOrder []string

	for rows.Next() {
		task, stage, err := scanTaskWithStage(rows)
		if err != nil {
			return nil, err
		}

		existing, ok := taskMap[task.TaskID]
		if !ok {
			task.Stages = []types.StageInstanceMeta{}
			taskMap[task.TaskID] = task
			taskOrder = append(taskOrder, task.TaskID)
			existing = task
		}

		if stage != nil {
			existing.Stages = append(existing.Stages, *stage)
		}
	}

	if err := rows.Err(); err != nil {
		return nil, err
	}

	tasks := make([]*types.TaskMeta, 0, len(taskOrder))
	for _, tid := range taskOrder {
		tasks = append(tasks, taskMap[tid])
	}

	if len(tasks) == 0 {
		return []*types.TaskMeta{}, nil
	}
	return tasks, nil
}

func (s *SQLiteMetaStore) UpdateTaskStatus(taskID string, status types.TaskStatus, currentStage string) error {
	var completedAt interface{}
	if status == types.TaskStatusCompleted || status == types.TaskStatusFailed || status == types.TaskStatusRolledBack {
		now := time.Now().UTC()
		completedAt = now
	}
	_, err := s.db.Exec(`
		UPDATE tasks SET status = ?, current_stage = ?, completed_at = COALESCE(?, completed_at)
		WHERE task_id = ?
	`, status, currentStage, completedAt, taskID)
	return err
}

func (s *SQLiteMetaStore) UpdateTaskWorkflow(taskID string, workflowID, runID string) error {
	_, err := s.db.Exec(`
		UPDATE tasks SET workflow_id = ?, run_id = ? WHERE task_id = ?
	`, workflowID, runID, taskID)
	return err
}

func (s *SQLiteMetaStore) SaveStageInstance(taskID string, meta *types.StageInstanceMeta) error {
	now := time.Now().UTC()
	if meta.StartedAt.IsZero() {
		meta.StartedAt = now
	}

	var completedAt interface{}
	if meta.CompletedAt != nil {
		completedAt = *meta.CompletedAt
	}

	_, err := s.db.Exec(`
		INSERT INTO stage_instances (task_id, stage_id, stage_type, name, status,
		                             order_idx, retry_count, duration_ms,
		                             contract_valid, ai_review_passed, human_review_passed,
		                             output_iri, error, started_at, completed_at)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
	`, taskID, meta.StageID, meta.StageType, meta.Name, meta.Status,
		meta.Order, meta.RetryCount, meta.DurationMs,
		boolPtrToNullInt(meta.ContractValid), boolPtrToNullInt(meta.AiReviewPassed),
		boolPtrToNullInt(meta.HumanReviewPassed),
		meta.OutputIRI, meta.Error, meta.StartedAt, completedAt)
	return err
}

func (s *SQLiteMetaStore) UpdateStageInstanceStatus(taskID, stageID string, status types.StageInstanceStatus) error {
	var completedAt interface{}
	if status == types.StageStatusCompleted || status == types.StageStatusFailed || status == types.StageStatusRolledBack {
		completedAt = time.Now().UTC()
	}
	_, err := s.db.Exec(`
		UPDATE stage_instances SET status = ?, completed_at = COALESCE(?, completed_at)
		WHERE task_id = ? AND stage_id = ?
	`, status, completedAt, taskID, stageID)
	return err
}

func (s *SQLiteMetaStore) GetStageInstance(taskID, stageID string) (*types.StageInstanceMeta, error) {
	row := s.db.QueryRow(`
		SELECT task_id, stage_id, stage_type, name, status, order_idx, retry_count,
		       duration_ms, contract_valid, ai_review_passed, human_review_passed,
		       output_iri, error, started_at, completed_at
		FROM stage_instances
		WHERE task_id = ? AND stage_id = ?
	`, taskID, stageID)

	return scanStageInstance(row)
}

func (s *SQLiteMetaStore) ListStageInstances(taskID string) ([]*types.StageInstanceMeta, error) {
	rows, err := s.db.Query(`
		SELECT task_id, stage_id, stage_type, name, status, order_idx, retry_count,
		       duration_ms, contract_valid, ai_review_passed, human_review_passed,
		       output_iri, error, started_at, completed_at
		FROM stage_instances
		WHERE task_id = ?
		ORDER BY order_idx ASC
	`, taskID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var stages []*types.StageInstanceMeta
	for rows.Next() {
		st, err := scanStageInstance(rows)
		if err != nil {
			return nil, err
		}
		stages = append(stages, st)
	}
	if stages == nil {
		return []*types.StageInstanceMeta{}, rows.Err()
	}
	return stages, rows.Err()
}

func (s *SQLiteMetaStore) ListAllStageInstances(offset, limit int) ([]*types.StageInstanceMeta, error) {
	if limit <= 0 {
		limit = 100
	}
	if offset < 0 {
		offset = 0
	}

	rows, err := s.db.Query(`
		SELECT task_id, stage_id, stage_type, name, status, order_idx,
		       retry_count, duration_ms, contract_valid,
		       ai_review_passed, human_review_passed,
		       output_iri, error, started_at, completed_at
		FROM stage_instances
		ORDER BY started_at DESC
		LIMIT ? OFFSET ?
	`, limit, offset)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var stages []*types.StageInstanceMeta
	for rows.Next() {
		st, err := scanStageInstance(rows)
		if err != nil {
			return nil, err
		}
		stages = append(stages, st)
	}
	if stages == nil {
		return []*types.StageInstanceMeta{}, rows.Err()
	}
	return stages, rows.Err()
}

func (s *SQLiteMetaStore) SearchTasksByStatus(status types.TaskStatus) ([]*types.TaskMeta, error) {
	rows, err := s.db.Query(`
		SELECT task_id, project_id, pipeline_name, status, current_stage,
		       workflow_id, run_id, error, started_at, completed_at, extras
		FROM tasks WHERE status = ?
		ORDER BY started_at DESC
	`, status)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var tasks []*types.TaskMeta
	for rows.Next() {
		t, err := scanTask(rows)
		if err != nil {
			return nil, err
		}
		stages, err := s.loadStages(t.TaskID)
		if err != nil {
			return nil, err
		}
		t.Stages = stages
		tasks = append(tasks, t)
	}
	if tasks == nil {
		return []*types.TaskMeta{}, rows.Err()
	}
	return tasks, rows.Err()
}

func (s *SQLiteMetaStore) GetWorkflowSnapshot(projectID, taskID string) (*types.WorkflowSnapshot, error) {
	row := s.db.QueryRow(`
		SELECT task_id, project_id, pipeline_name, status, current_stage,
		       workflow_id, run_id, error, started_at, completed_at, extras
		FROM tasks WHERE task_id = ? AND project_id = ?
	`, taskID, projectID)

	t, err := s.scanFullTask(row)
	if err != nil {
		return nil, err
	}

	snapshot := &types.WorkflowSnapshot{
		TaskMeta: *t,
	}

	if len(t.Stages) > 0 {
		var completedCount int
		for _, st := range t.Stages {
			if st.Status == types.StageStatusCompleted {
				completedCount++
			}
		}
		snapshot.Progress = float64(completedCount) / float64(len(t.Stages)) * 100.0
	}

	for _, st := range t.Stages {
		duration := st.DurationMs
		if duration == 0 && st.CompletedAt != nil {
			duration = st.CompletedAt.Sub(st.StartedAt).Milliseconds()
		}
		snapshot.Timeline = append(snapshot.Timeline, types.StageTimeline{
			StageID:    st.StageID,
			Name:       st.Name,
			Status:     string(st.Status),
			StartedAt:  st.StartedAt,
			DurationMs: duration,
		})
	}
	if snapshot.Timeline == nil {
		snapshot.Timeline = []types.StageTimeline{}
	}

	return snapshot, nil
}