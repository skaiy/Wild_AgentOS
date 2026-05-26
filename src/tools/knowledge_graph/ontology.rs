use super::types::{OntologyTerm, OntologyTermType};
use std::sync::OnceLock;

static BUILT_IN_ONTOLOGY: OnceLock<Vec<OntologyTerm>> = OnceLock::new();

pub struct OntologyManager;

impl OntologyManager {
    pub fn new() -> Self {
        Self
    }

    pub fn get_vocabulary(&self, domain: Option<&str>) -> Vec<OntologyTerm> {
        let all = Self::built_in_terms();
        match domain {
            Some(d) => all.iter().filter(|t| t.iri.contains(d)).cloned().collect(),
            None => all.clone(),
        }
    }

    pub fn format_vocabulary_for_prompt(&self, terms: &[OntologyTerm]) -> String {
        let mut classes: Vec<&OntologyTerm> = Vec::new();
        let mut properties: Vec<&OntologyTerm> = Vec::new();
        let mut relations: Vec<&OntologyTerm> = Vec::new();
        for t in terms {
            match t.term_type {
                OntologyTermType::Class => classes.push(t),
                OntologyTermType::Property => properties.push(t),
                OntologyTermType::Relation => relations.push(t),
            }
        }
        let mut result = String::new();
        if !classes.is_empty() {
            result.push_str("## 可用实体类型\n");
            for c in &classes {
                result.push_str(&format!(
                    "- IRI: {} | 名称: {} | {}\n",
                    c.iri, c.label, c.description
                ));
            }
        }
        if !properties.is_empty() {
            result.push_str("## 可用属性\n");
            for p in &properties {
                result.push_str(&format!(
                    "- IRI: {} | 名称: {} | {}\n",
                    p.iri, p.label, p.description
                ));
            }
        }
        if !relations.is_empty() {
            result.push_str("## 可用关系\n");
            for r in &relations {
                result.push_str(&format!(
                    "- IRI: {} | 名称: {} | {}\n",
                    r.iri, r.label, r.description
                ));
            }
        }
        result
    }

    fn built_in_terms() -> &'static Vec<OntologyTerm> {
        BUILT_IN_ONTOLOGY.get_or_init(|| {
            vec![
                OntologyTerm {
                    iri: "https://agentos.ontology/core/Person".into(),
                    label: "人物".into(),
                    description: "表示一个人".into(),
                    term_type: OntologyTermType::Class,
                },
                OntologyTerm {
                    iri: "https://agentos.ontology/core/Organization".into(),
                    label: "组织".into(),
                    description: "表示一个组织或公司".into(),
                    term_type: OntologyTermType::Class,
                },
                OntologyTerm {
                    iri: "https://agentos.ontology/core/Concept".into(),
                    label: "概念".into(),
                    description: "表示一个抽象概念".into(),
                    term_type: OntologyTermType::Class,
                },
                OntologyTerm {
                    iri: "https://agentos.ontology/core/Event".into(),
                    label: "事件".into(),
                    description: "表示一个事件".into(),
                    term_type: OntologyTermType::Class,
                },
                OntologyTerm {
                    iri: "https://agentos.ontology/business/Product".into(),
                    label: "产品".into(),
                    description: "表示一个产品或服务".into(),
                    term_type: OntologyTermType::Class,
                },
                OntologyTerm {
                    iri: "https://agentos.ontology/business/Project".into(),
                    label: "项目".into(),
                    description: "表示一个项目".into(),
                    term_type: OntologyTermType::Class,
                },
                OntologyTerm {
                    iri: "https://agentos.ontology/business/worksFor".into(),
                    label: "就职于".into(),
                    description: "人物与组织的工作关系".into(),
                    term_type: OntologyTermType::Relation,
                },
                OntologyTerm {
                    iri: "https://agentos.ontology/business/manages".into(),
                    label: "管理".into(),
                    description: "管理关系".into(),
                    term_type: OntologyTermType::Relation,
                },
                OntologyTerm {
                    iri: "https://agentos.ontology/business/dependsOn".into(),
                    label: "依赖".into(),
                    description: "依赖关系".into(),
                    term_type: OntologyTermType::Relation,
                },
                OntologyTerm {
                    iri: "https://agentos.ontology/bridge/hasSkill".into(),
                    label: "拥有技能".into(),
                    description: "实体与技能的关联".into(),
                    term_type: OntologyTermType::Relation,
                },
                OntologyTerm {
                    iri: "https://agentos.ontology/bridge/applicableIn".into(),
                    label: "适用于".into(),
                    description: "技能适用的业务场景".into(),
                    term_type: OntologyTermType::Relation,
                },
                OntologyTerm {
                    iri: "https://agentos.ontology/core/name".into(),
                    label: "名称".into(),
                    description: "实体名称".into(),
                    term_type: OntologyTermType::Property,
                },
                OntologyTerm {
                    iri: "https://agentos.ontology/core/description".into(),
                    label: "描述".into(),
                    description: "实体描述".into(),
                    term_type: OntologyTermType::Property,
                },
            ]
        })
    }
}
