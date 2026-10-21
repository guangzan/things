use std::path::PathBuf;

use things_core::{
    Config, ItemQuery, ItemStatus, ItemType, Service, View,
    models::{Item, Tag, Todo},
    query::{Filter, SortField, SortOrder},
};

fn live_database() -> Option<PathBuf> {
    let base = directories::BaseDirs::new()?;
    let container = base
        .home_dir()
        .join("Library/Group Containers")
        .join(things_core::config::GROUP_CONTAINER);
    let entries = std::fs::read_dir(container).ok()?;
    for entry in entries.flatten() {
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with("ThingsData-")
        {
            continue;
        }
        let path = entry
            .path()
            .join("Things Database.thingsdatabase")
            .join("main.sqlite");
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

#[test]
fn live_schema_and_views_are_readable() {
    let Some(path) = live_database() else {
        eprintln!("skipping live Things database test: database not found");
        return;
    };
    let service = Service::open(Config::default(), Some(&path)).expect("open live database");
    let version = service.database().schema_version().expect("schema version");
    assert!(version.is_some(), "databaseVersion must be present");
    let counts = service.counts().expect("counts");
    assert!(counts.todos >= 0);

    let today = service
        .list(&ItemQuery {
            view: Some(View::Today),
            limit: Some(20),
            ..ItemQuery::default()
        })
        .expect("today");
    for item in today.items {
        match item {
            Item::Todo(todo) => {
                assert_ne!(todo.id, "");
                assert_ne!(todo.title, "");
            }
            Item::Project(project) => assert_ne!(project.id, ""),
            _ => panic!("Today returned a non-task/project item"),
        }
    }

    let repeating = service
        .list(&ItemQuery {
            view: Some(View::Repeating),
            limit: Some(10),
            ..ItemQuery::default()
        })
        .expect("repeating");
    for item in repeating.items {
        match item {
            Item::Todo(todo) => assert!(todo.repeating),
            Item::Project(project) => assert!(project.repeating),
            _ => panic!("Repeating returned an unexpected item"),
        }
    }
}

#[test]
fn live_advanced_filters_and_search() {
    let Some(path) = live_database() else {
        eprintln!("skipping live Things database test: database not found");
        return;
    };
    let service = Service::open(Config::default(), Some(&path)).expect("open live database");
    let query = ItemQuery {
        filter: Filter {
            item_types: vec![ItemType::Todo],
            status: vec![ItemStatus::Open],
            ..Filter::default()
        },
        sort: SortField::Modified,
        order: SortOrder::Desc,
        limit: Some(5),
        ..ItemQuery::default()
    };
    let page = service.list(&query).expect("filtered list");
    assert!(page.items.len() <= 5);

    let search = service
        .search(
            "a",
            &ItemQuery {
                limit: Some(5),
                ..ItemQuery::default()
            },
        )
        .expect("search");
    assert!(search.items.len() <= 5);
}

#[test]
fn live_areas_and_tags_are_readable() {
    let Some(path) = live_database() else {
        eprintln!("skipping live Things database test: database not found");
        return;
    };
    let service = Service::open(Config::default(), Some(&path)).expect("open live database");
    let areas = service.areas().expect("areas");
    let tags = service.tags().expect("tags");
    assert!(areas.iter().all(|area| !area.id.is_empty()));
    assert!(tags.iter().all(|tag| !tag.id.is_empty()));
}

#[test]
fn live_headings_and_trash_are_addressable() {
    let Some(path) = live_database() else {
        eprintln!("skipping live Things database test: database not found");
        return;
    };
    let service = Service::open(Config::default(), Some(&path)).expect("open live database");

    let headings = service
        .list(&ItemQuery {
            filter: Filter {
                item_types: vec![ItemType::Heading],
                ..Filter::default()
            },
            limit: Some(10),
            ..ItemQuery::default()
        })
        .expect("headings");
    assert!(
        headings
            .items
            .iter()
            .all(|item| matches!(item, Item::Heading(_)))
    );

    let trash = service
        .list(&ItemQuery {
            view: Some(View::Trash),
            ..ItemQuery::default()
        })
        .expect("trash");
    let stored = service.database().trash_count().expect("trash count");
    let listed = i64::try_from(trash.total).unwrap();
    assert!(listed <= stored);
    let expanded = service
        .list(&ItemQuery {
            view: Some(View::Trash),
            filter: Filter {
                include_closed_project_tasks: true,
                ..Filter::default()
            },
            ..ItemQuery::default()
        })
        .expect("expanded trash");
    assert_eq!(
        i64::try_from(expanded.total).unwrap(),
        stored,
        "expanding closed-project children must cover every trashed row"
    );
}

#[allow(dead_code)]
fn assert_todo(_: Todo) {}

#[allow(dead_code)]
fn assert_tag(_: Tag) {}
