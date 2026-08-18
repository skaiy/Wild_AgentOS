package config

import (
	"fmt"
	"strings"

	"github.com/joho/godotenv"
	"github.com/spf13/viper"
)

type Config struct {
	Server    ServerConfig    `yaml:"server" mapstructure:"server"`
	Temporal  TemporalConfig  `yaml:"temporal" mapstructure:"temporal"`
	GRPC      GRPCConfig      `yaml:"grpc" mapstructure:"grpc"`
	Meta      MetaConfig      `yaml:"meta_store" mapstructure:"meta_store"`
	LLM       LLMConfig       `yaml:"llm" mapstructure:"llm"`
}

type ServerConfig struct {
	Port int    `yaml:"port" mapstructure:"port"`
	Host string `yaml:"host" mapstructure:"host"`
}

type TemporalConfig struct {
	Host      string `yaml:"host" mapstructure:"host"`
	Port      int    `yaml:"port" mapstructure:"port"`
	TaskQueue string `yaml:"task_queue" mapstructure:"task_queue"`
}

type GRPCConfig struct {
	Host string `yaml:"host" mapstructure:"host"`
	Port int    `yaml:"port" mapstructure:"port"`
}

type MetaConfig struct {
	Driver string `yaml:"driver" mapstructure:"driver"`
	DSN    string `yaml:"dsn" mapstructure:"dsn"`
}

type LLMConfig struct {
	APIKey   string `yaml:"api_key" mapstructure:"api_key"`
	BaseURL  string `yaml:"base_url" mapstructure:"base_url"`
	Model    string `yaml:"model" mapstructure:"model"`
	Provider string `yaml:"provider" mapstructure:"provider"`
}

func Load(path string) (*Config, error) {
	_ = godotenv.Load()

	v := viper.New()
	v.SetConfigFile(path)
	v.SetConfigType("yaml")

	v.SetEnvPrefix("SE")
	v.SetEnvKeyReplacer(strings.NewReplacer(".", "_"))
	v.AutomaticEnv()

	if err := v.ReadInConfig(); err != nil {
		return nil, fmt.Errorf("read config: %w", err)
	}

	var cfg Config
	if err := v.Unmarshal(&cfg); err != nil {
		return nil, fmt.Errorf("unmarshal config: %w", err)
	}
	return &cfg, nil
}

func Save(cfg *Config, path string) error {
	v := viper.New()
	v.SetConfigFile(path)
	v.SetConfigType("yaml")

	v.Set("server", map[string]interface{}{
		"port": cfg.Server.Port,
		"host": cfg.Server.Host,
	})
	v.Set("temporal", map[string]interface{}{
		"host":       cfg.Temporal.Host,
		"port":       cfg.Temporal.Port,
		"task_queue": cfg.Temporal.TaskQueue,
	})
	v.Set("grpc", map[string]interface{}{
		"host": cfg.GRPC.Host,
		"port": cfg.GRPC.Port,
	})
	v.Set("meta_store", map[string]interface{}{
		"driver": cfg.Meta.Driver,
		"dsn":    cfg.Meta.DSN,
	})
	v.Set("llm", map[string]interface{}{
		"api_key":  cfg.LLM.APIKey,
		"base_url": cfg.LLM.BaseURL,
		"model":    cfg.LLM.Model,
		"provider": cfg.LLM.Provider,
	})

	return v.WriteConfigAs(path)
}