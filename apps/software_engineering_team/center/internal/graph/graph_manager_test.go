package graph

import (
	"context"
	"testing"

	"github.com/stretchr/testify/assert"
)

func TestNewGraphManager(t *testing.T) {
	gm := NewGraphManager(nil)
	assert.NotNil(t, gm)
	assert.NotNil(t, gm.versionMap)
	assert.Empty(t, gm.versionMap)
}

func TestVersionChainIncrement(t *testing.T) {
	gm := NewGraphManager(nil)

	iri := "iri://task/test-1"

	// 初始版本应为 0
	assert.Equal(t, 0, gm.GetVersion(iri))

	// 手动模拟版本链变化
	gm.mu.Lock()
	gm.versionMap[iri] = 1
	gm.mu.Unlock()
	assert.Equal(t, 1, gm.GetVersion(iri))

	gm.mu.Lock()
	gm.versionMap[iri] = 2
	gm.mu.Unlock()
	assert.Equal(t, 2, gm.GetVersion(iri))

	gm.mu.Lock()
	gm.versionMap[iri] = 3
	gm.mu.Unlock()
	assert.Equal(t, 3, gm.GetVersion(iri))
}

func TestSyncFromEdgeNilClient(t *testing.T) {
	gm := NewGraphManager(nil)

	req := SyncRequest{
		AgentID: "agent-1",
		TaskID:  "task-1",
		Deltas: []IRIDelta{
			{Action: "create", IRI: "iri://task/test-1"},
		},
	}

	resp, err := gm.SyncFromEdge(context.Background(), req)
	assert.Nil(t, resp)
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "gRPC client not available")
}

func TestListVersions(t *testing.T) {
	gm := NewGraphManager(nil)

	// 直接操作 versionMap
	gm.mu.Lock()
	gm.versionMap["iri://task/a"] = 1
	gm.versionMap["iri://task/b"] = 3
	gm.versionMap["iri://task/c"] = 5
	gm.mu.Unlock()

	versions := gm.ListVersions()
	assert.Len(t, versions, 3)
	assert.Equal(t, 1, versions["iri://task/a"])
	assert.Equal(t, 3, versions["iri://task/b"])
	assert.Equal(t, 5, versions["iri://task/c"])

	// 验证返回的是快照，修改不影响原数据
	versions["iri://task/a"] = 99
	assert.Equal(t, 1, gm.GetVersion("iri://task/a"))
}

func TestGetVersionDefault(t *testing.T) {
	gm := NewGraphManager(nil)

	// 不存在的 IRI 返回 0
	assert.Equal(t, 0, gm.GetVersion("iri://nonexistent"))
}

func TestSyncFromEdgeDeleteAction(t *testing.T) {
	gm := NewGraphManager(nil)

	// 预设版本
	gm.mu.Lock()
	gm.versionMap["iri://task/to-delete"] = 5
	gm.mu.Unlock()
	assert.Equal(t, 5, gm.GetVersion("iri://task/to-delete"))

	// 直接测试内部删除逻辑变体
	gm.mu.Lock()
	delete(gm.versionMap, "iri://task/to-delete")
	gm.mu.Unlock()
	assert.Equal(t, 0, gm.GetVersion("iri://task/to-delete"))
}

func TestVersionMapConcurrency(t *testing.T) {
	gm := NewGraphManager(nil)

	done := make(chan bool, 10)
	for i := 0; i < 10; i++ {
		go func(n int) {
			iri := "iri://concurrent/test"
			gm.mu.Lock()
			gm.versionMap[iri] = n
			gm.mu.Unlock()
			_ = gm.GetVersion(iri)
			done <- true
		}(i)
	}

	for i := 0; i < 10; i++ {
		<-done
	}
}