use artmark::{ArtifactState, CatalogInput, Error, IndexInput, Registry, SearchFilters};
use serde_json::json;

fn sample_card(title: &str, text: &str) -> IndexInput {
    IndexInput {
        source_metadata: Some(json!({"title": title, "revision": "1"})),
        catalog: CatalogInput {
            summary: format!("Summary for {title}"),
            search_text: text.to_owned(),
            topics: vec!["architecture".into()],
            entities: vec!["artmark".into()],
            tags: vec!["spec".into()],
        },
        retrieval: Some(json!({"provider": "google_drive", "external_id": "LIB1"})),
        resolver: Some("registry_test".into()),
    }
}

#[test]
fn in_memory_registry_exercises_typed_seam_and_errors() {
    let mut reg = Registry::open_in_memory().unwrap();

    // Typed error variants for invalid input, empty query, and not found.
    assert!(matches!(
        reg.add(
            "https://example.com/file?token=TEST_SECRET",
            None,
            false,
            None
        ),
        Err(Error::InvalidInput(_))
    ));
    assert!(matches!(
        reg.search("   ", 10, &SearchFilters::default()),
        Err(Error::EmptyQuery)
    ));
    assert!(matches!(
        reg.get("missing-key", false),
        Err(Error::NotFound { .. })
    ));
    assert!(matches!(
        reg.forget("missing-key"),
        Err(Error::NotFound { .. })
    ));

    // Invalid card on missing key still reports InvalidInput.
    let bad_card = IndexInput {
        source_metadata: Some(json!({"body": "forbidden"})),
        catalog: CatalogInput {
            summary: "Bad".into(),
            search_text: "Bad".into(),
            topics: vec![],
            entities: vec![],
            tags: vec![],
        },
        retrieval: None,
        resolver: None,
    };
    assert!(matches!(
        reg.index("missing-key", bad_card),
        Err(Error::InvalidInput(_))
    ));

    // Register, index, search, track access, inspect typed reports, and forget.
    let registered = reg
        .add(
            "https://docs.google.com/document/d/LIB1/edit",
            Some("Pointer Title"),
            true,
            None,
        )
        .unwrap();
    assert_eq!(registered.state, ArtifactState::Registered);

    let indexed = reg
        .index(
            "https://docs.google.com/document/d/LIB1/view",
            sample_card("Library Seam RFC", "typed library seam for cli and mcp"),
        )
        .unwrap();
    assert_eq!(indexed.id, registered.id);
    assert_eq!(indexed.state, ArtifactState::Indexed);

    let hits = reg
        .search(
            "library seam",
            10,
            &SearchFilters {
                state: Some(ArtifactState::Indexed),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, indexed.id);
    assert_eq!(hits[0].state, ArtifactState::Indexed);

    let untracked = reg.get(&indexed.id, false).unwrap();
    assert_eq!(untracked.access_count, 0);
    let tracked = reg.get(&indexed.id, true).unwrap();
    assert_eq!(tracked.access_count, 1);

    let doctor = reg.doctor().unwrap();
    assert!(doctor.healthy);
    assert_eq!(doctor.artifacts, 1);

    let stats = reg.stats().unwrap();
    assert_eq!(stats.artifacts, 1);
    assert_eq!(stats.total_accesses, 1);
    assert_eq!(stats.states.get("indexed").copied(), Some(1));

    assert_eq!(reg.reindex().unwrap(), 1);
    reg.forget(&indexed.id).unwrap();
    assert!(matches!(
        reg.get(&indexed.id, false),
        Err(Error::NotFound { .. })
    ));
}
