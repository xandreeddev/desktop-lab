use super::*;
#[test]
fn catalog_failure_preserves_last_good_applications() {
    let assets = crate::test_adapters::adapters().assets;
    let mut catalog = Catalog::new(assets);
    let mut apps = vec![];
    let mut images = BTreeMap::new();
    let mut effects = Effects::default();
    catalog.update(
        Message::Observed(Ok(vec![])),
        &mut apps,
        &mut images,
        &mut effects,
    );
    catalog.update(
        Message::Observed(Err(DomainError::Unavailable("Observation stopped".into()))),
        &mut apps,
        &mut images,
        &mut effects,
    );
    assert!(matches!(catalog.status(), Status::Unavailable));
    assert!(apps.is_empty());
}
fn app(name: &str) -> Application {
    Application {
        id: AppId::new(format!("{name}.desktop")).unwrap(),
        name: name.into(),
        description: String::new(),
        keywords: vec![],
        icon: "missing-fixture-icon".into(),
        startup_class: String::new(),
        command: LaunchCommand {
            program: "unused-fixture".into(),
            args: vec![],
            directory: None,
            terminal: false,
        },
    }
}
#[test]
fn superseded_icon_results_do_not_replace_latest_catalog_and_removed_keys_retire() {
    let mut catalog = Catalog::new(crate::test_adapters::adapters().assets);
    let mut apps = vec![];
    let mut images = BTreeMap::new();
    let mut effects = Effects::default();
    catalog.update(
        Message::Observed(Ok(vec![app("one")])),
        &mut apps,
        &mut images,
        &mut effects,
    );
    let first = effects.tasks.remove(0);
    catalog.update(
        Message::Observed(Ok(vec![app("two")])),
        &mut apps,
        &mut images,
        &mut effects,
    );
    assert!(effects.tasks.is_empty());
    catalog.update(first.run(), &mut apps, &mut images, &mut effects);
    assert!(images.is_empty());
    assert_eq!(effects.tasks.len(), 1);
    let image = Arc::new(ImageData::from_rgba("fixture", 1, 1, vec![255; 4]).unwrap());
    let revision = catalog.active.unwrap();
    catalog.update(
        Message::Prepared(revision, Ok(vec![("app:two.desktop".into(), image)])),
        &mut apps,
        &mut images,
        &mut effects,
    );
    assert!(images.contains_key("app:two.desktop"));
    effects.tasks.clear();
    catalog.update(
        Message::Observed(Ok(vec![])),
        &mut apps,
        &mut images,
        &mut effects,
    );
    catalog.update(
        effects.tasks.remove(0).run(),
        &mut apps,
        &mut images,
        &mut effects,
    );
    assert!(!images.contains_key("app:two.desktop"));
}
#[test]
fn recovery_with_unchanged_apps_retries_icons_without_losing_last_good_data() {
    let mut catalog = Catalog::new(crate::test_adapters::adapters().assets);
    let mut apps = vec![];
    let mut images = BTreeMap::new();
    let mut effects = Effects::default();
    catalog.update(
        Message::Observed(Ok(vec![app("one")])),
        &mut apps,
        &mut images,
        &mut effects,
    );
    let old = effects.tasks.remove(0);
    catalog.update(
        Message::Observed(Err(DomainError::Unavailable("watch ended".into()))),
        &mut apps,
        &mut images,
        &mut effects,
    );
    assert_eq!(apps, vec![app("one")]);
    catalog.update(
        Message::Observed(Ok(vec![app("one")])),
        &mut apps,
        &mut images,
        &mut effects,
    );
    catalog.update(old.run(), &mut apps, &mut images, &mut effects);
    assert_eq!(effects.tasks.len(), 1);
    assert!(matches!(catalog.status(), Status::Ready));
}
