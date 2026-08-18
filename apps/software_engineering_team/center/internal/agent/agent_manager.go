package agent

import (
	"context"
	"fmt"
	"log"
	"sync"
	"time"
)

type AgentInfo struct {
	AgentID       string    `json:"agent_id"`
	UserID        string    `json:"user_id"`
	Status        string    `json:"status"`
	Capabilities  []string  `json:"capabilities"`
	LastHeartbeat time.Time `json:"last_heartbeat"`
	RegisteredAt  time.Time `json:"registered_at"`
	Version       string    `json:"version"`
}

const (
	AgentStatusOnline  = "online"
	AgentStatusOffline = "offline"

	defaultTimeoutDuration = 10 * time.Second
	defaultScanInterval    = 30 * time.Second
)

type AgentStore interface {
	Save(agent *AgentInfo) error
	Get(agentID string) (*AgentInfo, error)
	List() ([]*AgentInfo, error)
	Delete(agentID string) error
}

type AgentManager struct {
	store           AgentStore
	mu              sync.RWMutex
	timeoutDuration time.Duration
	scanInterval    time.Duration
	cancelFunc      context.CancelFunc
}

func NewAgentManager(store AgentStore) *AgentManager {
	return &AgentManager{
		store:           store,
		timeoutDuration: defaultTimeoutDuration,
		scanInterval:    defaultScanInterval,
	}
}

func NewAgentManagerWithConfig(store AgentStore, timeoutDuration, scanInterval time.Duration) *AgentManager {
	return &AgentManager{
		store:           store,
		timeoutDuration: timeoutDuration,
		scanInterval:    scanInterval,
	}
}

func (m *AgentManager) Register(info *AgentInfo) error {
	if info.AgentID == "" {
		return fmt.Errorf("agent_id is required")
	}
	if info.UserID == "" {
		return fmt.Errorf("user_id is required")
	}

	now := time.Now()
	info.RegisteredAt = now
	info.LastHeartbeat = now
	if info.Status == "" {
		info.Status = AgentStatusOnline
	}

	return m.store.Save(info)
}

func (m *AgentManager) Heartbeat(agentID string) error {
	agent, err := m.store.Get(agentID)
	if err != nil {
		return fmt.Errorf("agent not found: %w", err)
	}

	now := time.Now()
	agent.LastHeartbeat = now
	if agent.Status == AgentStatusOffline {
		agent.Status = AgentStatusOnline
		log.Printf("[AgentManager] agent %s reconnected, status -> online", agentID)
	}

	return m.store.Save(agent)
}

func (m *AgentManager) GetOnlineAgents() ([]*AgentInfo, error) {
	all, err := m.store.List()
	if err != nil {
		return nil, err
	}

	var online []*AgentInfo
	for _, a := range all {
		if a.Status == AgentStatusOnline {
			online = append(online, a)
		}
	}
	return online, nil
}

func (m *AgentManager) MatchAgent(capability string) *AgentInfo {
	all, err := m.store.List()
	if err != nil {
		return nil
	}

	for _, a := range all {
		if a.Status != AgentStatusOnline {
			continue
		}
		for _, cap := range a.Capabilities {
			if cap == capability {
				return a
			}
		}
	}
	return nil
}

func (m *AgentManager) StartTimeoutScanner(ctx context.Context) {
	m.mu.Lock()
	if m.cancelFunc != nil {
		m.cancelFunc()
	}
	ctx, cancel := context.WithCancel(ctx)
	m.cancelFunc = cancel
	m.mu.Unlock()

	go m.timeoutScanLoop(ctx)
}

func (m *AgentManager) StopTimeoutScanner() {
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.cancelFunc != nil {
		m.cancelFunc()
		m.cancelFunc = nil
	}
}

func (m *AgentManager) timeoutScanLoop(ctx context.Context) {
	ticker := time.NewTicker(m.scanInterval)
	defer ticker.Stop()

	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			m.scanAndMarkOffline()
		}
	}
}

func (m *AgentManager) scanAndMarkOffline() {
	all, err := m.store.List()
	if err != nil {
		log.Printf("[AgentManager] failed to list agents during timeout scan: %v", err)
		return
	}

	now := time.Now()
	for _, a := range all {
		if a.Status == AgentStatusOffline {
			continue
		}
		if now.Sub(a.LastHeartbeat) > m.timeoutDuration {
			a.Status = AgentStatusOffline
			if err := m.store.Save(a); err != nil {
				log.Printf("[AgentManager] failed to mark agent %s offline: %v", a.AgentID, err)
			} else {
				log.Printf("[AgentManager] agent %s timed out, marked offline", a.AgentID)
			}
		}
	}
}