package agent

import (
	"context"
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func newTestAgent() *AgentInfo {
	return &AgentInfo{
		AgentID:      "agent-test-1",
		UserID:       "user-1",
		Capabilities: []string{"coding", "review"},
		Version:      "1.0.0",
	}
}

func TestRegisterAndGet(t *testing.T) {
	store := NewInMemoryAgentStore()
	mgr := NewAgentManager(store)

	agent := newTestAgent()
	err := mgr.Register(agent)
	require.NoError(t, err)

	assert.NotZero(t, agent.RegisteredAt)
	assert.NotZero(t, agent.LastHeartbeat)
	assert.Equal(t, AgentStatusOnline, agent.Status)

	saved, err := store.Get(agent.AgentID)
	require.NoError(t, err)
	assert.Equal(t, agent.AgentID, saved.AgentID)
	assert.Equal(t, agent.UserID, saved.UserID)
	assert.Equal(t, agent.Capabilities, saved.Capabilities)
	assert.Equal(t, AgentStatusOnline, saved.Status)
}

func TestRegisterValidation(t *testing.T) {
	store := NewInMemoryAgentStore()
	mgr := NewAgentManager(store)

	tests := []struct {
		name    string
		agent   *AgentInfo
		wantErr string
	}{
		{
			name:    "missing agent_id",
			agent:   &AgentInfo{UserID: "u1", Capabilities: []string{"coding"}},
			wantErr: "agent_id is required",
		},
		{
			name:    "missing user_id",
			agent:   &AgentInfo{AgentID: "a1", Capabilities: []string{"coding"}},
			wantErr: "user_id is required",
		},
		{
			name:    "missing capabilities",
			agent:   &AgentInfo{AgentID: "a1", UserID: "u1"},
			wantErr: "at least one capability is required",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := mgr.Register(tt.agent)
			assert.EqualError(t, err, tt.wantErr)
		})
	}
}

func TestHeartbeat(t *testing.T) {
	store := NewInMemoryAgentStore()
	mgr := NewAgentManager(store)

	agent := newTestAgent()
	err := mgr.Register(agent)
	require.NoError(t, err)

	originalBeat := agent.LastHeartbeat

	time.Sleep(10 * time.Millisecond)

	err = mgr.Heartbeat(agent.AgentID)
	require.NoError(t, err)

	saved, _ := store.Get(agent.AgentID)
	assert.True(t, saved.LastHeartbeat.After(originalBeat), "last heartbeat should be updated")
}

func TestHeartbeatReconnectsOfflineAgent(t *testing.T) {
	store := NewInMemoryAgentStore()
	mgr := NewAgentManager(store)

	agent := newTestAgent()
	agent.Status = AgentStatusOffline
	err := store.Save(agent)
	require.NoError(t, err)

	err = mgr.Heartbeat(agent.AgentID)
	require.NoError(t, err)

	saved, _ := store.Get(agent.AgentID)
	assert.Equal(t, AgentStatusOnline, saved.Status)
}

func TestHeartbeatNonExistentAgent(t *testing.T) {
	store := NewInMemoryAgentStore()
	mgr := NewAgentManager(store)

	err := mgr.Heartbeat("non-existent")
	assert.Error(t, err)
	assert.Contains(t, err.Error(), "agent not found")
}

func TestGetOnlineAgents(t *testing.T) {
	store := NewInMemoryAgentStore()
	mgr := NewAgentManager(store)

	agent1 := &AgentInfo{AgentID: "a1", UserID: "u1", Capabilities: []string{"coding"}}
	agent2 := &AgentInfo{AgentID: "a2", UserID: "u2", Capabilities: []string{"review"}, Status: AgentStatusOffline}
	agent3 := &AgentInfo{AgentID: "a3", UserID: "u3", Capabilities: []string{"deploy"}}

	_ = mgr.Register(agent1)
	_ = store.Save(agent2)
	_ = mgr.Register(agent3)

	online, err := mgr.GetOnlineAgents()
	require.NoError(t, err)

	assert.Len(t, online, 2)

	ids := make(map[string]bool)
	for _, a := range online {
		ids[a.AgentID] = true
	}
	assert.True(t, ids["a1"])
	assert.True(t, ids["a3"])
	assert.False(t, ids["a2"])
}

func TestMatchAgent(t *testing.T) {
	store := NewInMemoryAgentStore()
	mgr := NewAgentManager(store)

	_ = mgr.Register(&AgentInfo{AgentID: "a1", UserID: "u1", Capabilities: []string{"coding"}})
	_ = mgr.Register(&AgentInfo{AgentID: "a2", UserID: "u2", Capabilities: []string{"review", "design"}})
	_ = mgr.Register(&AgentInfo{AgentID: "a3", UserID: "u3", Capabilities: []string{"deploy"}, Status: AgentStatusOffline})

	matched := mgr.MatchAgent("coding")
	require.NotNil(t, matched)
	assert.Equal(t, "a1", matched.AgentID)

	matched = mgr.MatchAgent("review")
	require.NotNil(t, matched)
	assert.Equal(t, "a2", matched.AgentID)

	matched = mgr.MatchAgent("deploy")
	assert.Nil(t, matched, "offline agent should not be matched")

	matched = mgr.MatchAgent("nonexistent")
	assert.Nil(t, matched)
}

func TestMatchAgentReturnsFirstMatch(t *testing.T) {
	store := NewInMemoryAgentStore()
	mgr := NewAgentManager(store)

	_ = mgr.Register(&AgentInfo{AgentID: "a1", UserID: "u1", Capabilities: []string{"coding", "review"}})
	_ = mgr.Register(&AgentInfo{AgentID: "a2", UserID: "u2", Capabilities: []string{"coding"}})

	matched := mgr.MatchAgent("coding")
	require.NotNil(t, matched)
	assert.True(t, matched.AgentID == "a1" || matched.AgentID == "a2",
		"should match any agent with coding capability, got %s", matched.AgentID)
	assert.Equal(t, AgentStatusOnline, matched.Status)

	matched = mgr.MatchAgent("review")
	require.NotNil(t, matched)
	assert.Equal(t, "a1", matched.AgentID)
}

func TestTimeoutScan(t *testing.T) {
	store := NewInMemoryAgentStore()
	mgr := NewAgentManagerWithConfig(store, 50*time.Millisecond, 100*time.Millisecond)

	agent := newTestAgent()
	err := mgr.Register(agent)
	require.NoError(t, err)

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	mgr.StartTimeoutScanner(ctx)

	time.Sleep(200 * time.Millisecond)

	saved, err := store.Get(agent.AgentID)
	require.NoError(t, err)
	assert.Equal(t, AgentStatusOffline, saved.Status)
}

func TestTimeoutScanSkipsAlreadyOffline(t *testing.T) {
	store := NewInMemoryAgentStore()
	mgr := NewAgentManagerWithConfig(store, 50*time.Millisecond, 100*time.Millisecond)

	agent := &AgentInfo{AgentID: "a1", UserID: "u1", Capabilities: []string{"coding"}, Status: AgentStatusOffline}
	_ = store.Save(agent)

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	mgr.StartTimeoutScanner(ctx)

	time.Sleep(200 * time.Millisecond)

	saved, _ := store.Get("a1")
	assert.Equal(t, AgentStatusOffline, saved.Status, "should remain offline")
}

func TestStopTimeoutScanner(t *testing.T) {
	store := NewInMemoryAgentStore()
	mgr := NewAgentManager(store)

	ctx := context.Background()
	mgr.StartTimeoutScanner(ctx)

	assert.NotPanics(t, func() {
		mgr.StopTimeoutScanner()
	})

	assert.NotPanics(t, func() {
		mgr.StopTimeoutScanner()
	})
}

func TestStoreSaveGetListDelete(t *testing.T) {
	s := NewInMemoryAgentStore()

	agent := newTestAgent()
	err := s.Save(agent)
	require.NoError(t, err)

	got, err := s.Get(agent.AgentID)
	require.NoError(t, err)
	assert.Equal(t, agent.AgentID, got.AgentID)

	_, err = s.Get("not-found")
	assert.Error(t, err)

	list, err := s.List()
	require.NoError(t, err)
	assert.Len(t, list, 1)

	err = s.Delete(agent.AgentID)
	require.NoError(t, err)

	_, err = s.Get(agent.AgentID)
	assert.Error(t, err)

	list, err = s.List()
	require.NoError(t, err)
	assert.Len(t, list, 0)
}