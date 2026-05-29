package grpc

import (
	"context"
	"fmt"
	"io"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/keepalive"
	pb "github.com/agent-os/se-app/proto/seapp"
)

type Client struct {
	conn   *grpc.ClientConn
	kernel pb.SeKernelServiceClient
}

func NewClient(target string) (*Client, error) {
	keepaliveParams := keepalive.ClientParameters{
		Time:                30 * time.Second,
		Timeout:             10 * time.Second,
		PermitWithoutStream: true,
	}

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()
	conn, err := grpc.DialContext(ctx, target,
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithKeepaliveParams(keepaliveParams),
		grpc.WithBlock(),
	)
	if err != nil {
		return nil, fmt.Errorf("连接 gRPC 服务失败: %w", err)
	}
	return &Client{
		conn:   conn,
		kernel: pb.NewSeKernelServiceClient(conn),
	}, nil
}

func (c *Client) Close() error { return c.conn.Close() }

func (c *Client) ExecuteStage(ctx context.Context, req *pb.ExecuteStageRequest) (*pb.ExecuteStageResponse, error) {
	ctx, cancel := context.WithTimeout(ctx, 60*time.Second)
	defer cancel()
	return c.kernel.ExecuteStage(ctx, req)
}

func (c *Client) ValidateContract(ctx context.Context, req *pb.ValidateContractRequest) (*pb.ValidateContractResponse, error) {
	ctx, cancel := context.WithTimeout(ctx, 30*time.Second)
	defer cancel()
	return c.kernel.ValidateContract(ctx, req)
}

func (c *Client) FlattenToFrontend(ctx context.Context, req *pb.FlattenRequest) (*pb.FlattenResponse, error) {
	ctx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	return c.kernel.FlattenToFrontend(ctx, req)
}

func (c *Client) SubmitHumanApproval(ctx context.Context, req *pb.SubmitApprovalRequest) (*pb.SubmitApprovalResponse, error) {
	ctx, cancel := context.WithTimeout(ctx, 10*time.Second)
	defer cancel()
	return c.kernel.SubmitHumanApproval(ctx, req)
}

type ChatStreamChunk struct {
	Content string
	Done    bool
	Status  string
}

func (c *Client) ChatStream(ctx context.Context, req *pb.ChatStreamRequest) (<-chan ChatStreamChunk, error) {
	stream, err := c.kernel.ChatStream(ctx, req)
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
