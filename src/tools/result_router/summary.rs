use serde_json::Value;

pub fn smart_truncate(result: &str, max_chars: usize) -> String {
    let trimmed = result.trim();

    if let Ok(val) = serde_json::from_str::<Value>(trimmed) {
        return smart_truncate_json_value(&val, max_chars);
    }

    smart_truncate_text(result, max_chars)
}

pub fn smart_truncate_json(json_str: &str, max_chars: usize) -> String {
    if let Ok(val) = serde_json::from_str::<Value>(json_str.trim()) {
        return smart_truncate_json_value(&val, max_chars);
    }
    smart_truncate_text(json_str, max_chars)
}

fn smart_truncate_json_value(val: &Value, max_chars: usize) -> String {
    match val {
        Value::Array(arr) => truncate_json_array(arr, max_chars),
        Value::Object(obj) => truncate_json_object(obj, max_chars),
        _ => {
            let s = val.to_string();
            if s.len() <= max_chars {
                s
            } else {
                smart_truncate_text(&s, max_chars)
            }
        }
    }
}

fn truncate_json_array(arr: &[Value], max_chars: usize) -> String {
    let total = arr.len();
    let mut kept = Vec::new();
    let mut current_size = 2;

    for item in arr {
        let item_str = item.to_string();
        let needed = if kept.is_empty() {
            item_str.len()
        } else {
            item_str.len() + 2
        };

        if current_size + needed + 50 > max_chars {
            break;
        }

        kept.push(item_str);
        current_size += needed;
    }

    let mut result = String::from("[");
    result.push_str(&kept.join(", "));
    result.push(']');

    if kept.len() < total {
        result.push_str(&format!(
            "\n\n[截断: 共 {} 个元素, 保留 {} 个]",
            total, kept.len()
        ));
    }

    result
}

fn truncate_json_object(obj: &serde_json::Map<String, Value>, max_chars: usize) -> String {
    let mut result_obj = serde_json::Map::new();
    let mut current_size = 2;
    let max_value_len = 200;

    for (key, value) in obj {
        let truncated_value = if let Value::String(s) = value {
            if s.len() > max_value_len {
                Value::String(format!("{}...[截断: 原始 {} 字符]", safe_slice(s, max_value_len), s.len()))
            } else {
                value.clone()
            }
        } else if let Value::Array(arr) = value {
            if arr.len() > 10 {
                let truncated: Vec<Value> = arr.iter().take(10).cloned().collect();
                Value::Array(truncated)
            } else {
                value.clone()
            }
        } else {
            value.clone()
        };

        let entry_size = key.len() + truncated_value.to_string().len() + 4;
        if current_size + entry_size > max_chars {
            break;
        }

        current_size += entry_size;
        result_obj.insert(key.clone(), truncated_value);
    }

    let mut result = serde_json::to_string_pretty(&Value::Object(result_obj)).unwrap_or_default();

    if result.len() > max_chars {
        result = smart_truncate_text(&result, max_chars);
    }

    result
}

fn safe_slice(s: &str, max_len: usize) -> &str {
    if max_len >= s.len() {
        return s;
    }
    let mut end = max_len;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

pub fn smart_truncate_text(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }

    let truncated = safe_slice(text, max_chars);

    if let Some(last_newline) = truncated.rfind('\n') {
        let result = truncated[..last_newline].to_string();
        let total_lines = text.lines().count();
        let kept_lines = result.lines().count();
        format!(
            "{}\n\n[截断: 共 {} 行, 保留 {} 行 | 原始 {} 字节]",
            result, total_lines, kept_lines, text.len()
        )
    } else {
        format!(
            "{}...\n\n[截断: 原始 {} 字节, 保留 {} 字节]",
            truncated, text.len(), truncated.len()
        )
    }
}

pub fn generate_text_summary(result_str: &str, tool_name: &str, preview_size: usize) -> String {
    let size = result_str.len();
    let lines: Vec<&str> = result_str.lines().collect();
    let line_count = lines.len();

    let preview = if result_str.len() > preview_size {
        safe_slice(result_str, preview_size).to_string()
    } else {
        result_str.to_string()
    };

    let mut summary = format!(
        "工具 [{}] 返回大文本结果 ({} 字节, {} 行):\n\n--- 预览 ---\n{}\n",
        tool_name, size, line_count, preview
    );

    if size > preview_size {
        let mut tail_start = size.saturating_sub(200);
        while tail_start < result_str.len() && !result_str.is_char_boundary(tail_start) {
            tail_start += 1;
        }
        let tail = safe_slice(&result_str[tail_start..], 200);
        summary.push_str(&format!("\n--- 末尾预览 ---\n{}\n", tail));
        summary.push_str(&format!("\n[完整结果已存储, 使用 read_full_result 工具按需读取]"));
    }

    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_json_array() {
        let items: Vec<Value> = (0..50)
            .map(|i| serde_json::json!({"id": i, "name": format!("item_{}", i)}))
            .collect();
        let json = serde_json::to_string(&items).unwrap();

        let result = smart_truncate(&json, 500);
        assert!(result.len() < 600);
        assert!(result.contains("截断"));
        assert!(result.contains("50 个元素"));
    }

    #[test]
    fn test_truncate_json_object() {
        let mut obj = serde_json::Map::new();
        for i in 0..20 {
            obj.insert(format!("key_{}", i), Value::String("x".repeat(500)));
        }
        let json = serde_json::to_string(&Value::Object(obj)).unwrap();

        let result = smart_truncate(&json, 1000);
        assert!(result.len() < 1100);
    }

    #[test]
    fn test_truncate_invalid_json_fallback() {
        let text = "not json\n".repeat(500);
        let result = smart_truncate(&text, 1000);
        assert!(result.contains("截断"));
        assert!(result.contains("行"));
    }

    #[test]
    fn test_truncate_text_at_newline() {
        let text = "line1\nline2\nline3\nline4\nline5";
        let result = smart_truncate_text(text, 15);
        assert!(result.contains("line1"));
        assert!(result.contains("截断"));
    }

    #[test]
    fn test_truncate_utf8_boundary() {
        let text = "你好世界\n".repeat(500);
        let result = smart_truncate_text(&text, 1000);
        assert!(result.contains("截断"));
        assert!(result.len() < 1100);
        assert!(result.is_char_boundary(result.len()));
    }

    #[test]
    fn test_generate_summary_utf8() {
        let text = "这是中文内容\n".repeat(1000);
        let summary = generate_text_summary(&text, "test_tool", 200);
        assert!(summary.contains("test_tool"));
        assert!(summary.contains("read_full_result"));
        assert!(summary.is_char_boundary(summary.len()));
    }

    #[test]
    fn test_small_text_passthrough() {
        let text = "small text";
        let result = smart_truncate_text(text, 100);
        assert_eq!(result, text);
    }

    #[test]
    fn test_generate_summary() {
        let text = "line\n".repeat(1000);
        let summary = generate_text_summary(&text, "test_tool", 200);
        assert!(summary.contains("test_tool"));
        assert!(summary.contains("1000 行"));
        assert!(summary.contains("read_full_result"));
    }
}
