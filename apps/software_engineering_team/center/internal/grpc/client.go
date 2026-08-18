package grpc

import (
	"context"
	"fmt"
	"io"
	"log"
	"sync"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/connectivity"
	"google.golang.org/grpc/credentials/insecure"
	pb "github.com/agent-os/se-center/proto/seapp"
)

const (
	reconnectInterval = 30 * time.Second
	executeStageTimeout = 30 * time.Minute
	validateContractTimeout = 30 * time.Second
	flattenTimeout = 10 * time.Second
	submitApprovalTimeout = 10 * time.Second
)

type Client struct {
	mu     sync.Mutex
	target string
	conn   *grpc.ClientConn
	kernel pb.SeKernelServiceClient
	stopCh chan struct{}
	closed bool
}

func NewClient(target string) (*Client, error) {
	c := &Client{
		target: target,
		stopCh: make(chan struct{}),
	}

	conn, err := c.dial()
	if err != nil {
		return nil, fmt.Errorf("连接 gRPC 服务失败: %w", err)
	}
	c.conn = conn
	c.kernel = pb.NewSeKernelServiceClient(conn)

	go c.reconnectLoop()

	return c, nil
}

func (c *Client) dial() (*grpc.ClientConn, error) {
	ctx := context.Background()
	conn, err := grpc.DialContext(ctx, c.target,
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithDefaultCallOptions(grpc.MaxCallRecvMsgSize(100*1024*1024)),
	)
	if err != nil {
		return nil, err
	}
	return conn, nil
}

func (c *Client) reconnectLoop() {
	ticker := time.NewTicker(reconnectInterval)
	defer ticker.Stop()

	for {
		select {
		case <-c.stopCh:
			return
		case <-ticker.C:
			c.mu.Lock()
			state := c.conn.GetState()
			if state == connectivity.TransientFailure || state == connectivity.Shutdown {
				log.Printf("[gRPC] 连接状态 %v, 尝试重连 %s ...", state, c.target)
				newConn, err := c.dial()
				if err != nil {
					log.Printf("[gRPC] 重连失败: %v", err)
					c.mu.Unlock()
					continue
				}
				oldConn := c.conn
				c.conn = newConn
				c.kernel = pb.NewSeKernelServiceClient(newConn)
				c.mu.Unlock()
				oldConn.Close()
				log.Printf("[gRPC] 重连成功 %s", c.target)
			} else {
				c.mu.Unlock()
			}
		}
	}
}

func (c *Client) getClient() pb.SeKernelServiceClient {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.kernel
}

func (c *Client) Close() error {
	c.mu.Lock()
	if c.closed {
		c.mu.Unlock()
		return nil
	}
	c.closed = true
	close(c.stopCh)
	conn := c.conn
	c.mu.Unlock()
	if conn != nil {
		return conn.Close()
	}
	return nil
}

func (c *Client) ExecuteStage(ctx context.Context, req *pb.ExecuteStageRequest) (*pb.ExecuteStageResponse, error) {
	ctx, cancel := context.WithTimeout(ctx, executeStageTimeout)
	defer cancel()
	return c.getClient().ExecuteStage(ctx, req)
}

func (c *Client) ValidateContract(ctx context.Context, req *pb.ValidateContractRequest) (*pb.ValidateContractResponse, error) {
	ctx, cancel := context.WithTimeout(ctx, validateContractTimeout)
	defer cancel()
	return c.getClient().ValidateContract(ctx, req)
}

func (c *Client) FlattenToFrontend(ctx context.Context, req *pb.FlattenRequest) (*pb.FlattenResponse, error) {
	ctx, cancel := context.WithTimeout(ctx, flattenTimeout)
	defer cancel()
	return c.getClient().FlattenToFrontend(ctx, req)
}

func (c *Client) SubmitHumanApproval(ctx context.Context, req *pb.SubmitApprovalRequest) (*pb.SubmitApprovalResponse, error) {
	ctx, cancel := context.WithTimeout(ctx, submitApprovalTimeout)
	defer cancel()
	return c.getClient().SubmitHumanApproval(ctx, req)
}

type ChatStreamChunk struct {
	Content string
	Done    bool
	Status  string
}

func (c *Client) ChatStream(ctx context.Context, req *pb.ChatStreamRequest) (<-chan ChatStreamChunk, error) {
	stream, err := c.getClient().ChatStream(ctx, req)
	if err != nil {
		return nil, fmt.Errorf("ChatStream gRPC 调用失败: %w", err)
	}

	ch := make(chan ChatStreamChunk, 64)

	go func() {
		defer close(ch)
		for {
			chunk, err := stream.Recv()
			if err == io.EOF {
				ch <- ChatStreamChunk{Done: true, Status: "completed"}
				return
			}
			if err != nil {
				ch <- ChatStreamChunk{
					Content: fmt.Sprintf("流式传输错误: %v", err),
					Done:    true,
					Status:  "error",
				}
				return
			}
			ch <- ChatStreamChunk{
				Content: chunk.GetContent(),
				Done:    chunk.GetDone(),
				Status:  chunk.GetStatus(),
			}
			if chunk.GetDone() {
				return
			}
		}
	}()

	return ch, nil
}