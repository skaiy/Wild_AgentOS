use agent_os::gateway::UnifiedGateway;
use agent_os::config::GatewaySettings;
use std::collections::HashMap;

fn test_gateway_settings() -> GatewaySettings {
    GatewaySettings {
        base_url: "http://localhost:3000".to_string(),
        api_key: "sk-test-key".to_string(),
        default_model: "gpt-3.5-turbo".to_string(),
        timeout_seconds: 30,
        max_retries: 3,
        model_mapping: HashMap::from([
            ("planning".to_string(), "gpt-4-turbo-preview".to_string()),
            ("execution".to_string(), "gpt-4-turbo-preview".to_string()),
            ("analysis".to_string(), "gpt-3.5-turbo".to_string()),
            ("default".to_string(), "gpt-3.5-turbo".to_string()),
        ]),
    }
}

#[test]
fn test_gateway_model_mapping() {
    let gateway = UnifiedGateway::new(&test_gateway_settings()).unwrap();
    assert_eq!(gateway.get_model("planning"), "gpt-4-turbo-preview");
    assert_eq!(gateway.get_model("execution"), "gpt-4-turbo-preview");
    assert_eq!(gateway.get_model("analysis"), "gpt-3.5-turbo");
    assert_eq!(gateway.get_model("unknown"), "gpt-3.5-turbo");
    assert_eq!(gateway.get_model("default"), "gpt-3.5-turbo");
}

#[test]
fn test_gateway_default_model() {
    let gateway = UnifiedGateway::new(&test_gateway_settings()).unwrap();
    assert_eq!(gateway.default_model(), "gpt-3.5-turbo");
}

#[test]
fn test_gateway_set_model_mapping() {
    let mut gateway = UnifiedGateway::new(&test_gateway_settings()).unwrap();
    gateway.set_model_mapping("custom_task".to_string(), "gpt-4".to_string());
    assert_eq!(gateway.get_model("custom_task"), "gpt-4");
}

#[test]
fn test_gateway_construction() {
    let gateway = UnifiedGateway::new(&test_gateway_settings());
    assert!(gateway.is_ok());
}
