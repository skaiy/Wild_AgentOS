use agent_os::memory::l0_store::L0Store;
use agent_os::memory::l1_session::L1Session;
use agent_os::memory::l2_blackboard::Blackboard;
use agent_os::memory::l3_projection::ProjectionEngine;
use agent_os::memory::memory_manager::MemoryManager;
use agent_os::CoreConfig;
use std::sync::Arc;
use tempfile::tempdir;

/// Test L0 → L1 → L2 → L3 memory pipeline
#[test]
fn test_memory_pipeline() {
    let dir = tempdir().unwrap();
    let l0 = Arc::new(L0Store::new(dir.path().to_string_lossy().as_ref()).unwrap());
    let l2 = Arc::new(Blackboard::new().unwrap());
    let proj = Arc::new(ProjectionEngine::new(l2.clone(), 500));

    // L0 store/retrieve
    l0.store("iri://test/1", "test data").unwrap();
    let entry = l0.retrieve("iri://test/1").unwrap().unwrap();
    assert_eq!(entry.content, "test data");

    // L1 session
    let mut l1 = L1Session::new("agent_1", "DA", "iri://task/1");
    l1.add_summary("assistant", "Completed step 1", None);
    l1.add_summary("assistant", "Completed step 2", None);
    assert_eq!(l1.turn_count(), 2);

    // L2 blackboard
    let json_ld = r#"{"@id":"iri://task/1","@type":"Test"}"#;
    let config = agent_os::CoreConfig::default();
    l2.write_node("iri://task/1/node_1", json_ld, &config).unwrap();
    let node = l2.read_node("iri://task/1/node_1").unwrap().unwrap();
    assert_eq!(node.node_type.as_ref().unwrap(), "Test");

    // L3 projection
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let result = proj.project("iri://task/1", "reference_only", std::collections::HashMap::new()).await.unwrap();
        assert!(result.contains("task_iri"));
    });
}

/// Test MemoryManager coordination
#[test]
fn test_memory_manager() {
    let dir = tempdir().unwrap();
    let l0 = Arc::new(L0Store::new(dir.path().to_string_lossy().as_ref()).unwrap());
    let l2 = Arc::new(Blackboard::new().unwrap());
    let proj = Arc::new(ProjectionEngine::new(l2.clone(), 500));
    let mut mm = MemoryManager::new(l0.clone(), l2.clone(), proj.clone(), CoreConfig::default());

    let session = mm.create_session("agent_1", "DA", "iri://task/1");
    let sid = session.session_id().to_string();
    mm.track_session(session);

    let fetched = mm.get_session(&sid);
    assert!(fetched.is_some());
    assert_eq!(fetched.unwrap().agent_id(), "agent_1");

    let summary = mm.close_session(&sid).unwrap();
    assert_eq!(summary.agent_role, "DA");
}
