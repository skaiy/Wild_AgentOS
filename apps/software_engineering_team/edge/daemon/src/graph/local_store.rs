use oxigraph::io::RdfFormat;
use oxigraph::model::*;
use oxigraph::sparql::results::QueryResultsFormat;
use oxigraph::sparql::QueryResults;
use oxigraph::store::Store;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TripleData {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub object_type: String,
}

pub struct LocalGraphStore {
    store: Store,
}

impl LocalGraphStore {
    pub fn new(db_path: &str) -> anyhow::Result<Self> {
        let store = if db_path.is_empty() {
            Store::new()?
        } else {
            Store::open(db_path)?
        };
        Ok(Self { store })
    }

    pub async fn insert_triples(&self, triples: Vec<TripleData>) -> anyhow::Result<()> {
        for t in triples {
            let subject = NamedNode::new(t.subject.as_str())?;
            let predicate = NamedNode::new(t.predicate.as_str())?;
            let object: Term = if t.object_type == "literal" {
                Literal::new_simple_literal(t.object).into()
            } else {
                NamedNode::new(t.object.as_str())?.into()
            };
            let quad = Quad::new(subject, predicate, object, GraphName::DefaultGraph);
            self.store.insert(&quad)?;
        }
        Ok(())
    }

    pub async fn query_sparql(&self, query: &str) -> anyhow::Result<Vec<serde_json::Value>> {
        let results = self.store.query(query)?;
        match results {
            QueryResults::Solutions(solutions) => {
                let results = QueryResults::from(solutions);
                let buf = results.write(Vec::new(), QueryResultsFormat::Json)?;
                let parsed: serde_json::Value = serde_json::from_slice(&buf)?;
                let bindings = parsed["results"]["bindings"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let mut rows = Vec::new();
                for binding in bindings {
                    let mut row = serde_json::Map::new();
                    if let Some(obj) = binding.as_object() {
                        for (var, val) in obj {
                            if let Some(value) = val.get("value") {
                                row.insert(var.clone(), value.clone());
                            }
                        }
                    }
                    rows.push(serde_json::Value::Object(row));
                }
                Ok(rows)
            }
            QueryResults::Graph(_graph) => Ok(Vec::new()),
            QueryResults::Boolean(val) => Ok(vec![serde_json::json!({"result": val})]),
        }
    }

    pub async fn construct_sparql(&self, query: &str) -> anyhow::Result<String> {
        let results = self.store.query(query)?;
        match results {
            QueryResults::Graph(graph) => {
                let results = QueryResults::Graph(graph);
                let buf = results.write_graph(Vec::new(), RdfFormat::NTriples)?;
                Ok(String::from_utf8(buf)?)
            }
            QueryResults::Solutions(_) => Ok(String::new()),
            QueryResults::Boolean(_) => Ok(String::new()),
        }
    }

    pub async fn clear(&self) -> anyhow::Result<()> {
        self.store.update("CLEAR ALL")?;
        Ok(())
    }
}