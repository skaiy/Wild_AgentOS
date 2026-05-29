package sandbox

import (
	"database/sql"
	"fmt"
	"strings"
	"time"

	_ "github.com/mattn/go-sqlite3"
)

type SandboxStatus string

const (
	SandboxStatusCreating   SandboxStatus = "creating"
	SandboxStatusRunning    SandboxStatus = "running"
	SandboxStatusStopped    SandboxStatus = "stopped"
	SandboxStatusError      SandboxStatus = "error"
	SandboxStatusTerminated SandboxStatus = "terminated"
)

type SandboxMeta struct {
	SandboxID     string     `json:"sandbox_id" db:"sandbox_id"`
	TaskID        string     `json:"task_id" db:"task_id"`
	ProjectID     string     `json:"project_id" db:"project_id"`
	ContainerID   string     `json:"container_id,omitempty" db:"container_id"`
	Status        SandboxStatus `json:"status" db:"status"`
	Stack         string     `json:"stack" db:"stack"`
	WorkspacePath string     `json:"workspace_path" db:"workspace_path"`
	Port          int        `json:"port,omitempty" db:"port"`
	CodeServerURL string     `json:"code_server_url,omitempty" db:"code_server_url"`
	Password      string     `json:"-" db:"password"`
	CPULimit      float64    `json:"cpu_limit" db:"cpu_limit"`
	MemoryLimitMB int        `json:"memory_limit_mb" db:"memory_limit_mb"`
	CreatedAt     time.Time  `json:"created_at" db:"created_at"`
	TerminatedAt  *time.Time `json:"terminated_at,omitempty" db:"terminated_at"`
	Error         string     `json:"error,omitempty" db:"error"`
}

type SandboxStore interface {
	CreateSandbox(meta *SandboxMeta) error
	GetSandbox(id string) (*SandboxMeta, error)
	UpdateSandboxStatus(id string, status SandboxStatus, containerID, csURL, errMsg string) error
	UpdateSandboxContainer(id string, containerID string, port int, csURL, password string) error
	ListSandboxes() ([]*SandboxMeta, error)
	ListSandboxesByTask(taskID string) ([]*SandboxMeta, error)
	DeleteSandbox(id string) error
}

type SQLiteSandboxStore struct {
	db *sql.DB
}

func buildDSN(dsn string) string {
	params := "_journal_mode=WAL&_busy_timeout=5000"
	if strings.Contains(dsn, "?") {
		return dsn + "&" + params
	}
	return dsn + "?" + params
}

func NewSQLiteSandboxStore(dsn string) (*SQLiteSandboxStore, error) {
	db, err := sql.Open("sqlite3", buildDSN(dsn))
	if err != nil {
		return nil, fmt.Errorf("open sqlite: %w", err)
	}
	if err := db.Ping(); err != nil {
		return nil, fmt.Errorf("ping sqlite: %w", err)
	}
	s := &SQLiteSandboxStore{db: db}
	if err := s.migrate(); err != nil {
		return nil, fmt.Errorf("migrate: %w", err)
	}
	return s, nil
}

func (s *SQLiteSandboxStore) Close() error {
	return s.db.Close()
}

func (s *SQLiteSandboxStore) migrate() error {
	schema := `
	CREATE TABLE IF NOT EXISTS sandboxes (
		sandbox_id     TEXT PRIMARY KEY,
		task_id        TEXT NOT NULL,
		project_id     TEXT NOT NULL,
		container_id   TEXT,
		status         TEXT NOT NULL DEFAULT 'creating',
		stack          TEXT NOT NULL DEFAULT 'base',
		workspace_path TEXT NOT NULL,
		port           INTEGER,
		code_server_url TEXT,
		password       TEXT,
		cpu_limit      REAL DEFAULT 2.0,
		memory_limit_mb INTEGER DEFAULT 2048,
		created_at     DATETIME NOT NULL,
		terminated_at  DATETIME,
		error          TEXT
	);

	CREATE INDEX IF NOT EXISTS idx_sandboxes_task_id ON sandboxes(task_id);
	CREATE INDEX IF NOT EXISTS idx_sandboxes_status ON sandboxes(status);
	`
	_, err := s.db.Exec(schema)
	return err
}

func (s *SQLiteSandboxStore) scanSandbox(scanner interface {
	Scan(dest ...interface{}) error
}) (*SandboxMeta, error) {
	var m SandboxMeta
	var containerID, codeServerURL, password, errMsg sql.NullString
	var port sql.NullInt64
	var terminatedAt sql.NullTime

	err := scanner.Scan(
		&m.SandboxID, &m.TaskID, &m.ProjectID,
		&containerID, &m.Status, &m.Stack, &m.WorkspacePath,
		&port, &codeServerURL, &password,
		&m.CPULimit, &m.MemoryLimitMB,
		&m.CreatedAt, &terminatedAt, &errMsg,
	)
	if err != nil {
		return nil, err
	}
	m.ContainerID = containerID.String
	m.Port = int(port.Int64)
	m.CodeServerURL = codeServerURL.String
	m.Password = password.String
	m.TerminatedAt = nullTimeToPtr(terminatedAt)
	m.Error = errMsg.String
	return &m, nil
}

func nullTimeToPtr(t sql.NullTime) *time.Time {
	if t.Valid {
		return &t.Time
	}
	return nil
}

func (s *SQLiteSandboxStore) CreateSandbox(meta *SandboxMeta) error {
	now := time.Now().UTC()
	if meta.CreatedAt.IsZero() {
		meta.CreatedAt = now
	}
	if meta.Status == "" {
		meta.Status = SandboxStatusCreating
	}
	if meta.Stack == "" {
		meta.Stack = "base"
	}

	var terminatedAt interface{}
	if meta.TerminatedAt != nil {
		terminatedAt = *meta.TerminatedAt
	}

	_, err := s.db.Exec(`
		INSERT INTO sandboxes (sandbox_id, task_id, project_id, container_id, status,
		                       stack, workspace_path, port, code_server_url, password,
		                       cpu_limit, memory_limit_mb, created_at, terminated_at, error)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
	`, meta.SandboxID, meta.TaskID, meta.ProjectID, meta.ContainerID, meta.Status,
		meta.Stack, meta.WorkspacePath, meta.Port, meta.CodeServerURL, meta.Password,
		meta.CPULimit, meta.MemoryLimitMB, meta.CreatedAt, terminatedAt, meta.Error)
	return err
}

func (s *SQLiteSandboxStore) GetSandbox(id string) (*SandboxMeta, error) {
	row := s.db.QueryRow(`
		SELECT sandbox_id, task_id, project_id, container_id, status,
		       stack, workspace_path, port, code_server_url, password,
		       cpu_limit, memory_limit_mb, created_at, terminated_at, error
		FROM sandboxes WHERE sandbox_id = ?
	`, id)
	return s.scanSandbox(row)
}

func (s *SQLiteSandboxStore) UpdateSandboxStatus(id string, status SandboxStatus, containerID, csURL, errMsg string) error {
	var terminatedAt interface{}
	if status == SandboxStatusTerminated || status == SandboxStatusError {
		now := time.Now().UTC()
		terminatedAt = now
	}
	_, err := s.db.Exec(`
		UPDATE sandboxes SET status = ?, container_id = COALESCE(NULLIF(?, ''), container_id),
		                     code_server_url = COALESCE(NULLIF(?, ''), code_server_url),
		                     error = COALESCE(NULLIF(?, ''), error),
		                     terminated_at = COALESCE(?, terminated_at)
		WHERE sandbox_id = ?
	`, status, containerID, csURL, errMsg, terminatedAt, id)
	return err
}

func (s *SQLiteSandboxStore) UpdateSandboxContainer(id string, containerID string, port int, csURL, password string) error {
	_, err := s.db.Exec(`
		UPDATE sandboxes SET container_id = ?, port = ?, code_server_url = ?, password = ?
		WHERE sandbox_id = ?
	`, containerID, port, csURL, password, id)
	return err
}

func (s *SQLiteSandboxStore) ListSandboxes() ([]*SandboxMeta, error) {
	rows, err := s.db.Query(`
		SELECT sandbox_id, task_id, project_id, container_id, status,
		       stack, workspace_path, port, code_server_url, password,
		       cpu_limit, memory_limit_mb, created_at, terminated_at, error
		FROM sandboxes
		ORDER BY created_at DESC
	`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var sandboxes []*SandboxMeta
	for rows.Next() {
		m, err := s.scanSandbox(rows)
		if err != nil {
			return nil, err
		}
		sandboxes = append(sandboxes, m)
	}
	if sandboxes == nil {
		return []*SandboxMeta{}, rows.Err()
	}
	return sandboxes, rows.Err()
}

func (s *SQLiteSandboxStore) ListSandboxesByTask(taskID string) ([]*SandboxMeta, error) {
	rows, err := s.db.Query(`
		SELECT sandbox_id, task_id, project_id, container_id, status,
		       stack, workspace_path, port, code_server_url, password,
		       cpu_limit, memory_limit_mb, created_at, terminated_at, error
		FROM sandboxes WHERE task_id = ?
		ORDER BY created_at DESC
	`, taskID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var sandboxes []*SandboxMeta
	for rows.Next() {
		m, err := s.scanSandbox(rows)
		if err != nil {
			return nil, err
		}
		sandboxes = append(sandboxes, m)
	}
	if sandboxes == nil {
		return []*SandboxMeta{}, rows.Err()
	}
	return sandboxes, rows.Err()
}

func (s *SQLiteSandboxStore) DeleteSandbox(id string) error {
	_, err := s.db.Exec(`DELETE FROM sandboxes WHERE sandbox_id = ?`, id)
	return err
}
