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

#[test]
fn in_memory_registry_with_base_dir_deduplicates_relative_paths() {
    let dir = std::env::temp_dir().join(format!("artmark-reg-base-{}", uuid::Uuid::now_v7()));
    let docs = dir.join("docs");
    std::fs::create_dir_all(&docs).unwrap();
    let spec = dir.join("spec.md");
    std::fs::write(&spec, "# Spec\n").unwrap();

    let mut reg = Registry::open_in_memory().unwrap().with_base_dir(&dir);
    let first = reg.add("spec.md", None, true, None).unwrap();
    let second = reg.add("./spec.md", None, false, None).unwrap();
    let third = reg.add("docs/../spec.md", None, false, None).unwrap();
    assert_eq!(second.id, first.id);
    assert_eq!(third.id, first.id);
    assert_eq!(third.aliases.len(), 3);

    let hits = reg
        .search("./spec.md", 10, &SearchFilters::default())
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, first.id);

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn filtered_search_survives_more_than_500_unfiltered_fts_matches_and_hydrates_topics() {
    let mut reg = Registry::open_in_memory().unwrap();

    for idx in 0..549 {
        let uri = format!("https://example.com/decoy-{idx}");
        let card = IndexInput {
            source_metadata: Some(json!({"title": format!("Outage Decoy {idx}")})),
            catalog: CatalogInput {
                summary: "Outage outage outage decoy".into(),
                search_text: "outage incident decoy".into(),
                topics: vec!["noise".into()],
                entities: vec![],
                tags: vec!["noise".into()],
            },
            retrieval: None,
            resolver: None,
        };
        reg.add(&uri, None, false, Some(card)).unwrap();
    }

    let target_card = IndexInput {
        source_metadata: Some(json!({"title": "GitHub Outage Postmortem"})),
        catalog: CatalogInput {
            summary: "Postmortem for database outage".into(),
            search_text: "root cause analysis for outage".into(),
            topics: vec!["incident-response".into(), "postgres".into()],
            entities: vec!["Primary DB".into()],
            tags: vec!["sev1".into()],
        },
        retrieval: None,
        resolver: None,
    };
    let target = reg
        .add(
            "https://github.com/org/repo/issues/550",
            None,
            true,
            Some(target_card),
        )
        .unwrap();

    let by_provider = reg
        .search(
            "outage",
            10,
            &SearchFilters {
                provider: Some("github"),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(by_provider.len(), 1);
    assert_eq!(by_provider[0].id, target.id);
    assert_eq!(
        by_provider[0].topics,
        vec!["incident-response".to_owned(), "postgres".to_owned()]
    );

    let by_topic = reg
        .search(
            "outage",
            10,
            &SearchFilters {
                topic: Some("INCIDENT-RESPONSE"),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(by_topic.len(), 1);
    assert_eq!(by_topic[0].id, target.id);

    let by_tag = reg
        .search(
            "outage",
            10,
            &SearchFilters {
                tag: Some("SEV1"),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(by_tag.len(), 1);
    assert_eq!(by_tag[0].id, target.id);
}

#[test]
fn search_evaluates_all_eight_filter_predicates_and_locator_short_circuit() {
    let mut reg = Registry::open_in_memory().unwrap();

    let doc = reg
        .add(
            "https://docs.google.com/document/d/FILT1/edit",
            None,
            true,
            Some(sample_card("Filter Spec", "sqlite filter pushdown spec")),
        )
        .unwrap();

    let empty_date = reg
        .search(
            "pushdown",
            10,
            &SearchFilters {
                created_after: Some("2999-01-01T00:00:00Z"),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(empty_date.is_empty());

    let matched = reg
        .search(
            "pushdown",
            10,
            &SearchFilters {
                provider: Some("google_drive"),
                kind: Some("google_doc"),
                topic: Some("ARCHITECTURE"),
                tag: Some("SPEC"),
                state: Some(ArtifactState::Indexed),
                created_after: Some("2000-01-01T00:00:00Z"),
                seen_after: Some("2000-01-01T00:00:00Z"),
                indexed_after: Some("2000-01-01T00:00:00Z"),
            },
        )
        .unwrap();
    assert_eq!(matched.len(), 1);
    assert_eq!(matched[0].id, doc.id);
    assert_eq!(matched[0].topics, vec!["architecture".to_owned()]);

    // Exact locator short-circuits even when excluded by a filter.
    let excluded_locator = reg
        .search(
            "https://docs.google.com/document/d/FILT1/view",
            10,
            &SearchFilters {
                provider: Some("github"),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(excluded_locator.is_empty());
}
