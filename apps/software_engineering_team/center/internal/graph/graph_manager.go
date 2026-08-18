package graph

import (
	"context"
	"fmt"
	"sync"

	"github.com/agent-os/se-center/internal/grpc"
	pb "github.com/agent-os/se-center/proto/seapp"
)

// IRIDelta — 变更集项
type IRIDelta struct {
	Action  string `json:"action"`            // "create" | "update" | "delete"
	IRI     string `json:"iri"`
	JSONLD  []byte `json:"jsonld,omitempty"`
	Version int    `json:"version"`
}

// SyncRequest — 同步请求
type SyncRequest struct {
	AgentID string    `json:"agent_id"`
	TaskID  string    `json:"task_id"`
	Deltas  []IRIDelta `json:"deltas"`
}

// SyncResponse — 同步响应
type SyncResponse struct {
	Status     string   `json:"status"`               // "accepted" | "rejected"
	MergedIRIs []string `json:"merged_iris"`
	Violations []string `json:"violations,omitempty"`
}

// GraphManager — 全局图谱管理器
type GraphManager struct {
	grpcClient *grpc.Client
	mu         sync.RWMutex
	versionMap map[string]int // iri -> current version
}

func NewGraphManager(grpcClient *grpc.Client) *GraphManager {
	return &GraphManager{
		grpcClient: grpcClient,
		versionMap: make(map[string]int),
	}
}

// SyncFromEdge — 同步来自边缘的变更集
// 1. 委托 Agent OS 内核进行 SHACL 校验
// 2. 校验通过 → 合并到全局图谱并更新版本链
// 3. 校验失败 → 返回 violations
func (m *GraphManager) SyncFromEdge(ctx context.Context, req SyncRequest) (*SyncResponse, error) {
	if m.grpcClient == nil {
		return nil, fmt.Errorf("gRPC client not available")
	}

	var mergedIRIs []string
	var violations []string

	for _, delta := range req.Deltas {
		if delta.Action == "delete" {
			m.mu.Lock()
			delete(m.versionMap, delta.IRI)
			m.mu.Unlock()
			mergedIRIs = append(mergedIRIs, delta.IRI)
			continue
		}

		if len(delta.JSONLD) > 0 {
			validateReq := &pb.ValidateContractRequest{
				OutputIri:  delta.IRI,
				SchemaName: "shacl",
				OutputJson: delta.JSONLD,
			}
			validateResp, err := m.grpcClient.ValidateContract(ctx, validateReq)
			if err != nil {
				violations = append(violations, fmt.Sprintf("validate %s: %v", delta.IRI, err))
				continue
			}
			if !validateResp.Valid {
				violations = append(violations, validateResp.Violations...)
				continue
			}
		}

		m.mu.Lock()
		currentVer := m.versionMap[delta.IRI]
		newVer := currentVer + 1
		if delta.Action == "create" {
			newVer = 1
		}
		m.versionMap[delta.IRI] = newVer
		m.mu.Unlock()

		mergedIRIs = append(mergedIRIs, delta.IRI)
	}

	status := "accepted"
	if len(violations) > 0 {
		status = "rejected"
	}

	return &SyncResponse{
		Status:     status,
		MergedIRIs: mergedIRIs,
		Violations: violations,
	}, nil
}

// GetVersion — 获取指定 IRI 的当前版本号
func (m *GraphManager) GetVersion(iri string) int {
	m.mu.RLock()
	defer m.mu.RUnlock()
	return m.versionMap[iri]
}

// ListVersions — 返回当前所有 IRI 的版本快照
func (m *GraphManager) ListVersions() map[string]int {
	m.mu.RLock()
	defer m.mu.RUnlock()
	snapshot := make(map[string]int, len(m.versionMap))
	for k, v := range m.versionMap {
		snapshot[k] = v
	}
	return snapshot
}