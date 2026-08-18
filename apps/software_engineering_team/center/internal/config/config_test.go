package config

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

func TestSave(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "test_config.yaml")

	cfg := &Config{
		Server: ServerConfig{
			Host: "0.0.0.0",
			Port: 9090,
		},
		Temporal: TemporalConfig{
			Host:      "localhost",
			Port:      7233,
			TaskQueue: "test-queue",
		},
		GRPC: GRPCConfig{
			Host: "localhost",
			Port: 50052,
		},
		Meta: MetaConfig{
			Driver: "sqlite3",
			DSN:    "test.db",
		},
		LLM: LLMConfig{
			APIKey:   "sk-test",
			BaseURL:  "https://test.api.com/v1",
			Model:    "gpt-4",
			Provider: "openai",
		},
	}

	err := Save(cfg, path)
	require.NoError(t, err)

	data, err := os.ReadFile(path)
	require.NoError(t, err)

	content := string(data)
	assert.Contains(t, content, "9090")
	assert.Contains(t, content, "sk-test")
	assert.Contains(t, content, "gpt-4")
	assert.Contains(t, content, "test-queue")

	loadedCfg, err := Load(path)
	require.NoError(t, err)
	assert.Equal(t, cfg.Server.Port, loadedCfg.Server.Port)
	assert.Equal(t, cfg.LLM.APIKey, loadedCfg.LLM.APIKey)
	assert.Equal(t, cfg.LLM.Model, loadedCfg.LLM.Model)
	assert.Equal(t, cfg.LLM.Provider, loadedCfg.LLM.Provider)
}

func TestSaveOverwritesExistingFile(t *testing.T) {
	tmpDir := t.TempDir()
	path := filepath.Join(tmpDir, "existing.yaml")

	initialCfg := &Config{
		Server: ServerConfig{Host: "0.0.0.0", Port: 8080},
		LLM:    LLMConfig{Model: "gpt-3.5", Provider: "openai"},
	}
	err := Save(initialCfg, path)
	require.NoError(t, err)

	updatedCfg := &Config{
		Server: ServerConfig{Host: "0.0.0.0", Port: 8080},
		LLM:    LLMConfig{Model: "gpt-4", Provider: "azure"},
	}
	err = Save(updatedCfg, path)
	require.NoError(t, err)

	loadedCfg, err := Load(path)
	require.NoError(t, err)
	assert.Equal(t, "gpt-4", loadedCfg.LLM.Model)
	assert.Equal(t, "azure", loadedCfg.LLM.Provider)
}

func TestSaveInvalidPath(t *testing.T) {
	cfg := &Config{
		LLM: LLMConfig{Model: "gpt-4"},
	}
	err := Save(cfg, "/nonexistent/dir/config.yaml")
	assert.Error(t, err)
}