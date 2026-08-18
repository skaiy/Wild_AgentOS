package grpc

import (
	"testing"
	"time"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestNewClient_NilTarget(t *testing.T) {
	client, err := NewClient("")
	require.Error(t, err)
	assert.Nil(t, client)
}

func TestClient_Close(t *testing.T) {
	client, err := NewClient("127.0.0.1:9999")
	require.NoError(t, err)
	require.NotNil(t, client)

	err = client.Close()
	assert.NoError(t, err)
}

func TestNewClient_Success(t *testing.T) {
	client, err := NewClient("localhost:50051")
	require.NoError(t, err)
	require.NotNil(t, client)
	assert.NotNil(t, client.conn)
	assert.NotNil(t, client.kernel)
	assert.Equal(t, "localhost:50051", client.target)

	err = client.Close()
	assert.NoError(t, err)
}

func TestClient_DoubleClose(t *testing.T) {
	client, err := NewClient("localhost:50051")
	require.NoError(t, err)

	err = client.Close()
	assert.NoError(t, err)

	err = client.Close()
	assert.NoError(t, err)
}

func TestClient_ExecuteStageTimeout(t *testing.T) {
	client, err := NewClient("localhost:50051")
	require.NoError(t, err)
	defer client.Close()

	assert.Equal(t, 30*time.Minute, executeStageTimeout)
}

func TestClient_ReconnectInterval(t *testing.T) {
	assert.Equal(t, 30*time.Second, reconnectInterval)
}