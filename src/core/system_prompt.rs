use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SystemPromptRegion {
    RoleDefinition,
    EmphasizedConstraints,
    OutputFormat,
    Tools,
    ExtractionPrompt,
}

impl SystemPromptRegion {
    pub fn order(&self) -> usize {
        match self {
            Self::RoleDefinition => 1,
            Self::EmphasizedConstraints => 2,
            Self::OutputFormat => 3,
            Self::Tools => 4,
            Self::ExtractionPrompt => 5,
        }
    }

    pub fn header(&self) -> &'static str {
        match self {
            Self::RoleDefinition => "# 角色定义",
            Self::EmphasizedConstraints => "# 重要约束（必须遵守）",
            Self::OutputFormat => "# 输出格式",
            Self::Tools => "# 可用工具",
            Self::ExtractionPrompt => "# 强调内容提取",
        }
    }
}

pub struct ToolRegionContent {
    pub builtin_tools: String,
    pub dynamic_tools: String,
}

impl ToolRegionContent {
    pub fn new() -> Self {
        Self {
            builtin_tools: String::new(),
            dynamic_tools: String::new(),
        }
    }

    pub fn with_builtin(mut self, tools: &str) -> Self {
        self.builtin_tools = tools.to_string();
        self
    }

    pub fn with_dynamic(mut self, tools: &str) -> Self {
        self.dynamic_tools = tools.to_string();
        self
    }

    pub fn build(&self) -> String {
        let mut parts = Vec::new();
        
        if !self.builtin_tools.is_empty() {
            parts.push(format!("## 内置工具（固定）\n{}", self.builtin_tools));
        }
        
        if !self.dynamic_tools.is_empty() {
            parts.push(format!("## 动态工具（按需调整）\n{}", self.dynamic_tools));
        }
        
        parts.join("\n\n")
    }
}

impl Default for ToolRegionContent {
    fn default() -> Self {
        Self::new()
    }
}

pub struct SystemPromptBuilder {
    regions: HashMap<SystemPromptRegion, String>,
}

impl SystemPromptBuilder {
    pub fn new() -> Self {
        Self {
            regions: HashMap::new(),
        }
    }

    pub fn set_region(&mut self, region: SystemPromptRegion, content: String) {
        self.regions.insert(region, content);
    }

    pub fn get_region(&self, region: &SystemPromptRegion) -> Option<&String> {
        self.regions.get(region)
    }

    pub fn clear_region(&mut self, region: &SystemPromptRegion) {
        self.regions.remove(region);
    }

    pub fn build(&self) -> String {
        let mut ordered_regions: Vec<(&SystemPromptRegion, &String)> = 
            self.regions.iter().collect();
        ordered_regions.sort_by_key(|(r, _)| r.order());

        let mut parts = Vec::new();
        for (region, content) in ordered_regions {
            if !content.is_empty() {
                parts.push(format!("{}\n\n{}", region.header(), content));
            }
        }
        parts.join("\n\n---\n\n")
    }

    pub fn build_with_emphasis(&self, emphasis_items: &[String]) -> String {
        let mut builder = self.clone();
        
        if !emphasis_items.is_empty() {
            let emphasis_content = emphasis_items
                .iter()
                .map(|e| format!("- {}", e))
                .collect::<Vec<_>>()
                .join("\n");
            builder.set_region(SystemPromptRegion::EmphasizedConstraints, emphasis_content);
        }
        
        builder.build()
    }
}

impl Default for SystemPromptBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for SystemPromptBuilder {
    fn clone(&self) -> Self {
        Self {
            regions: self.regions.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_region_order() {
        assert!(SystemPromptRegion::RoleDefinition.order() < SystemPromptRegion::EmphasizedConstraints.order());
        assert!(SystemPromptRegion::EmphasizedConstraints.order() < SystemPromptRegion::OutputFormat.order());
        assert!(SystemPromptRegion::OutputFormat.order() < SystemPromptRegion::Tools.order());
        assert!(SystemPromptRegion::Tools.order() < SystemPromptRegion::ExtractionPrompt.order());
    }

    #[test]
    fn test_build_system_prompt() {
        let mut builder = SystemPromptBuilder::new();
        builder.set_region(SystemPromptRegion::RoleDefinition, "你是计划Agent".to_string());
        builder.set_region(SystemPromptRegion::OutputFormat, "输出JSON格式".to_string());
        
        let result = builder.build();
        assert!(result.contains("角色定义"));
        assert!(result.contains("输出格式"));
        assert!(result.contains("你是计划Agent"));
    }

    #[test]
    fn test_build_with_emphasis() {
        let mut builder = SystemPromptBuilder::new();
        builder.set_region(SystemPromptRegion::RoleDefinition, "你是计划Agent".to_string());
        
        let emphasis = vec!["必须使用异步方式".to_string(), "注意错误处理".to_string()];
        let result = builder.build_with_emphasis(&emphasis);
        
        assert!(result.contains("重要约束"));
        assert!(result.contains("必须使用异步方式"));
        assert!(result.contains("注意错误处理"));
    }

    #[test]
    fn test_tool_region_content() {
        let tool_content = ToolRegionContent::new()
            .with_builtin("file_read: 读取文件\nfile_write: 写入文件")
            .with_dynamic("http_request: HTTP请求\ncode_execute: 执行代码");
        
        let result = tool_content.build();
        assert!(result.contains("内置工具（固定）"));
        assert!(result.contains("动态工具（按需调整）"));
        assert!(result.contains("file_read"));
        assert!(result.contains("http_request"));
    }

    #[test]
    fn test_build_with_tools() {
        let mut builder = SystemPromptBuilder::new();
        builder.set_region(SystemPromptRegion::RoleDefinition, "你是执行Agent".to_string());
        
        let tool_content = ToolRegionContent::new()
            .with_builtin("file_read: 读取文件")
            .with_dynamic("custom_tool: 自定义工具")
            .build();
        builder.set_region(SystemPromptRegion::Tools, tool_content);
        
        let result = builder.build();
        assert!(result.contains("可用工具"));
        assert!(result.contains("内置工具（固定）"));
        assert!(result.contains("动态工具（按需调整）"));
    }
}
