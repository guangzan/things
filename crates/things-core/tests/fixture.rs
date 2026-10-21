use chrono::NaiveDate;
use rusqlite::{Connection, params};
use tempfile::TempDir;
use things_core::{
    Config, EditItem, ItemQuery, ItemStatus, ItemType, Service, View, WriteOptions,
    dates::{decode_date, encode_date},
    models::Item,
    query::{Filter, NOTE_PREVIEW_CHARS, SortField, SortOrder},
};

struct Fixture {
    _directory: TempDir,
    path: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("main.sqlite");
        let connection = Connection::open(&path).expect("create fixture");
        connection
            .execute_batch(
                r#"
                CREATE TABLE Meta (key TEXT PRIMARY KEY, value TEXT);
                CREATE TABLE TMSettings (
                    uuid TEXT PRIMARY KEY,
                    uriSchemeAuthenticationToken TEXT
                );
                CREATE TABLE TMArea (
                    uuid TEXT PRIMARY KEY,
                    title TEXT,
                    visible INTEGER DEFAULT 1,
                    "index" INTEGER DEFAULT 0
                );
                CREATE TABLE TMTag (
                    uuid TEXT PRIMARY KEY,
                    title TEXT,
                    shortcut TEXT,
                    parent TEXT,
                    "index" INTEGER DEFAULT 0
                );
                CREATE TABLE TMTask (
                    uuid TEXT PRIMARY KEY,
                    title TEXT,
                    notes TEXT,
                    type INTEGER DEFAULT 0,
                    status INTEGER DEFAULT 0,
                    start INTEGER DEFAULT 0,
                    startBucket INTEGER DEFAULT 0,
                    trashed INTEGER DEFAULT 0,
                    startDate INTEGER,
                    reminderTime INTEGER,
                    deadline INTEGER,
                    deadlineSuppressionDate INTEGER,
                    stopDate REAL,
                    creationDate REAL,
                    userModificationDate REAL,
                    "index" INTEGER DEFAULT 0,
                    todayIndex INTEGER DEFAULT 0,
                    todayIndexReferenceDate INTEGER,
                    project TEXT,
                    area TEXT,
                    heading TEXT,
                    rt1_repeatingTemplate TEXT,
                    rt1_recurrenceRule BLOB,
                    rt1_nextInstanceStartDate INTEGER,
                    repeater BLOB
                );
                CREATE TABLE TMTaskTag (tasks TEXT, tags TEXT);
                CREATE TABLE TMAreaTag (areas TEXT, tags TEXT);
                CREATE TABLE TMChecklistItem (
                    uuid TEXT PRIMARY KEY,
                    task TEXT,
                    title TEXT,
                    status INTEGER DEFAULT 0,
                    stopDate REAL,
                    "index" INTEGER DEFAULT 0
                );
                "#,
            )
            .expect("schema");

        connection
            .execute(
                "INSERT INTO Meta(key, value) VALUES ('databaseVersion', '<plist><integer>29</integer></plist>')",
                [],
            )
            .expect("meta");
        connection
            .execute(
                "INSERT INTO TMSettings(uuid, uriSchemeAuthenticationToken) VALUES ('RhAzEf6qDxCD5PmnZVtBZR', 'token-123')",
                [],
            )
            .expect("settings");
        connection
            .execute(
                "INSERT INTO TMArea(uuid, title, visible, \"index\") VALUES
                 ('area-work', 'Work', 1, 0),
                 ('area-home', 'Home', 1, 1),
                 ('R4t2G8Q63aGZq4epMHNeC1', 'Real area id', 1, 2)",
                [],
            )
            .expect("areas");
        connection
            .execute(
                "INSERT INTO TMTag(uuid, title, shortcut, parent, \"index\") VALUES
                 ('tag-focus', 'focus', 'f', NULL, 0),
                 ('tag-urgent', 'urgent', NULL, NULL, 1),
                 ('tag-home', 'home', NULL, NULL, 2)",
                [],
            )
            .expect("tags");

        let today = NaiveDate::from_ymd_opt(2026, 10, 19).unwrap();
        let tomorrow = today + chrono::Duration::days(1);
        let yesterday = today - chrono::Duration::days(1);
        let created = 1_760_000_000.0;
        connection
            .execute(
                "INSERT INTO TMTask(
                    uuid, title, notes, type, status, start, startBucket, trashed,
                    startDate, reminderTime, deadline, stopDate, creationDate,
                    userModificationDate, \"index\", todayIndex, project, area, heading,
                    rt1_repeatingTemplate, rt1_recurrenceRule
                 ) VALUES
                 (?1, 'Write report', 'Draft the quarterly report', 0, 0, 1, 1, 0,
                  ?2, ?3, ?4, NULL, ?5, ?5, 1, 1, ?6, NULL, ?7, NULL, NULL),
                 (?8, 'Buy milk', 'From the corner shop', 0, 0, 0, 0, 0,
                  NULL, NULL, NULL, NULL, ?5, ?5, 2, 0, NULL, ?9, NULL, NULL, NULL),
                 (?10, 'Finished item', 'Old', 0, 3, 1, 1, 0,
                  ?2, NULL, NULL, ?5, ?5, ?5, 3, 0, NULL, NULL, NULL, NULL, NULL),
                 (?11, 'Canceled item', '', 0, 2, 1, 1, 0,
                  NULL, NULL, NULL, ?17, ?5, ?5, 4, 0, NULL, NULL, NULL, NULL, NULL),
                 (?12, 'Trashed item', '', 0, 0, 0, 0, 1,
                  NULL, NULL, NULL, NULL, ?5, ?5, 5, 0, NULL, NULL, NULL, NULL, NULL),
                 (?13, 'Quarterly planning', 'Plan next quarter', 1, 0, 1, 1, 0,
                  ?2, NULL, ?14, NULL, ?5, ?5, 6, 0, NULL, ?9, NULL, NULL, NULL),
                 (?15, 'Plan section', '', 2, 0, 1, 1, 0,
                  NULL, NULL, NULL, NULL, ?5, ?5, 7, 0, ?13, NULL, NULL, NULL, NULL),
                 (?16, 'Repeating review', 'Weekly review', 0, 0, 1, 1, 0,
                  ?2, NULL, NULL, NULL, ?5, ?5, 8, 0, ?13, NULL, NULL, NULL, X'00'),
                 (?18, 'Zeta archive', '', 2, 3, 1, 1, 0,
                  NULL, NULL, NULL, ?5, ?5, ?5, 9, 0, ?13, NULL, NULL, NULL, NULL)",
                params![
                    "todo-report",
                    encode_date(today),
                    (9_i64 << 26) | (30_i64 << 20),
                    encode_date(tomorrow),
                    created,
                    "project-quarterly",
                    "heading-plan",
                    "todo-milk",
                    "area-home",
                    "todo-done",
                    "todo-canceled",
                    "todo-trashed",
                    "project-quarterly",
                    encode_date(yesterday),
                    "heading-plan",
                    "todo-repeat",
                    created + 60.0,
                    "R4t2G8Q63aGZq4epMHNeC2"
                ],
            )
            .expect("tasks");
        connection
            .execute(
                "INSERT INTO TMTaskTag(tasks, tags) VALUES
                 ('todo-report', 'tag-focus'),
                 ('todo-report', 'tag-urgent'),
                 ('todo-milk', 'tag-home'),
                 ('todo-repeat', 'tag-focus')",
                [],
            )
            .expect("task tags");
        connection
            .execute(
                "INSERT INTO TMAreaTag(areas, tags) VALUES ('area-work', 'tag-focus')",
                [],
            )
            .expect("area tags");
        connection
            .execute(
                "INSERT INTO TMChecklistItem(uuid, task, title, status, stopDate, \"index\")
                 VALUES ('check-1', 'todo-report', 'Collect numbers', 0, NULL, 0),
                        ('check-2', 'todo-report', 'Write summary', 3, ?1, 1)",
                [created],
            )
            .expect("checklist");
        drop(connection);
        Self {
            _directory: directory,
            path,
        }
    }

    fn service(&self) -> Service {
        Service::open(Config::default(), Some(&self.path)).expect("open fixture")
    }
}

#[test]
fn schema_counts_and_metadata_are_readable() {
    let fixture = Fixture::new();
    let service = fixture.service();
    assert_eq!(service.database().schema_version().unwrap(), Some(29));
    assert_eq!(service.token().unwrap(), "token-123");
    let counts = service.counts().unwrap();
    assert_eq!(counts.todos, 3);
    assert_eq!(counts.projects, 1);
    assert_eq!(counts.areas, 3);
    assert_eq!(counts.tags, 3);
}

#[test]
fn incompatible_schema_fails_loudly() {
    let fixture = Fixture::new();
    let connection = Connection::open(&fixture.path).expect("fixture");
    connection
        .execute_batch(
            r"
            DROP TABLE TMChecklistItem;
            CREATE TABLE TMChecklistItem (
                uuid TEXT PRIMARY KEY,
                task TEXT,
                title TEXT
            );
            ",
        )
        .expect("drop column");
    drop(connection);
    let error = Service::open(Config::default(), Some(&fixture.path)).unwrap_err();
    assert_eq!(error.code(), things_core::error::ErrorCode::Unsupported);
    assert!(error.to_string().contains("TMChecklistItem"));
}

#[test]
fn views_include_only_expected_items() {
    let fixture = Fixture::new();
    let service = fixture.service();

    let inbox = service
        .list(&ItemQuery {
            view: Some(View::Inbox),
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(titles(&inbox.items), vec!["Buy milk"]);

    let logbook = service
        .list(&ItemQuery {
            view: Some(View::Logbook),
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(
        titles(&logbook.items),
        vec!["Canceled item", "Finished item"]
    );

    let trash = service
        .list(&ItemQuery {
            view: Some(View::Trash),
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(titles(&trash.items), vec!["Trashed item"]);

    let repeating = service
        .list(&ItemQuery {
            view: Some(View::Repeating),
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(titles(&repeating.items), vec!["Repeating review"]);
}

#[test]
fn repeating_templates_stay_out_of_ordinary_views() {
    let fixture = Fixture::new();
    let connection = Connection::open(&fixture.path).expect("fixture");
    let today = encode_date(chrono::Local::now().date_naive());
    let upcoming = encode_date(chrono::Local::now().date_naive() + chrono::Duration::days(2));
    connection
        .execute_batch(&format!(
            r#"
            INSERT INTO TMTask(
                uuid, title, type, status, start, startBucket, trashed,
                startDate, deadline, "index", project, rt1_recurrenceRule
            ) VALUES
                ('project-template', 'Template project', 1, 0, 1, 0, 0,
                 NULL, NULL, 20, NULL, X'00'),
                ('todo-template', 'Template todo', 0, 0, 1, 0, 0,
                 NULL, NULL, 21, NULL, X'00'),
                ('todo-in-template', 'Inside template project', 0, 0, 1, 0, 0,
                 NULL, NULL, 22, 'project-template', NULL),
                ('todo-template-anytime', 'Template anytime', 0, 0, 1, 0, 0,
                 NULL, NULL, 23, NULL, X'00'),
                ('todo-template-today', 'Template today', 0, 0, 1, 0, 0,
                 NULL, {today}, 24, NULL, X'00'),
                ('todo-template-upcoming', 'Template upcoming', 0, 0, 2, 0, 0,
                 {upcoming}, NULL, 25, NULL, X'00'),
                ('todo-template-someday', 'Template someday', 0, 0, 2, 0, 0,
                 NULL, NULL, 26, NULL, X'00'),
                ('trashed-template', 'Trashed template', 0, 0, 1, 0, 1,
                 NULL, NULL, 27, NULL, X'00'),
                ('trashed-project-template', 'Trashed project template', 1, 0, 1, 0, 1,
                 NULL, NULL, 28, NULL, X'00');
            "#
        ))
        .expect("insert repeating templates");
    drop(connection);

    let service = fixture.service();
    for view in [View::Anytime, View::Someday, View::Projects] {
        let page = service
            .list(&ItemQuery {
                view: Some(view),
                ..ItemQuery::default()
            })
            .unwrap();
        let visible = titles(&page.items);
        for hidden in [
            "Template project",
            "Template todo",
            "Inside template project",
            "Template anytime",
            "Template someday",
        ] {
            assert!(
                !visible.contains(&hidden),
                "{hidden:?} appeared in {view:?}"
            );
        }
    }

    let today = service
        .list(&ItemQuery {
            view: Some(View::Today),
            ..ItemQuery::default()
        })
        .unwrap();
    assert!(!titles(&today.items).contains(&"Template today"));

    let upcoming = service
        .list(&ItemQuery {
            view: Some(View::Upcoming),
            ..ItemQuery::default()
        })
        .unwrap();
    assert!(!titles(&upcoming.items).contains(&"Template upcoming"));

    let viewless = service.list(&ItemQuery::default()).unwrap();
    let visible = titles(&viewless.items);
    for hidden in [
        "Template project",
        "Template todo",
        "Inside template project",
        "Template anytime",
        "Template today",
        "Template upcoming",
        "Template someday",
        "Trashed template",
        "Trashed project template",
    ] {
        assert!(
            !visible.contains(&hidden),
            "{hidden:?} appeared in a viewless listing"
        );
    }

    let active = service
        .list(&ItemQuery {
            filter: Filter {
                trashed: Some(false),
                ..Filter::default()
            },
            ..ItemQuery::default()
        })
        .unwrap();
    let visible = titles(&active.items);
    for hidden in [
        "Template project",
        "Template todo",
        "Inside template project",
        "Template anytime",
        "Template today",
        "Template upcoming",
        "Template someday",
        "Trashed template",
        "Trashed project template",
    ] {
        assert!(
            !visible.contains(&hidden),
            "{hidden:?} appeared in an explicitly active listing"
        );
    }

    let discarded = service
        .list(&ItemQuery {
            filter: Filter {
                trashed: Some(true),
                ..Filter::default()
            },
            ..ItemQuery::default()
        })
        .unwrap();
    let visible = titles(&discarded.items);
    for shown in ["Trashed template", "Trashed project template"] {
        assert!(
            visible.contains(&shown),
            "{shown:?} missing from an explicitly trashed listing"
        );
    }

    let repeating = service
        .list(&ItemQuery {
            view: Some(View::Repeating),
            ..ItemQuery::default()
        })
        .unwrap();
    let visible = titles(&repeating.items);
    for template in [
        "Repeating review",
        "Template project",
        "Template todo",
        "Template anytime",
        "Template today",
        "Template upcoming",
        "Template someday",
    ] {
        assert!(
            visible.contains(&template),
            "{template:?} missing from Repeating"
        );
    }
    assert!(!visible.contains(&"Inside template project"));
}

#[test]
fn title_resolution_prefers_instances_but_falls_back_to_templates() {
    let fixture = Fixture::new();
    let connection = Connection::open(&fixture.path).expect("fixture");
    connection
        .execute_batch(
            r#"
            INSERT INTO TMTask(
                uuid, title, type, status, start, startBucket, trashed, "index",
                rt1_recurrenceRule
            ) VALUES
                ('same-title-template', 'Shared title', 0, 0, 1, 0, 0, 30, X'00'),
                ('same-title-instance', 'Shared title', 0, 0, 1, 0, 0, 31, NULL),
                ('template-only', 'Template only', 0, 0, 1, 0, 0, 32, X'00'),
                ('cross-kind-project-template', 'Cross-kind title', 1, 0, 1, 0, 0, 33, X'00'),
                ('cross-kind-todo', 'Cross-kind title', 0, 0, 1, 0, 0, 34, NULL);
            "#,
        )
        .expect("insert title rows");
    drop(connection);

    let service = fixture.service();
    assert_eq!(
        service.get("Shared title").unwrap().id(),
        "same-title-instance"
    );
    assert_eq!(service.get("Template only").unwrap().id(), "template-only");
    let error = service.get("Cross-kind title").unwrap_err();
    assert_eq!(error.code(), things_core::error::ErrorCode::Ambiguous);
    assert_eq!(
        error
            .candidates()
            .unwrap_or_default()
            .iter()
            .map(|candidate| candidate.id.as_str())
            .collect::<std::collections::HashSet<_>>(),
        std::collections::HashSet::from(["cross-kind-project-template", "cross-kind-todo"])
    );
}

#[test]
fn ambiguous_title_reports_truncated_candidates() {
    let fixture = Fixture::new();
    let connection = Connection::open(&fixture.path).expect("fixture");
    let mut values = Vec::new();
    for index in 0..30 {
        values.push(format!(
            "('many-{index}', 'Shared ambiguous title', 0, 0, 1, 0, 0, {index})"
        ));
    }
    connection
        .execute_batch(&format!(
            "INSERT INTO TMTask(uuid, title, type, status, start, startBucket, trashed, \"index\") VALUES {};",
            values.join(",")
        ))
        .expect("insert many same-title rows");
    drop(connection);

    let service = fixture.service();
    let error = service.get("Shared ambiguous title").unwrap_err();
    assert_eq!(error.code(), things_core::error::ErrorCode::Ambiguous);
    assert_eq!(error.candidates().map(<[_]>::len), Some(25));
    assert!(error.candidates_truncated());
}

#[test]
fn evening_is_separate_from_the_start_bucket() {
    let fixture = Fixture::new();
    let connection = Connection::open(&fixture.path).expect("fixture");
    connection
        .execute(
            "UPDATE TMTask SET start = 1, startBucket = 1 WHERE uuid = 'todo-report'",
            [],
        )
        .expect("evening flag");
    drop(connection);

    let service = fixture.service();
    let Item::Todo(todo) = service.get("todo-report").unwrap() else {
        panic!("expected todo");
    };
    assert_eq!(todo.start_bucket, things_core::StartBucket::Anytime);
    assert_eq!(todo.start_bucket_code, 1);
    assert!(todo.evening);
}

#[test]
fn active_views_hide_open_todos_in_closed_projects() {
    let fixture = Fixture::new();
    let connection = Connection::open(&fixture.path).expect("fixture");
    connection
        .execute_batch(
            r"
            INSERT INTO TMTask(uuid, title, type, status, start, startBucket, trashed)
            VALUES
                ('project-done', 'Completed project', 1, 3, 1, 0, 0),
                ('project-canceled', 'Canceled project', 1, 2, 1, 0, 0),
                ('project-trashed', 'Trashed project', 1, 0, 1, 0, 1),
                ('todo-in-done', 'Left open in completed project', 0, 0, 1, 0, 0),
                ('todo-in-canceled', 'Left open in canceled project', 0, 0, 1, 0, 0),
                ('todo-in-trashed', 'Left open in trashed project', 0, 0, 1, 0, 0);
            UPDATE TMTask SET project = 'project-done' WHERE uuid = 'todo-in-done';
            UPDATE TMTask SET project = 'project-canceled' WHERE uuid = 'todo-in-canceled';
            UPDATE TMTask SET project = 'project-trashed' WHERE uuid = 'todo-in-trashed';
            ",
        )
        .expect("insert closed project tasks");
    drop(connection);

    let service = fixture.service();
    let anytime = service
        .list(&ItemQuery {
            view: Some(View::Anytime),
            ..ItemQuery::default()
        })
        .unwrap();
    let visible = titles(&anytime.items);
    for hidden in [
        "Left open in completed project",
        "Left open in canceled project",
        "Left open in trashed project",
    ] {
        assert!(
            !visible.contains(&hidden),
            "{hidden:?} must not appear in Anytime"
        );
    }
    assert!(visible.contains(&"Write report"));

    let logout = service
        .list(&ItemQuery {
            view: Some(View::Logbook),
            ..ItemQuery::default()
        })
        .unwrap();
    assert!(titles(&logout.items).contains(&"Finished item"));

    let all = service
        .list(&ItemQuery {
            view: Some(View::Anytime),
            filter: things_core::Filter {
                include_closed_project_tasks: true,
                ..things_core::Filter::default()
            },
            ..ItemQuery::default()
        })
        .unwrap();
    assert!(titles(&all.items).contains(&"Left open in trashed project"));
}

#[test]
fn today_view_follows_app_ordering() {
    let fixture = Fixture::new();
    let connection = Connection::open(&fixture.path).expect("fixture");
    connection
        .execute("DELETE FROM TMTask", [])
        .expect("clear tasks");
    connection
        .execute("DELETE FROM TMTaskTag", [])
        .expect("clear tags");

    // Anchor to the real clock so the view predicate is deterministic.
    let today = chrono::Local::now().date_naive();
    let yesterday = today - chrono::Duration::days(1);
    let tomorrow = today + chrono::Duration::days(1);
    let packed = |date: Option<NaiveDate>| date.map(encode_date);

    let insert = |uuid: &str,
                  title: &str,
                  start: i64,
                  bucket: i64,
                  start_date: Option<NaiveDate>,
                  deadline: Option<NaiveDate>,
                  suppression: Option<NaiveDate>,
                  index: i64,
                  today_index: i64,
                  reference: Option<NaiveDate>| {
        connection
            .execute(
                "INSERT INTO TMTask(uuid, title, type, status, start, startBucket, trashed,
                        startDate, deadline, deadlineSuppressionDate, \"index\", todayIndex,
                        todayIndexReferenceDate)
                 VALUES (?1, ?2, 0, 0, ?3, ?4, 0, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    uuid,
                    title,
                    start,
                    bucket,
                    packed(start_date),
                    packed(deadline),
                    packed(suppression),
                    index,
                    today_index,
                    packed(reference),
                ],
            )
            .expect("insert task");
    };

    insert(
        "t-regular",
        "Regular",
        1,
        0,
        Some(yesterday),
        None,
        None,
        10,
        5,
        Some(today),
    );
    insert(
        "t-regular-old",
        "Regular Old",
        1,
        0,
        Some(yesterday),
        None,
        None,
        11,
        1,
        Some(yesterday),
    );
    insert(
        "t-regular-noref",
        "Regular NoRef",
        1,
        0,
        Some(yesterday),
        None,
        None,
        12,
        2,
        None,
    );
    insert(
        "t-evening",
        "Evening",
        1,
        1,
        Some(yesterday),
        None,
        None,
        13,
        0,
        Some(today),
    );
    insert(
        "t-someday-past",
        "Someday Past",
        2,
        0,
        Some(yesterday),
        None,
        None,
        14,
        3,
        None,
    );
    insert(
        "t-someday-future",
        "Someday Future",
        2,
        0,
        Some(tomorrow),
        None,
        None,
        15,
        0,
        None,
    );
    insert(
        "t-overdue",
        "Overdue",
        1,
        0,
        None,
        Some(yesterday),
        None,
        16,
        0,
        None,
    );
    insert(
        "t-suppressed",
        "Suppressed",
        1,
        0,
        None,
        Some(yesterday),
        Some(yesterday),
        17,
        0,
        None,
    );
    drop(connection);

    let expected = vec![
        "Regular",
        "Regular Old",
        "Overdue",
        "Regular NoRef",
        "Someday Past",
        "Evening",
    ];

    let service = fixture.service();
    // The SQL clause alone must produce the order.
    let todos = service
        .database()
        .list_todos(&ItemQuery {
            view: Some(View::Today),
            ..ItemQuery::default()
        })
        .unwrap();
    let sql_titles: Vec<&str> = todos.items.iter().map(|todo| todo.title.as_str()).collect();
    assert_eq!(sql_titles, expected);

    // The merged (todo + project) listing must agree.
    let page = service
        .list(&ItemQuery {
            view: Some(View::Today),
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(titles(&page.items), expected);
    assert_eq!(page.total, expected.len());
}

#[test]
fn upcoming_and_deadlines_use_natural_date_order() {
    let fixture = Fixture::new();
    let connection = Connection::open(&fixture.path).expect("fixture");
    connection
        .execute("DELETE FROM TMTask", [])
        .expect("clear tasks");

    let today = chrono::Local::now().date_naive();
    let day = |offset: i64| Some(today + chrono::Duration::days(offset));
    let insert = |uuid: &str,
                  title: &str,
                  start: i64,
                  start_date: Option<NaiveDate>,
                  deadline: Option<NaiveDate>| {
        connection
            .execute(
                "INSERT INTO TMTask(uuid, title, type, status, start, startBucket, trashed,
                        startDate, deadline, \"index\")
                 VALUES (?1, ?2, 0, 0, ?3, 0, 0, ?4, ?5, 0)",
                params![
                    uuid,
                    title,
                    start,
                    start_date.map(encode_date),
                    deadline.map(encode_date),
                ],
            )
            .expect("insert task");
    };

    insert("u-early", "Early", 2, day(1), None);
    insert("u-late", "Late", 2, day(3), None);
    insert("u-nodate-mid", "NoDate Mid", 1, None, day(2));
    insert("u-nodate-far", "NoDate Far", 1, None, day(3));
    drop(connection);

    let service = fixture.service();
    let upcoming = service
        .list(&ItemQuery {
            view: Some(View::Upcoming),
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(
        titles(&upcoming.items),
        vec!["Early", "Late", "NoDate Mid", "NoDate Far"]
    );

    let deadlines = service
        .list(&ItemQuery {
            view: Some(View::Deadlines),
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(titles(&deadlines.items), vec!["NoDate Mid", "NoDate Far"]);

    let reversed = service
        .list(&ItemQuery {
            view: Some(View::Deadlines),
            order: SortOrder::Desc,
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(titles(&reversed.items), vec!["NoDate Far", "NoDate Mid"]);
}

#[test]
fn merged_views_apply_sorting_before_pagination() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let page = service
        .list(&ItemQuery {
            view: Some(View::Logbook),
            sort: SortField::Index,
            order: SortOrder::Asc,
            limit: Some(1),
            offset: 0,
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(page.total, 2);
    // Natural Logbook order is newest completion first.
    assert_eq!(titles(&page.items), vec!["Canceled item"]);
    assert!(page.has_more);

    let page = service
        .list(&ItemQuery {
            view: Some(View::Logbook),
            sort: SortField::Index,
            order: SortOrder::Desc,
            limit: Some(1),
            offset: 0,
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(titles(&page.items), vec!["Finished item"]);
}

#[test]
fn projects_view_reports_a_bounded_default_page() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let page = service
        .list(&ItemQuery {
            view: Some(View::Projects),
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.limit, 1);
    assert!(!page.has_more);
}

#[test]
fn stop_date_range_filters_completed_and_canceled_items() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let stopped_on = chrono::DateTime::from_timestamp(1_760_000_000, 0)
        .unwrap()
        .date_naive();

    // Inclusive bounds on the exact stop date return both stopped items.
    let in_range = service
        .list(&ItemQuery {
            filter: Filter {
                stop_from: Some(stopped_on),
                stop_to: Some(stopped_on),
                ..Filter::default()
            },
            sort: SortField::Title,
            order: SortOrder::Asc,
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(
        titles(&in_range.items),
        vec!["Canceled item", "Finished item"]
    );

    // Bounds that miss the stop date exclude every stopped item.
    let after = service
        .list(&ItemQuery {
            filter: Filter {
                stop_from: Some(stopped_on + chrono::Duration::days(1)),
                ..Filter::default()
            },
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(after.total, 0);

    let before = service
        .list(&ItemQuery {
            filter: Filter {
                stop_to: Some(stopped_on - chrono::Duration::days(1)),
                ..Filter::default()
            },
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(before.total, 0);
}

#[test]
fn boolean_tag_and_regex_filters_compose() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let query = ItemQuery {
        filter: Filter {
            item_types: vec![ItemType::Todo],
            status: vec![ItemStatus::Open],
            tags_all: vec!["focus".to_string()],
            tags_any: vec!["urgent".to_string(), "home".to_string()],
            tags_none: vec!["home".to_string()],
            title_regex: Some("(?i)report|review".to_string()),
            notes_regex: Some("(quarter|week)".to_string()),
            has_checklist: Some(true),
            ..Filter::default()
        },
        sort: SortField::Title,
        order: SortOrder::Asc,
        ..ItemQuery::default()
    };
    let page = service.list(&query).unwrap();
    assert_eq!(titles(&page.items), vec!["Write report"]);
    assert_eq!(page.total, 1);
}

#[test]
fn text_search_treats_sql_wildcards_literally() {
    let fixture = Fixture::new();
    let connection = Connection::open(&fixture.path).expect("fixture");
    connection
        .execute_batch(
            r"
            UPDATE TMTask SET notes = '100% complete' WHERE uuid = 'todo-report';
            UPDATE TMTask SET notes = '100 percent complete' WHERE uuid = 'todo-milk';
            ",
        )
        .expect("literal wildcard data");
    drop(connection);
    let service = fixture.service();

    let page = service
        .search(
            "%",
            &ItemQuery {
                filter: Filter {
                    item_types: vec![ItemType::Todo],
                    ..Filter::default()
                },
                ..ItemQuery::default()
            },
        )
        .unwrap();
    assert_eq!(titles(&page.items), vec!["Write report"]);
}

#[test]
fn container_filters_resolve_prefixes_and_unique_titles() {
    let fixture = Fixture::new();
    let service = fixture.service();

    for project in ["project-quarterly", "proj", "Quarterly planning"] {
        let page = service
            .list(&ItemQuery {
                filter: Filter {
                    project: Some(project.to_string()),
                    ..Filter::default()
                },
                ..ItemQuery::default()
            })
            .unwrap();
        assert!(
            titles(&page.items).contains(&"Write report"),
            "project reference {project:?} did not resolve"
        );
    }

    for area in ["area-home", "area-h"] {
        let page = service
            .list(&ItemQuery {
                filter: Filter {
                    area: Some(area.to_string()),
                    ..Filter::default()
                },
                ..ItemQuery::default()
            })
            .unwrap();
        assert!(
            titles(&page.items).contains(&"Buy milk"),
            "area reference {area:?} did not resolve"
        );
    }

    for heading in ["heading-plan", "head", "Plan section"] {
        let page = service
            .list(&ItemQuery {
                filter: Filter {
                    project: Some("project-quarterly".to_string()),
                    heading: Some(heading.to_string()),
                    ..Filter::default()
                },
                ..ItemQuery::default()
            })
            .unwrap();
        assert!(
            titles(&page.items).contains(&"Write report"),
            "heading reference {heading:?} did not resolve"
        );
    }
}

#[test]
fn container_filters_fail_instead_of_silently_returning_nothing() {
    let fixture = Fixture::new();
    let service = fixture.service();

    for (filter, message) in [
        (
            Filter {
                project: Some("missing project".to_string()),
                ..Filter::default()
            },
            "project",
        ),
        (
            Filter {
                area: Some("missing area".to_string()),
                ..Filter::default()
            },
            "area",
        ),
        (
            Filter {
                project: Some("project-quarterly".to_string()),
                heading: Some("missing heading".to_string()),
                ..Filter::default()
            },
            "heading",
        ),
    ] {
        let error = service
            .list(&ItemQuery {
                filter,
                ..ItemQuery::default()
            })
            .unwrap_err();
        assert!(
            error.code() == things_core::error::ErrorCode::NotFound
                || error.code() == things_core::error::ErrorCode::InvalidInput,
            "{message}: {}",
            error.code().as_str()
        );
    }
}

#[test]
fn typed_container_filters_ignore_same_named_other_types() {
    let fixture = Fixture::new();
    let connection = Connection::open(&fixture.path).expect("fixture");
    connection
        .execute_batch(
            r#"
            INSERT INTO TMTask(uuid, title, notes, type, status, start, "index", area)
                VALUES ('todo-area-only', 'Area-only task', '', 0, 0, 1, 20, 'area-home');
            INSERT INTO TMTaskTag(tasks, tags)
                VALUES ('todo-area-only', 'tag-home');
            "#,
        )
        .expect("same-named area and tag");
    drop(connection);
    let service = fixture.service();

    let page = service
        .list(&ItemQuery {
            filter: Filter {
                area: Some("Home".to_string()),
                ..Filter::default()
            },
            ..ItemQuery::default()
        })
        .unwrap();
    let ids = page.items.iter().map(Item::id).collect::<Vec<_>>();
    assert!(ids.contains(&"todo-area-only"));
    assert!(
        page.items.iter().all(|item| match item {
            Item::Todo(todo) =>
                todo.area_id.as_deref() == Some("area-home")
                    || todo
                        .project_id
                        .as_deref()
                        .is_some_and(|project| project == "project-quarterly"),
            Item::Project(project) => project.area_id.as_deref() == Some("area-home"),
            _ => false,
        }),
        "the work area leaked in"
    );

    let page = service
        .list(&ItemQuery {
            filter: Filter {
                tags_all: vec!["Home".to_string()],
                ..Filter::default()
            },
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(titles(&page.items), vec!["Buy milk", "Area-only task"]);

    let page = service
        .list(&ItemQuery {
            filter: Filter {
                tags_all: vec!["tag-h".to_string()],
                ..Filter::default()
            },
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(titles(&page.items), vec!["Buy milk", "Area-only task"]);

    let renamed = service
        .rename_area(
            "Home",
            "House",
            &WriteOptions {
                dry_run: true,
                ..WriteOptions::default()
            },
        )
        .unwrap();
    assert_eq!(renamed.id, "area-home");

    let renamed_tag = service
        .rename_tag(
            "home",
            "household",
            &WriteOptions {
                dry_run: true,
                ..WriteOptions::default()
            },
        )
        .unwrap();
    assert_eq!(renamed_tag.id, "tag-home");
}

#[test]
fn item_type_filter_is_respected_without_a_view() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let page = service
        .list(&ItemQuery {
            filter: Filter {
                item_types: vec![ItemType::Project],
                ..Filter::default()
            },
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(page.total, 1);
    assert!(
        page.items
            .iter()
            .all(|item| matches!(item, Item::Project(_)))
    );

    let logbook = service
        .list(&ItemQuery {
            view: Some(View::Logbook),
            filter: Filter {
                item_types: vec![ItemType::Project],
                ..Filter::default()
            },
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(logbook.total, 0);
}

#[test]
fn project_queries_never_admit_todos_or_headings() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let page = service
        .database()
        .list_projects(&ItemQuery {
            filter: Filter {
                item_types: vec![ItemType::Todo, ItemType::Project, ItemType::Heading],
                ..Filter::default()
            },
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, "project-quarterly");
}

#[test]
fn project_status_edits_require_yes_even_for_dry_run() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let edit = EditItem {
        target: "project-quarterly".to_string(),
        status: Some(ItemStatus::Completed),
        ..EditItem::default()
    };
    for options in [
        WriteOptions {
            dry_run: true,
            ..WriteOptions::default()
        },
        WriteOptions::default(),
    ] {
        let error = service.edit(&edit, &options).unwrap_err();
        assert_eq!(error.code(), things_core::error::ErrorCode::Conflict);
        assert!(error.to_string().contains("--yes"));
    }

    let planned = service
        .edit(
            &edit,
            &WriteOptions {
                dry_run: true,
                yes: true,
                ..WriteOptions::default()
            },
        )
        .unwrap();
    assert!(!planned.sent);
    assert_eq!(planned.action, "edit_project");
}

#[test]
fn viewless_listing_hides_trashed_items_unless_requested() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let all = service.list(&ItemQuery::default()).unwrap();
    assert!(!titles(&all.items).contains(&"Trashed item"));

    let trashed = service
        .list(&ItemQuery {
            filter: Filter {
                trashed: Some(true),
                ..Filter::default()
            },
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(titles(&trashed.items), vec!["Trashed item"]);
}

#[test]
fn get_returns_related_data_and_resolution_is_safe() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let Item::Todo(todo) = service.get("todo-report").unwrap() else {
        panic!("expected todo");
    };
    assert_eq!(todo.project_title.as_deref(), Some("Quarterly planning"));
    assert_eq!(todo.area_title.as_deref(), Some("Home"));
    assert_eq!(todo.heading_title.as_deref(), Some("Plan section"));
    assert_eq!(todo.tags, vec!["focus", "urgent"]);
    assert_eq!(todo.checklist.len(), 2);
    assert_eq!(todo.reminder.as_deref(), Some("09:30"));
    assert!(todo.deadline.is_some());

    let resolved = service.get("todo-repo").unwrap();
    assert_eq!(resolved.id(), "todo-report");
    let ambiguous = service.get("item").unwrap_err();
    assert_eq!(ambiguous.code(), things_core::error::ErrorCode::Ambiguous);
    assert!(
        ambiguous
            .candidates()
            .is_some_and(|values| values.len() >= 2)
    );

    let Item::Area(area) = service.get("area-home").unwrap() else {
        panic!("expected area");
    };
    assert_eq!(area.open_task_count, 3);
    assert_eq!(area.project_count, 1);

    let Item::Tag(tag) = service.get("tag-focus").unwrap() else {
        panic!("expected tag");
    };
    assert_eq!(tag.task_count, 2);
    assert_eq!(tag.area_count, 1);
}

#[test]
fn dry_run_with_tag_creation_does_not_write() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let options = WriteOptions {
        dry_run: true,
        create_missing_tags: true,
        ..WriteOptions::default()
    };
    let result = service
        .create_todo(
            things_core::CreateTodo {
                title: "Dry run".to_string(),
                tags: vec!["new-tag".to_string()],
                ..things_core::CreateTodo::default()
            },
            &options,
        )
        .unwrap();
    assert!(!result.sent);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("new-tag"))
    );
    assert!(
        service
            .tags()
            .unwrap()
            .iter()
            .all(|tag| tag.title != "new-tag")
    );
}

#[test]
fn todo_reminder_is_encoded_in_when_and_verified_from_the_database() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let result = service
        .create_todo(
            things_core::CreateTodo {
                title: "Reminder create".to_string(),
                when: Some("2026-10-09".to_string()),
                reminder: Some("09:00".to_string()),
                ..things_core::CreateTodo::default()
            },
            &WriteOptions {
                dry_run: true,
                ..WriteOptions::default()
            },
        )
        .unwrap();
    assert!(!result.sent);
    assert!(result.url.contains("when=2026-10-09%4009%3A00"));
    assert!(!result.url.contains("reminder="));
}

#[test]
fn todo_reminder_defaults_to_today_and_rejects_date_buckets() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let options = WriteOptions {
        dry_run: true,
        ..WriteOptions::default()
    };
    let created = service
        .create_todo(
            things_core::CreateTodo {
                title: "Today reminder".to_string(),
                reminder: Some("18:30".to_string()),
                ..things_core::CreateTodo::default()
            },
            &options,
        )
        .unwrap();
    let today = things_core::dates::today().format("%Y-%m-%d");
    assert!(
        created.url.contains(&format!("when={today}%4018%3A30")),
        "{}",
        created.url
    );

    for when in ["anytime", "someday"] {
        let error = service
            .create_todo(
                things_core::CreateTodo {
                    title: "Invalid reminder".to_string(),
                    when: Some(when.to_string()),
                    reminder: Some("09:00".to_string()),
                    ..things_core::CreateTodo::default()
                },
                &options,
            )
            .unwrap_err();
        assert_eq!(error.code(), things_core::error::ErrorCode::Conflict);
    }
}

#[test]
fn todo_edit_reminder_preserves_the_existing_scheduled_date() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let result = service
        .edit(
            &EditItem {
                target: "todo-report".to_string(),
                reminder: Some("10:15".to_string()),
                ..EditItem::default()
            },
            &WriteOptions {
                dry_run: true,
                ..WriteOptions::default()
            },
        )
        .unwrap();
    assert!(
        result.url.contains("when=2026-10-19%4010%3A15"),
        "{}",
        result.url
    );
    assert!(!result.url.contains("when=today"), "{}", result.url);
}

#[test]
fn todo_edit_reminder_rejects_unscheduled_items_and_invalid_times() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let options = WriteOptions {
        dry_run: true,
        ..WriteOptions::default()
    };
    let error = service
        .edit(
            &EditItem {
                target: "todo-milk".to_string(),
                reminder: Some("09:00".to_string()),
                ..EditItem::default()
            },
            &options,
        )
        .unwrap_err();
    assert_eq!(error.code(), things_core::error::ErrorCode::Conflict);

    let error = service
        .edit(
            &EditItem {
                target: "todo-report".to_string(),
                reminder: Some("9:00".to_string()),
                ..EditItem::default()
            },
            &options,
        )
        .unwrap_err();
    assert_eq!(error.code(), things_core::error::ErrorCode::InvalidDate);
}

#[test]
fn todo_edit_reminder_with_when_updates_the_date_and_time_together() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let result = service
        .edit(
            &EditItem {
                target: "todo-report".to_string(),
                when: Some("tomorrow".to_string()),
                reminder: Some("07:45".to_string()),
                ..EditItem::default()
            },
            &WriteOptions {
                dry_run: true,
                ..WriteOptions::default()
            },
        )
        .unwrap();
    let tomorrow = (things_core::dates::today() + chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    assert!(
        result.url.contains(&format!("when={tomorrow}%4007%3A45")),
        "{}",
        result.url
    );
}

#[test]
fn todo_edit_rejects_clear_reminder_without_changing_the_schedule() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let error = service
        .edit(
            &EditItem {
                target: "todo-report".to_string(),
                clear_reminder: true,
                ..EditItem::default()
            },
            &WriteOptions {
                dry_run: true,
                ..WriteOptions::default()
            },
        )
        .unwrap_err();
    assert_eq!(error.code(), things_core::error::ErrorCode::Unsupported);
}

#[test]
fn project_edit_rejects_project_and_checklist_fields() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let options = WriteOptions {
        dry_run: true,
        ..WriteOptions::default()
    };
    let project_as_parent = EditItem {
        target: "project-quarterly".to_string(),
        project: Some("project-quarterly".to_string()),
        ..EditItem::default()
    };
    let error = service.edit(&project_as_parent, &options).unwrap_err();
    assert_eq!(error.code(), things_core::error::ErrorCode::InvalidInput);

    let checklist = EditItem {
        target: "project-quarterly".to_string(),
        append_checklist: vec!["Not a project checklist".to_string()],
        ..EditItem::default()
    };
    let error = service.edit(&checklist, &options).unwrap_err();
    assert_eq!(error.code(), things_core::error::ErrorCode::InvalidInput);
}

#[test]
fn invalid_import_shapes_are_rejected_before_sending() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let options = WriteOptions {
        dry_run: true,
        ..WriteOptions::default()
    };
    for payload in [
        r#"[{"type":"to-do","attributes":{"title":""}}]"#,
        r#"[{"type":"heading","attributes":{"title":"Wrong slot"}}]"#,
        r#"[{"type":"to-do","attributes":{"title":"x","tags":"not-an-array"}}]"#,
        r#"[{"type":"to-do","attributes":{"title":"x","title":"duplicate"}}]"#,
    ] {
        let error = service.import(payload, &options).unwrap_err();
        assert_eq!(
            error.code(),
            things_core::error::ErrorCode::InvalidInput,
            "{payload}"
        );
    }
}

#[test]
fn dry_run_import_uses_the_database_auth_token() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let result = service
        .import(
            r#"[{"type":"to-do","attributes":{"title":"From import"}}]"#,
            &WriteOptions {
                dry_run: true,
                ..WriteOptions::default()
            },
        )
        .unwrap();
    assert!(
        result.url.contains("auth-token=token-123"),
        "{}",
        result.url
    );
    assert!(!result.sent);
    assert_eq!(result.created.len(), 1);
    assert!(!result.created[0].confirmed);
    assert_eq!(result.created[0].reason.as_deref(), Some("dry-run"));
}

#[test]
fn dry_run_import_reports_nested_updates_without_writing() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let result = service
        .import(
            r#"[{"type":"project","attributes":{"title":"Launch","items":[
                {"type":"to-do","operation":"update","id":"todo-report","attributes":{"completed":true}}
            ]}}]"#,
            &WriteOptions {
                dry_run: true,
                ..WriteOptions::default()
            },
        )
        .unwrap();

    assert!(!result.sent);
    assert_eq!(result.updated.len(), 1);
    assert_eq!(result.updated[0].path, "[0].attributes.items[0]");
    assert_eq!(result.updated[0].id, "todo-report");
    assert_eq!(result.updated[0].title, "Write report");
    assert_eq!(
        result.updated[0].wanted_status,
        Some(things_core::ItemStatus::Completed)
    );
    assert_eq!(result.updated[0].reason.as_deref(), Some("dry-run"));
    assert!(!result.updated[0].confirmed);
}

#[test]
fn import_update_id_type_mismatch_is_rejected_before_sending() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let error = service
        .import(
            r#"[{"type":"project","operation":"update","id":"todo-report","attributes":{"completed":true}}]"#,
            &WriteOptions {
                dry_run: true,
                ..WriteOptions::default()
            },
        )
        .unwrap_err();
    assert_eq!(error.code(), things_core::error::ErrorCode::InvalidInput);
    assert!(error.to_string().contains("does not match"));
}

#[test]
fn import_update_rejects_restricted_fields_on_repeating_items() {
    let fixture = Fixture::new();
    let service = fixture.service();
    for attributes in [
        r#"{"completed":false}"#,
        r#"{"when":"tomorrow"}"#,
        r#"{"deadline":"2026-12-01"}"#,
        r#"{"canceled":false}"#,
    ] {
        let payload = format!(
            r#"[{{"type":"to-do","operation":"update","id":"todo-repeat","attributes":{attributes}}}]"#
        );
        let error = service
            .import(
                &payload,
                &WriteOptions {
                    dry_run: true,
                    ..WriteOptions::default()
                },
            )
            .unwrap_err();
        assert_eq!(
            error.code(),
            things_core::error::ErrorCode::Unsupported,
            "{attributes}"
        );
    }
}

#[test]
fn import_update_rejects_wrong_field_types_before_sending() {
    let fixture = Fixture::new();
    let service = fixture.service();
    for attributes in [
        r#"{"when":3}"#,
        r#"{"deadline":false}"#,
        r#"{"completed":"yes"}"#,
        r#"{"canceled":1}"#,
        r#"{"list-id":[]}"#,
        r#"{"heading":{}}"#,
        r#"{"area-id":3}"#,
        r#"{"creation-date":1791196200}"#,
        r#"{"completion-date":[]}"#,
    ] {
        let payload = format!(
            r#"[{{"type":"to-do","operation":"update","id":"todo-report","attributes":{attributes}}}]"#
        );
        let error = service
            .import(
                &payload,
                &WriteOptions {
                    dry_run: true,
                    ..WriteOptions::default()
                },
            )
            .unwrap_err();
        let expected = if attributes.starts_with(r#"{"creation-date""#)
            || attributes.starts_with(r#"{"completion-date""#)
        {
            things_core::error::ErrorCode::InvalidDate
        } else {
            things_core::error::ErrorCode::InvalidInput
        };
        assert_eq!(error.code(), expected, "{attributes}");
    }
}

#[test]
fn import_update_allows_a_valid_move_to_another_destination() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let result = service
        .import(
            r#"[{"type":"to-do","operation":"update","id":"todo-report","attributes":{"list":"area-home"}}]"#,
            &WriteOptions {
                dry_run: true,
                ..WriteOptions::default()
            },
        )
        .unwrap();
    assert_eq!(result.updated.len(), 1);
    assert_eq!(result.updated[0].id, "todo-report");
    assert_eq!(result.updated[0].reason.as_deref(), Some("dry-run"));
    assert!(!result.updated[0].confirmed);
}

#[test]
fn import_update_list_id_accepts_only_existing_things_ids() {
    let fixture = Fixture::new();
    let service = fixture.service();
    for reference in ["R4t2G8Q63aGZq4epMHNeCr", "aaaaaaaaaaaaaaaaaaaaaa"] {
        let payload = format!(
            r#"[{{"type":"to-do","operation":"update","id":"todo-report","attributes":{{"list-id":"{reference}"}}}}]"#
        );
        let error = service
            .import(
                &payload,
                &WriteOptions {
                    dry_run: true,
                    ..WriteOptions::default()
                },
            )
            .unwrap_err();
        assert_eq!(error.code(), things_core::error::ErrorCode::Conflict);
        assert!(error.to_string().contains("does not identify"));
    }
}

#[test]
fn import_update_rejects_unknown_or_malformed_destinations_before_sending() {
    let fixture = Fixture::new();
    let service = fixture.service();
    for attributes in [
        r#"{"list":"missing project"}"#,
        r#"{"list-id":"not-a-things-uuid"}"#,
        r#"{"list-id":""}"#,
        r#"{"area":"missing area"}"#,
        r#"{"area-id":""}"#,
        r#"{"heading":"missing heading"}"#,
        r#"{"heading-id":""}"#,
    ] {
        let payload = format!(
            r#"[{{"type":"to-do","operation":"update","id":"todo-report","attributes":{attributes}}}]"#
        );
        let error = service
            .import(
                &payload,
                &WriteOptions {
                    dry_run: true,
                    ..WriteOptions::default()
                },
            )
            .unwrap_err();
        assert_eq!(
            error.code(),
            things_core::error::ErrorCode::Conflict,
            "{attributes}: {error}"
        );
    }
}

#[test]
fn import_update_id_fields_require_things_ids_and_matching_types() {
    let fixture = Fixture::new();
    let service = fixture.service();
    for (item_type, attributes) in [
        ("to-do", r#"{"list-id":"area-home"}"#),
        ("to-do", r#"{"list-id":"area-h"}"#),
        ("to-do", r#"{"area-id":"area-home"}"#),
        ("project", r#"{"area-id":"area-home"}"#),
        ("to-do", r#"{"heading-id":"heading-plan"}"#),
        ("to-do", r#"{"heading-id":"heading-pla"}"#),
    ] {
        let id = if item_type == "project" {
            "project-quarterly"
        } else {
            "todo-report"
        };
        let payload = format!(
            r#"[{{"type":"{item_type}","operation":"update","id":"{id}","attributes":{attributes}}}]"#
        );
        let error = service
            .import(
                &payload,
                &WriteOptions {
                    dry_run: true,
                    ..WriteOptions::default()
                },
            )
            .unwrap_err();
        assert_eq!(
            error.code(),
            things_core::error::ErrorCode::Conflict,
            "{attributes}: {error}"
        );
    }
}

#[test]
fn import_update_destination_fields_must_match_the_item_type() {
    let fixture = Fixture::new();
    let service = fixture.service();
    for (item_type, id, attributes) in [
        ("to-do", "todo-report", r#"{"area":"Home"}"#),
        (
            "to-do",
            "todo-report",
            r#"{"area-id":"R4t2G8Q63aGZq4epMHNeC1"}"#,
        ),
        ("project", "project-quarterly", r#"{"list":"Home"}"#),
        (
            "project",
            "project-quarterly",
            r#"{"list-id":"R4t2G8Q63aGZq4epMHNeC1"}"#,
        ),
        (
            "project",
            "project-quarterly",
            r#"{"heading":"Plan section"}"#,
        ),
        (
            "project",
            "project-quarterly",
            r#"{"heading-id":"R4t2G8Q63aGZq4epMHNeC2"}"#,
        ),
    ] {
        let payload = format!(
            r#"[{{"type":"{item_type}","operation":"update","id":"{id}","attributes":{attributes}}}]"#
        );
        let error = service
            .import(
                &payload,
                &WriteOptions {
                    dry_run: true,
                    ..WriteOptions::default()
                },
            )
            .unwrap_err();
        assert_eq!(
            error.code(),
            things_core::error::ErrorCode::Conflict,
            "{item_type} {attributes}: {error}"
        );
        assert!(
            error.to_string().contains("not a valid destination")
                || error.to_string().contains("do not accept"),
            "{item_type} {attributes}: {error}"
        );
    }
}

#[test]
fn import_update_id_fields_accept_existing_compatible_things_ids() {
    let fixture = Fixture::new();
    let service = fixture.service();
    for (item_type, id, field, reference) in [
        ("to-do", "todo-report", "list-id", "R4t2G8Q63aGZq4epMHNeC1"),
        (
            "to-do",
            "todo-report",
            "heading-id",
            "R4t2G8Q63aGZq4epMHNeC2",
        ),
        (
            "project",
            "project-quarterly",
            "area-id",
            "R4t2G8Q63aGZq4epMHNeC1",
        ),
    ] {
        let payload = format!(
            r#"[{{"type":"{item_type}","operation":"update","id":"{id}","attributes":{{"{field}":"{reference}"}}}}]"#
        );
        let result = service
            .import(
                &payload,
                &WriteOptions {
                    dry_run: true,
                    ..WriteOptions::default()
                },
            )
            .unwrap();
        assert_eq!(result.updated.len(), 1, "{item_type} {field}");
        assert_eq!(result.updated[0].reason.as_deref(), Some("dry-run"));
    }
}

#[test]
fn import_update_validates_nested_todo_parent_context() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let error = service
        .import(
            r#"[{"type":"project","operation":"update","id":"project-quarterly","attributes":{
                "items":[{"type":"to-do","operation":"update","id":"todo-milk","attributes":{"completed":true}}]
            }}]"#,
            &WriteOptions {
                dry_run: true,
                ..WriteOptions::default()
            },
        )
        .unwrap_err();
    assert_eq!(error.code(), things_core::error::ErrorCode::Conflict);
    assert!(error.to_string().contains("does not belong"));
}

#[test]
fn checklist_update_dry_run_resolves_the_parent_and_rejects_other_fields() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let result = service
        .import(
            r#"[{"type":"to-do","operation":"update","id":"todo-report","attributes":{
                "checklist-items":[{
                    "type":"checklist-item",
                    "operation":"update",
                    "id":"check-1",
                    "attributes":{"completed":true}
                }]
            }}]"#,
            &WriteOptions {
                dry_run: true,
                ..WriteOptions::default()
            },
        )
        .unwrap();
    assert_eq!(result.updated.len(), 2);
    assert_eq!(result.updated[0].title, "Write report");
    assert_eq!(result.updated[0].reason.as_deref(), Some("dry-run"));
    assert_eq!(result.updated[1].title, "Collect numbers");
    assert_eq!(result.updated[1].wanted_status, Some(ItemStatus::Completed));
    assert_eq!(result.updated[1].reason.as_deref(), Some("dry-run"));

    let error = service
        .import(
            r#"[{"type":"to-do","operation":"update","id":"todo-report","attributes":{
                "checklist-items":[{
                    "type":"checklist-item",
                    "operation":"update",
                    "id":"check-1",
                    "attributes":{"title":"Rename step"}
                }]
            }}]"#,
            &WriteOptions {
                dry_run: true,
                ..WriteOptions::default()
            },
        )
        .unwrap_err();
    assert_eq!(error.code(), things_core::error::ErrorCode::Unsupported);
}

#[test]
fn checklist_update_rejects_a_parent_that_does_not_own_it() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let error = service
        .import(
            r#"[{"type":"to-do","operation":"update","id":"todo-milk","attributes":{
                "checklist-items":[{
                    "type":"checklist-item",
                    "operation":"update",
                    "id":"check-1",
                    "attributes":{"completed":true}
                }]
            }}]"#,
            &WriteOptions {
                dry_run: true,
                ..WriteOptions::default()
            },
        )
        .unwrap_err();
    assert_eq!(error.code(), things_core::error::ErrorCode::Conflict);
    assert!(error.to_string().contains("does not belong"));
}

#[test]
fn headings_areas_tags_and_stats_are_joined() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let headings = service.headings("Quarterly planning").unwrap();
    assert_eq!(headings.len(), 2);
    assert_eq!(headings[0].title, "Plan section");

    let areas = service.areas().unwrap();
    let home = areas.iter().find(|area| area.title == "Home").unwrap();
    assert_eq!(home.open_task_count, 3);
    let work = areas.iter().find(|area| area.title == "Work").unwrap();
    assert_eq!(work.tags, vec!["focus"]);

    let tags = service.tags().unwrap();
    let focus = tags.iter().find(|tag| tag.title == "focus").unwrap();
    assert_eq!(focus.task_count, 2);
    assert_eq!(focus.area_count, 1);

    let project = service
        .list(&ItemQuery {
            view: Some(View::Projects),
            ..ItemQuery::default()
        })
        .unwrap();
    let Item::Project(project) = &project.items[0] else {
        panic!("expected project");
    };
    assert_eq!(project.open_task_count, 2);
    assert_eq!(project.task_count, 2);
}

#[test]
fn packed_dates_round_trip_in_database_filters() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let date = NaiveDate::from_ymd_opt(2026, 10, 19).unwrap();
    let page = service
        .list(&ItemQuery {
            filter: Filter {
                start_from: Some(date),
                start_to: Some(date),
                ..Filter::default()
            },
            ..ItemQuery::default()
        })
        .unwrap();
    assert!(page.items.iter().any(|item| item.title() == "Write report"));
    assert_eq!(decode_date(encode_date(date)), Some(date));
}

#[test]
fn headings_are_only_listed_when_the_type_is_requested() {
    let fixture = Fixture::new();
    let service = fixture.service();

    let implicit = service.list(&ItemQuery::default()).unwrap();
    assert!(!titles(&implicit.items).contains(&"Plan section"));

    let requested = service
        .list(&ItemQuery {
            filter: Filter {
                item_types: vec![ItemType::Heading],
                ..Filter::default()
            },
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(
        titles(&requested.items),
        vec!["Plan section", "Zeta archive"]
    );
    assert_eq!(requested.total, 2);
}

#[test]
fn list_notes_are_previewed_and_checklist_follows_the_query_flag() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let long_notes = format!("{} tail", "日".repeat(250));
    let connection = Connection::open(&fixture.path).expect("fixture");
    connection
        .execute(
            "UPDATE TMTask SET notes = ?1 WHERE uuid = 'todo-report'",
            [&long_notes],
        )
        .expect("long notes");
    drop(connection);

    let compact = service
        .list(&ItemQuery {
            view: Some(View::Today),
            include_checklist: false,
            ..ItemQuery::default()
        })
        .unwrap();
    let Item::Todo(todo) = compact
        .items
        .iter()
        .find(|item| item.id() == "todo-report")
        .expect("todo")
    else {
        panic!("expected todo");
    };
    assert_eq!(todo.notes.chars().count(), NOTE_PREVIEW_CHARS);
    assert_eq!(todo.notes, "日".repeat(NOTE_PREVIEW_CHARS));
    assert!(todo.notes_truncated);
    assert_eq!(todo.checklist.len(), 0);

    let full = service
        .list(&ItemQuery {
            view: Some(View::Today),
            include_notes: true,
            ..ItemQuery::default()
        })
        .unwrap();
    let Item::Todo(todo) = full
        .items
        .iter()
        .find(|item| item.id() == "todo-report")
        .expect("todo")
    else {
        panic!("expected todo");
    };
    assert_eq!(todo.notes, long_notes);
    assert!(!todo.notes_truncated);
    assert_eq!(todo.checklist.len(), 2);

    let Item::Todo(todo) = service.get("todo-report").unwrap() else {
        panic!("expected todo");
    };
    assert_eq!(todo.notes, long_notes);
    assert!(!todo.notes_truncated);
}

#[test]
fn headings_resolve_by_id_prefix_and_title() {
    let fixture = Fixture::new();
    let service = fixture.service();

    for reference in ["heading-plan", "heading", "Plan section"] {
        let item = service.get(reference).unwrap();
        assert_eq!(item.id(), "heading-plan", "{reference}");
        assert_eq!(item.item_type(), ItemType::Heading);
    }
}

#[test]
fn resolution_never_matches_items_inside_the_trash() {
    let fixture = Fixture::new();
    let service = fixture.service();
    let error = service.get("Trashed item").unwrap_err();
    assert_eq!(error.code(), things_core::error::ErrorCode::NotFound);

    // The full id still works, so trashed items stay addressable.
    let item = service.get("todo-trashed").unwrap();
    assert_eq!(item.title(), "Trashed item");
}

#[test]
fn trash_count_tracks_the_trash_view() {
    let fixture = Fixture::new();
    let service = fixture.service();
    assert_eq!(service.database().trash_count().unwrap(), 1);
    let trashed = service
        .list(&ItemQuery {
            view: Some(View::Trash),
            ..ItemQuery::default()
        })
        .unwrap();
    assert_eq!(i64::try_from(trashed.total).unwrap(), 1);
}

#[test]
fn trash_view_hides_children_of_trashed_projects_by_default() {
    let fixture = Fixture::new();
    let connection = Connection::open(&fixture.path).expect("fixture");
    connection
        .execute_batch(
            r"
            INSERT INTO TMTask(uuid, title, type, status, start, trashed)
                VALUES ('project-trashed-2', 'Deleted project', 1, 0, 1, 1),
                       ('todo-left-trashed', 'Child in deleted project', 0, 0, 1, 0);
            UPDATE TMTask SET project = 'project-trashed-2' WHERE uuid = 'todo-left-trashed';
            ",
        )
        .expect("trashed project and child");
    drop(connection);

    let service = fixture.service();
    let page = service
        .list(&ItemQuery {
            view: Some(View::Trash),
            ..ItemQuery::default()
        })
        .unwrap();
    assert!(!titles(&page.items).contains(&"Child in deleted project"));
    assert!(titles(&page.items).contains(&"Deleted project"));
}

#[test]
fn empty_trash_requires_confirmation_and_reports_pending_items() {
    let fixture = Fixture::new();
    let service = fixture.service();

    let unconfirmed = service.empty_trash(&WriteOptions::default()).unwrap_err();
    assert_eq!(unconfirmed.code(), things_core::error::ErrorCode::Conflict);

    let planned = service
        .empty_trash(&WriteOptions {
            dry_run: true,
            yes: true,
            ..WriteOptions::default()
        })
        .unwrap();
    assert!(!planned.sent);
    assert_eq!(planned.action, "empty_trash");
    assert_eq!(planned.url, "applescript://empty-trash");
    assert!(planned.warnings[0].contains("1 trashed item"));
}

#[test]
fn deleting_an_item_that_is_already_trashed_reports_a_conflict() {
    let fixture = Fixture::new();
    let service = fixture.service();

    // `todo-trashed` is already in the Trash. Things' AppleScript dictionary
    // cannot address it (`to do id` fails with -1728), so the tool must not
    // attempt the delete and surface an opaque `app_unavailable` instead.
    let error = service
        .delete(
            "todo-trashed",
            &WriteOptions {
                yes: true,
                ..WriteOptions::default()
            },
        )
        .unwrap_err();
    assert_eq!(error.code(), things_core::error::ErrorCode::Conflict);
    assert!(error.to_string().contains("already in Things Trash"));
}

#[test]
fn area_mutations_validate_targets_before_writing() {
    let fixture = Fixture::new();
    let service = fixture.service();

    // An existing name is returned untouched instead of creating a duplicate.
    let existing = service.add_area("home", &WriteOptions::default()).unwrap();
    assert_eq!(existing.id, "area-home");

    let planned = service
        .add_area(
            "Errands",
            &WriteOptions {
                dry_run: true,
                ..WriteOptions::default()
            },
        )
        .unwrap();
    assert_eq!(planned.title, "Errands");
    assert_eq!(planned.id, "dry-run");

    let not_an_area = service
        .rename_area(
            "todo-report",
            "Nope",
            &WriteOptions {
                dry_run: true,
                ..WriteOptions::default()
            },
        )
        .unwrap_err();
    assert_eq!(not_an_area.code(), things_core::error::ErrorCode::NotFound);

    let unconfirmed = service
        .delete_area("Work", &WriteOptions::default())
        .unwrap_err();
    assert_eq!(unconfirmed.code(), things_core::error::ErrorCode::Conflict);

    let renamed = service
        .rename_area(
            "area-home",
            "House",
            &WriteOptions {
                dry_run: true,
                ..WriteOptions::default()
            },
        )
        .unwrap();
    assert_eq!(renamed.title, "House");
}

#[test]
fn status_changes_reject_repeating_items() {
    let fixture = Fixture::new();
    let connection = Connection::open(&fixture.path).expect("fixture");
    connection
        .execute(
            "UPDATE TMTask SET rt1_recurrenceRule = X'00' WHERE uuid = 'project-quarterly'",
            [],
        )
        .expect("make the project repeating");
    drop(connection);
    let service = fixture.service();
    let options = WriteOptions {
        dry_run: true,
        yes: true,
        ..WriteOptions::default()
    };

    for target in ["project-quarterly", "todo-repeat"] {
        let error = service
            .set_status(target, ItemStatus::Completed, &options)
            .unwrap_err();
        assert_eq!(
            error.code(),
            things_core::error::ErrorCode::Unsupported,
            "{target}"
        );
    }
}

#[test]
fn legacy_repeater_column_is_treated_as_a_repeat_rule() {
    let fixture = Fixture::new();
    let connection = Connection::open(&fixture.path).expect("fixture");
    connection
        .execute(
            "UPDATE TMTask SET repeater = X'00' WHERE uuid = 'todo-milk'",
            [],
        )
        .expect("legacy repeater");
    drop(connection);
    let service = fixture.service();

    let repeating = service
        .list(&ItemQuery {
            view: Some(View::Repeating),
            ..ItemQuery::default()
        })
        .unwrap();
    assert!(titles(&repeating.items).contains(&"Buy milk"));
    let Item::Todo(todo) = service.get("todo-milk").unwrap() else {
        panic!("expected todo");
    };
    assert!(todo.repeating);

    let error = service
        .set_status(
            "todo-milk",
            ItemStatus::Completed,
            &WriteOptions {
                dry_run: true,
                ..WriteOptions::default()
            },
        )
        .unwrap_err();
    assert_eq!(error.code(), things_core::error::ErrorCode::Unsupported);
}

fn titles(items: &[Item]) -> Vec<&str> {
    items.iter().map(Item::title).collect()
}
