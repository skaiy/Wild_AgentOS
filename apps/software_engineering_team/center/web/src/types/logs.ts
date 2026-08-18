export type LogLevel = 'DEBUG' | 'INFO' | 'WARN' | 'ERROR';

export interface LogEntry {
  id: string;
  timestamp: string;
  level: LogLevel;
  message: string;
  source: string;
  metadata?: Record<string, unknown>;
}

export interface LogFilter {
  level?: LogLevel;
  since?: string;
  until?: string;
  keyword?: string;
  source?: string;
}

export interface LogExportOptions {
  format: 'json' | 'txt';
  filter: LogFilter;
}