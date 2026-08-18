package api

import (
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"sync"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/gorilla/websocket"
)

var upgrader = websocket.Upgrader{
	CheckOrigin: func(r *http.Request) bool {
		return true
	},
}

type Message struct {
	Type    string          `json:"type"`
	Payload json.RawMessage `json:"payload"`
}

type Hub struct {
	mu      sync.RWMutex
	clients map[string][]*websocket.Conn
}

func NewHub() *Hub {
	return &Hub{
		clients: make(map[string][]*websocket.Conn),
	}
}

func (h *Hub) Broadcast(projectID string, msg Message) {
	h.mu.RLock()
	conns := h.clients[projectID]
	h.mu.RUnlock()

	data, err := json.Marshal(msg)
	if err != nil {
		log.Printf("marshal message: %v", err)
		return
	}

	for _, conn := range conns {
		if err := conn.WriteMessage(websocket.TextMessage, data); err != nil {
			log.Printf("write message: %v", err)
			h.removeConn(projectID, conn)
		}
	}
}

func (h *Hub) removeConn(projectID string, conn *websocket.Conn) {
	h.mu.Lock()
	defer h.mu.Unlock()

	conns := h.clients[projectID]
	for i, c := range conns {
		if c == conn {
			h.clients[projectID] = append(conns[:i], conns[i+1:]...)
			conn.Close()
			return
		}
	}
}

func (svc *Service) HandleWebSocket(c *gin.Context) {
	projectID := c.Query("project_id")
	if projectID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "project_id is required"})
		return
	}

	conn, err := upgrader.Upgrade(c.Writer, c.Request, nil)
	if err != nil {
		log.Printf("upgrade websocket: %v", err)
		return
	}

	svc.Hub.mu.Lock()
	svc.Hub.clients[projectID] = append(svc.Hub.clients[projectID], conn)
	svc.Hub.mu.Unlock()

	defer func() {
		svc.Hub.removeConn(projectID, conn)
	}()

	conn.SetReadDeadline(time.Now().Add(60 * time.Second))
	conn.SetPongHandler(func(string) error {
		conn.SetReadDeadline(time.Now().Add(60 * time.Second))
		return nil
	})

	for {
		_, _, err := conn.ReadMessage()
		if err != nil {
			break
		}
	}
}

func (svc *Service) NotifyStageUpdate(projectID, stageID, status string) {
	payload, _ := json.Marshal(map[string]string{
		"stage_id": stageID,
		"status":   status,
	})
	svc.Hub.Broadcast(projectID, Message{
		Type:    fmt.Sprintf("stage_update"),
		Payload: payload,
	})
}