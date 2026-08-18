package config

import (
"fmt"
"os"
"path/filepath"
"testing"
)

func TestDebugSave(t *testing.T) {
tmpDir := t.TempDir()
path := filepath.Join(tmpDir, "test.yaml")

cfg := &Config{}
cfg.Server.Port = 9090
cfg.Server.Host = "0.0.0.0"
cfg.LLM.APIKey = "sk-test"
cfg.LLM.BaseURL = "https://test.api.com/v1"
cfg.LLM.Model = "gpt-4"
cfg.LLM.Provider = "openai"

err := Save(cfg, path)
fmt.Printf("Save error: %v\n", err)

data, _ := os.ReadFile(path)
fmt.Printf("Written YAML:\n%s\n", string(data))

loaded, err := Load(path)
fmt.Printf("Load error: %v\n", err)
if loaded != nil {
	t.Logf("Loaded LLM APIKey: '%s'\n", loaded.LLM.APIKey)
	t.Logf("Loaded LLM BaseURL: '%s'\n", loaded.LLM.BaseURL)
	t.Logf("Loaded LLM Model: '%s'\n", loaded.LLM.Model)
	t.Logf("Loaded LLM Provider: '%s'\n", loaded.LLM.Provider)
}

seKey := os.Getenv("SE_LLM_API_KEY")
fmt.Printf("SE_LLM_API_KEY env: '%s'\n", seKey)
}
