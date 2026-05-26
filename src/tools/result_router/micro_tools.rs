use serde_json::json;

use super::{MicroToolSchema, MicroToolType, SchemaAnalysis};

pub struct MicroToolGenerator;

impl MicroToolGenerator {
    pub fn new() -> Self {
        Self
    }

    pub fn generate_from_schema(
        analysis: &SchemaAnalysis,
        call_id: &str,
        max_tools: usize,
    ) -> Vec<MicroToolSchema> {
        let graph_name = format!("graph:tool-result:{}", call_id);
        let mut tools = Vec::new();

        for (type_name, count) in &analysis.entity_types {
            if tools.len() >= max_tools {
                break;
            }
            let short_name = type_name.split('/').last().unwrap_or(type_name);
            let tool_name = format!("query_{}", short_name.to_lowercase());

            tools.push(MicroToolSchema {
                name: tool_name,
                description: format!(
                    "查询 {} 类型的实体 (共 {} 个)。支持按属性过滤。",
                    short_name, count
                ),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "filter_property": {
                            "type": "string",
                            "description": "要过滤的属性名"
                        },
                        "filter_value": {
                            "type": "string",
                            "description": "过滤值"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "返回数量限制",
                            "default": 10
                        }
                    }
                }),
                tool_type: MicroToolType::EntityTypeQuery {
                    entity_type: type_name.clone(),
                    graph_name: graph_name.clone(),
                },
            });
        }

        if tools.len() < max_tools {
            tools.push(MicroToolSchema {
                name: "get_entity_details".to_string(),
                description: "获取指定实体的全部属性和关系".to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "entity_id": {
                            "type": "string",
                            "description": "实体 ID"
                        }
                    },
                    "required": ["entity_id"]
                }),
                tool_type: MicroToolType::EntityDetails {
                    graph_name: graph_name.clone(),
                },
            });
        }

        if tools.len() < max_tools && !analysis.relation_types.is_empty() {
            tools.push(MicroToolSchema {
                name: "expand_relation".to_string(),
                description: format!(
                    "沿关系边遍历。可用关系: {}",
                    analysis.relation_types.join(", ")
                ),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "entity_id": {
                            "type": "string",
                            "description": "起始实体 ID"
                        },
                        "relation": {
                            "type": "string",
                            "description": "关系类型"
                        },
                        "depth": {
                            "type": "integer",
                            "description": "遍历深度",
                            "default": 1
                        }
                    },
                    "required": ["entity_id"]
                }),
                tool_type: MicroToolType::RelationTraversal {
                    graph_name: graph_name.clone(),
                },
            });
        }

        tools.truncate(max_tools);
        tools
    }

    pub fn generate_read_full_tool(
        call_id: &str,
        storage_key: &str,
        preview_size: usize,
    ) -> MicroToolSchema {
        MicroToolSchema {
            name: "read_full_result".to_string(),
            description: format!(
                "读取工具完整结果 (预览已展示前 {} 字符)",
                preview_size
            ),
            parameters: json!({
                "type": "object",
                "properties": {
                    "offset": {
                        "type": "integer",
                        "description": "起始偏移 (字符)",
                        "default": 0
                    },
                    "limit": {
                        "type": "integer",
                        "description": "读取长度 (字符)",
                        "default": 4000
                    }
                }
            }),
            tool_type: MicroToolType::FullTextRead {
                storage_key: storage_key.to_string(),
            },
        }
    }

    pub fn format_tool_injection_message(summary: &str, tools: &[MicroToolSchema]) -> String {
        let mut msg = format!("{}\n\n可用查询工具:\n", summary);
        for tool in tools {
            msg.push_str(&format!(
                "- **{}**: {}\n",
                tool.name, tool.description
            ));
        }
        msg.push_str("\n你可以调用这些工具来查询完整数据。");
        msg
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_analysis() -> SchemaAnalysis {
        SchemaAnalysis {
            entity_types: vec![
                ("Person".to_string(), 10),
                ("Organization".to_string(), 3),
            ],
            relation_types: vec!["works_for".to_string()],
            property_names: vec!["name".to_string(), "age".to_string()],
            total_entities: 13,
            total_relations: 5,
        }
    }

    #[test]
    fn test_generate_from_schema() {
        let analysis = make_analysis();
        let tools = MicroToolGenerator::generate_from_schema(&analysis, "call_1", 5);

        assert!(tools.len() >= 2);
        assert!(tools.iter().any(|t| t.name == "query_person"));
        assert!(tools.iter().any(|t| t.name == "query_organization"));
        assert!(tools.iter().any(|t| t.name == "get_entity_details"));
        assert!(tools.iter().any(|t| t.name == "expand_relation"));
    }

    #[test]
    fn test_generate_respects_max_tools() {
        let analysis = make_analysis();
        let tools = MicroToolGenerator::generate_from_schema(&analysis, "call_2", 2);

        assert!(tools.len() <= 2);
    }

    #[test]
    fn test_generate_read_full_tool() {
        let tool = MicroToolGenerator::generate_read_full_tool("call_3", "storage_key_1", 2000);

        assert_eq!(tool.name, "read_full_result");
        assert!(tool.description.contains("2000"));
        assert!(matches!(tool.tool_type, MicroToolType::FullTextRead { .. }));
    }

    #[test]
    fn test_format_injection_message() {
        let analysis = make_analysis();
        let tools = MicroToolGenerator::generate_from_schema(&analysis, "call_4", 5);
        let msg = MicroToolGenerator::format_tool_injection_message("测试摘要", &tools);

        assert!(msg.contains("测试摘要"));
        assert!(msg.contains("query_person"));
        assert!(msg.contains("查询工具"));
    }
}
