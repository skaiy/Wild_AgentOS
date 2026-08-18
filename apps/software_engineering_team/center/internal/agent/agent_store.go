package agent

import (
	"fmt"
	"sync"
)

type InMemoryAgentStore struct {
	mu     sync.RWMutex
	agents map[string]*AgentInfo
}

func NewInMemoryAgentStore() *InMemoryAgentStore {
	return &InMemoryAgentStore{
		agents: make(map[string]*AgentInfo),
	}
}

func (s *InMemoryAgentStore) Save(agent *AgentInfo) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.agents[agent.AgentID] = agent
	return nil
}

func (s *InMemoryAgentStore) Get(agentID string) (*AgentInfo, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	agent, ok := s.agents[agentID]
	if !ok {
		return nil, fmt.Errorf("agent %s not found", agentID)
	}
	return agent, nil
}

func (s *InMemoryAgentStore) List() ([]*AgentInfo, error) {
	s.mu.RLock()
	defer s.mu.RUnlock()
	result := make([]*AgentInfo, 0, len(s.agents))
	for _, a := range s.agents {
		result = append(result, a)
	}
	return result, nil
}

func (s *InMemoryAgentStore) Delete(agentID string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	delete(s.agents, agentID)
	return nil
}