pub mod template_engine;
pub mod schemas;

pub use template_engine::{
    TemplateManager, TemplateEngine, Template, Schema,
    build_system_prompt,
};

pub use schemas::{
    AgentTemplate, PromptSegment, SegmentType, TemplateRegistry,
    validate_template,
    create_pa_template, create_da_template, create_ca_template, create_aa_template,
};
