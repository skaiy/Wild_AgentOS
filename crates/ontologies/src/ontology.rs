use std::collections::HashSet;
use std::io::Cursor;

use oxigraph::io::{RdfFormat, RdfParser};
use oxigraph::sparql::{QueryResults, SparqlEvaluator};
use oxigraph::store::Store;

use crate::graph::GraphStore;

pub struct OntologyService;

impl OntologyService {
    /// Validate RDF syntax. Returns a JSON report (never errors on bad input).
    pub fn validate_string(content: &str) -> anyhow::Result<String> {
        match GraphStore::validate_turtle(content) {
            Ok(count) => Ok(serde_json::json!({
                "valid": true,
                "triple_count": count,
                "errors": []
            })
            .to_string()),
            Err(e) => Ok(serde_json::json!({
                "valid": false,
                "triple_count": 0,
                "errors": [e.to_string()]
            })
            .to_string()),
        }
    }

    /// Convert between RDF formats.
    pub fn convert(content: &str, _from: &str, to: &str) -> anyhow::Result<String> {
        let store = GraphStore::new();
        store.load_turtle(content, None)?;
        store.serialize(to)
    }

    /// Diff two ontologies. Returns added/removed triples.
    pub fn diff(old_content: &str, new_content: &str) -> anyhow::Result<String> {
        let old_store = Store::new()?;
        let new_store = Store::new()?;

        let old_reader = Cursor::new(old_content.as_bytes());
        for quad in RdfParser::from_format(RdfFormat::Turtle).for_reader(old_reader) {
            old_store.insert(&quad?)?;
        }

        let new_reader = Cursor::new(new_content.as_bytes());
        for quad in RdfParser::from_format(RdfFormat::Turtle).for_reader(new_reader) {
            new_store.insert(&quad?)?;
        }

        let old_triples: HashSet<String> = old_store
            .iter()
            .filter_map(|q| q.ok())
            .map(|q| format!("{} {} {}", q.subject, q.predicate, q.object))
            .collect();

        let new_triples: HashSet<String> = new_store
            .iter()
            .filter_map(|q| q.ok())
            .map(|q| format!("{} {} {}", q.subject, q.predicate, q.object))
            .collect();

        let added: Vec<&String> = new_triples.difference(&old_triples).collect();
        let removed: Vec<&String> = old_triples.difference(&new_triples).collect();

        Ok(serde_json::json!({
            "added": added.len(),
            "removed": removed.len(),
            "added_triples": added,
            "removed_triples": removed,
        })
        .to_string())
    }

    /// Lint an ontology -- check for missing labels, comments, domains.
    pub fn lint(content: &str) -> anyhow::Result<String> {
        let store = Store::new()?;
        let reader = Cursor::new(content.as_bytes());
        for quad in RdfParser::from_format(RdfFormat::Turtle).for_reader(reader) {
            store.insert(&quad?)?;
        }

        let issues = Self::collect_lint_issues(&store)?;

        Ok(serde_json::json!({
            "issues": issues,
            "issue_count": issues.len(),
            "suppressed_count": 0,
        })
        .to_string())
    }

    fn collect_lint_issues(store: &Store) -> anyhow::Result<Vec<serde_json::Value>> {
        let mut issues: Vec<serde_json::Value> = Vec::new();

        // Find classes without rdfs:label
        let query = r#"
            SELECT ?class WHERE {
                { ?class a <http://www.w3.org/2002/07/owl#Class> }
                UNION
                { ?class a <http://www.w3.org/2000/01/rdf-schema#Class> }
                FILTER NOT EXISTS { ?class <http://www.w3.org/2000/01/rdf-schema#label> ?label }
            }
        "#;
        if let Ok(prepared) = SparqlEvaluator::new().parse_query(query) {
            if let Ok(QueryResults::Solutions(solutions)) = prepared.on_store(store).execute() {
                for row in solutions.flatten() {
                    if let Some(term) = row.get("class") {
                        issues.push(serde_json::json!({
                            "severity": "warning",
                            "type": "missing_label",
                            "entity": term.to_string(),
                            "message": format!("{} has no rdfs:label", term),
                        }));
                    }
                }
            }
        }

        // Find classes without rdfs:comment
        let query = r#"
            SELECT ?class WHERE {
                { ?class a <http://www.w3.org/2002/07/owl#Class> }
                UNION
                { ?class a <http://www.w3.org/2000/01/rdf-schema#Class> }
                FILTER NOT EXISTS { ?class <http://www.w3.org/2000/01/rdf-schema#comment> ?comment }
            }
        "#;
        if let Ok(prepared) = SparqlEvaluator::new().parse_query(query) {
            if let Ok(QueryResults::Solutions(solutions)) = prepared.on_store(store).execute() {
                for row in solutions.flatten() {
                    if let Some(term) = row.get("class") {
                        issues.push(serde_json::json!({
                            "severity": "warning",
                            "type": "missing_comment",
                            "entity": term.to_string(),
                            "message": format!("{} has no rdfs:comment", term),
                        }));
                    }
                }
            }
        }

        // Find properties without domain
        let query = r#"
            SELECT ?prop WHERE {
                { ?prop a <http://www.w3.org/2002/07/owl#ObjectProperty> }
                UNION
                { ?prop a <http://www.w3.org/2002/07/owl#DatatypeProperty> }
                FILTER NOT EXISTS { ?prop <http://www.w3.org/2000/01/rdf-schema#domain> ?d }
            }
        "#;
        if let Ok(prepared) = SparqlEvaluator::new().parse_query(query) {
            if let Ok(QueryResults::Solutions(solutions)) = prepared.on_store(store).execute() {
                for row in solutions.flatten() {
                    if let Some(term) = row.get("prop") {
                        issues.push(serde_json::json!({
                            "severity": "info",
                            "type": "missing_domain",
                            "entity": term.to_string(),
                            "message": format!("{} has no rdfs:domain", term),
                        }));
                    }
                }
            }
        }

        Ok(issues)
    }
}
