package grpc

import "testing"

func TestNewClient_InvalidTarget(t *testing.T) {
    _, err := NewClient("invalid:target")
    if err == nil {
        t.Error("期望连接失败但成功了")
    }
}