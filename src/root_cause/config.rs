/// Configuration for the RootCauseEngine.
///
/// Architecture Layer: L1 — Enforcement (RootCauseEngine)
///
/// Default impl is in types.rs alongside the struct definition.

use super::types::RootCauseConfig;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RootCauseConfig::default();
        assert_eq!(config.max_trace_depth, 5);
        assert!((config.min_confidence - 0.7).abs() < 0.01);
        assert!(config.enable_auto_trace);
        assert_eq!(config.trace_timeout_ms, 30_000);
    }
}
