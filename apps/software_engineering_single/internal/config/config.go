package config

import (
	"fmt"

	"github.com/spf13/viper"
)

type Config struct {
	Server    ServerConfig    `mapstructure:"server"`
	Temporal  TemporalConfig  `mapstructure:"temporal"`
	GRPC      GRPCConfig      `mapstructure:"grpc"`
	MetaStore MetaStoreConfig `mapstructure:"meta_store"`
	LLM       LLMConfig       `mapstructure:"llm"`
	Sandbox   SandboxConfig   `mapstructure:"sandbox"`
}

type ServerConfig struct {
	Port int `mapstructure:"port"`
}

type TemporalConfig struct {
	HostPort  string `mapstructure:"host_port"`
	TaskQueue string `mapstructure:"task_queue"`
}

type GRPCConfig struct {
	Target string `mapstructure:"target"`
}

type MetaStoreConfig struct {
	Driver string `mapstructure:"driver"`
	DSN    string `mapstructure:"dsn"`
}

type LLMConfig struct {
	ApiKey  string `mapstructure:"api_key"`
	BaseURL string `mapstructure:"base_url"`
	Model   string `mapstructure:"model"`
}

type SandboxConfig struct {
	Enabled              bool    `mapstructure:"enabled"`
	BaseDir              string  `mapstructure:"base_dir"`
	BasePort             int     `mapstructure:"base_port"`
	MaxPort              int     `mapstructure:"max_port"`
	DefaultStack         string  `mapstructure:"default_stack"`
	DefaultCPULimit      float64 `mapstructure:"default_cpu_limit"`
	DefaultMemoryLimitMB int     `mapstructure:"default_memory_limit_mb"`
	CodeServerTimeout    int     `mapstructure:"code_server_timeout"`
	AutoTerminateMinutes int     `mapstructure:"auto_terminate_minutes"`
}

func Load(path string) (*Config, error) {
	v := viper.New()

	v.SetConfigFile(path)
	v.SetConfigType("yaml")

	v.AutomaticEnv()
	v.SetEnvPrefix("SE")

	if err := v.ReadInConfig(); err != nil {
		return nil, fmt.Errorf("read config: %w", err)
	}

	var cfg Config
	if err := v.Unmarshal(&cfg); err != nil {
		return nil, fmt.Errorf("unmarshal config: %w", err)
	}

	return &cfg, nil
}

func DefaultConfig() *Config {
	return &Config{
		Server: ServerConfig{
			Port: 8080,
		},
		Temporal: TemporalConfig{
			HostPort:  "localhost:7233",
			TaskQueue: "se-pipeline",
		},
		GRPC: GRPCConfig{
			Target: "localhost:50051",
		},
		MetaStore: MetaStoreConfig{
			Driver: "sqlite",
			DSN:    "se_pipeline.db",
		},
		LLM: LLMConfig{
			BaseURL: "http://localhost:3000",
			Model:   "gpt-4-turbo-preview",
		},
		Sandbox: SandboxConfig{
			Enabled:              true,
			BaseDir:              "/var/lib/agent-os/workspaces",
			BasePort:             9000,
			MaxPort:              9100,
			DefaultStack:         "base",
			DefaultCPULimit:      2.0,
			DefaultMemoryLimitMB: 2048,
			CodeServerTimeout:    30,
			AutoTerminateMinutes: 120,
		},
	}
}