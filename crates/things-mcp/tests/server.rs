use serde_json::{Value, json};
use tempfile::TempDir;
use things_core::{Config, Service};
use things_mcp::{McpConfig, ThingsServer};

struct Fixture {
    _directory: TempDir,
    service: Service,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("tempdir");
        let path = directory.path().join("main.sqlite");
        let connection = rusqlite::Connection::open(&path).expect("fixture db");
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
                    rt1_nextInstanceStartDate INTEGER
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
                INSERT INTO Meta(key, value)
                    VALUES ('databaseVersion', '<plist><integer>29</integer></plist>');
                INSERT INTO TMSettings(uuid, uriSchemeAuthenticationToken)
                    VALUES ('RhAzEf6qDxCD5PmnZVtBZR', 'token-123');
                INSERT INTO TMArea(uuid, title) VALUES ('area-1', 'Work');
                INSERT INTO TMTag(uuid, title, "index") VALUES
                    ('tag-1', 'focus', 0), ('tag-2', 'urgent', 1);
                INSERT INTO TMTask(
                    uuid, title, notes, type, status, start, "index", project, area
                ) VALUES
                    ('todo-1', 'Write report', 'Quarterly draft', 0, 0, 1, 1, NULL, 'area-1'),
                    ('todo-2', 'Buy milk', 'Corner shop', 0, 0, 0, 2, NULL, NULL),
                    ('project-1', 'Planning', '', 1, 0, 1, 3, NULL, 'area-1'),
                    ('heading-1', 'Plan section', '', 2, 0, 1, 4, 'project-1', NULL);
                INSERT INTO TMTaskTag(tasks, tags) VALUES
                    ('todo-1', 'tag-1'), ('todo-1', 'tag-2');
                INSERT INTO TMChecklistItem(uuid, task, title, status, "index")
                    VALUES ('check-1', 'todo-1', 'Collect numbers', 0, 0);
                "#,
            )
            .expect("fixture schema");
        drop(connection);
        let service = Service::open(Config::default(), Some(&path)).expect("open fixture");
        Self {
            _directory: directory,
            service,
        }
    }

    fn server(&self, read_only: bool) -> ThingsServer {
        ThingsServer::new(
            &self.service_reopen(),
            McpConfig {
                read_only,
                default_limit: 20,
                max_limit: 100,
            },
        )
    }

    fn service_reopen(&self) -> Service {
        Service::open(
            self.service.config().clone(),
            Some(self.service.database().path()),
        )
        .expect("reopen fixture")
    }
}

#[tokio::test]
async fn read_only_mode_hides_mutation_tools() {
    let fixture = Fixture::new();
    let server = fixture.server(true);
    let names = server.tool_names();
    assert!(names.iter().any(|name| name == "list"));
    assert!(names.iter().any(|name| name == "stats"));
    assert!(!names.iter().any(|name| name == "add_todo"));
    assert!(!names.iter().any(|name| name == "delete"));
    assert!(!names.iter().any(|name| name == "empty_trash"));
    assert!(!names.iter().any(|name| name == "add_area"));
    assert!(!names.iter().any(|name| name == "import"));
}

#[tokio::test]
async fn read_tools_are_annotated_read_only() {
    let fixture = Fixture::new();
    let server = fixture.server(true);
    for name in ["list", "search", "get", "stats", "list_areas", "list_tags"] {
        let result = server
            .call_result(name, read_arguments(name))
            .expect("tool");
        assert_eq!(result.is_error, Some(false), "{name}");
    }
    let tools = server.tool_definitions();
    for tool in &tools {
        let annotations = tool.annotations.as_ref().expect("annotations");
        let expected = matches!(
            tool.name.as_ref(),
            "list"
                | "search"
                | "get"
                | "list_todos"
                | "list_projects"
                | "list_areas"
                | "list_tags"
                | "list_headings"
                | "stats"
                | "get_agent_brief"
        );
        assert_eq!(annotations.read_only_hint, Some(expected), "{}", tool.name);
        assert_eq!(annotations.destructive_hint, Some(false), "{}", tool.name);
    }
}

#[tokio::test]
async fn agent_brief_explains_repeating_template_resolution() {
    let fixture = Fixture::new();
    let server = fixture.server(true);
    let brief = call(&server, "get_agent_brief", json!({}));
    let rules = brief["rules"].as_array().expect("rules");
    assert!(
        rules.iter().any(|rule| {
            let text = rule.as_str().unwrap_or_default();
            text.contains("title lookup prefers the generated instance")
                && text.contains("address the template by UUID")
        }),
        "agent brief must explain how to address a repeating template"
    );
}

#[tokio::test]
async fn writable_mode_exposes_the_complete_tool_surface() {
    let fixture = Fixture::new();
    let server = fixture.server(false);
    let names = server.tool_names();
    for name in [
        "list",
        "search",
        "get",
        "list_todos",
        "list_projects",
        "list_areas",
        "list_tags",
        "list_headings",
        "stats",
        "get_agent_brief",
        "add_todo",
        "add_project",
        "edit",
        "set_status",
        "delete",
        "empty_trash",
        "add_tag",
        "rename_tag",
        "delete_tag",
        "add_area",
        "rename_area",
        "delete_area",
        "import",
        "open",
        "open_repeat_editor",
    ] {
        assert!(
            names.iter().any(|value| value == name),
            "missing tool {name}"
        );
    }
}

#[tokio::test]
async fn only_delete_tools_are_annotated_destructive() {
    let fixture = Fixture::new();
    let server = fixture.server(false);
    for tool in server.tool_definitions() {
        let annotations = tool.annotations.as_ref().expect("annotations");
        let expected = matches!(
            tool.name.as_ref(),
            "delete" | "empty_trash" | "delete_tag" | "delete_area"
        );
        assert_eq!(
            annotations.destructive_hint,
            Some(expected),
            "{}",
            tool.name
        );
    }
}

#[tokio::test]
async fn heading_type_filters_are_supported() {
    let fixture = Fixture::new();
    let server = fixture.server(true);

    let listed = call(&server, "list", json!({"types": ["heading"]}));
    assert_eq!(listed["total"], 1);
    assert_eq!(listed["items"][0]["title"], "Plan section");

    let implicit = call(&server, "list", json!({}));
    let titles = implicit["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["title"].as_str().expect("title").to_string())
        .collect::<Vec<_>>();
    assert!(!titles.iter().any(|title| title == "Plan section"));
}

#[tokio::test]
async fn closed_project_tasks_are_hidden_unless_requested() {
    let fixture = Fixture::new();
    {
        let path = fixture.service.database().path();
        let connection = rusqlite::Connection::open(path).expect("fixture db");
        connection
            .execute_batch(
                r"
                INSERT INTO TMTask(uuid, title, type, status, start, trashed)
                    VALUES ('project-done', 'Old project', 1, 3, 1, 0),
                           ('todo-left', 'Left behind', 0, 0, 1, 0);
                UPDATE TMTask SET project = 'project-done' WHERE uuid = 'todo-left';
                ",
            )
            .expect("closed context");
    }
    let server = fixture.server(true);

    let active = call(&server, "list", json!({"types": ["todo"]}));
    let titles = active["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["title"].as_str().expect("title"))
        .collect::<Vec<_>>();
    assert!(!titles.contains(&"Left behind"));

    let archived = call(
        &server,
        "list",
        json!({"types": ["todo"], "include_closed_project_tasks": true}),
    );
    let titles = archived["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["title"].as_str().expect("title"))
        .collect::<Vec<_>>();
    assert!(titles.contains(&"Left behind"));
}

#[tokio::test]
async fn search_accepts_the_same_filters_as_list() {
    let fixture = Fixture::new();
    let deadline = things_core::dates::encode_date(
        chrono::NaiveDate::from_ymd_opt(2026, 10, 24).expect("deadline"),
    );
    {
        let path = fixture.service.database().path();
        let connection = rusqlite::Connection::open(path).expect("fixture db");
        connection
            .execute(
                "UPDATE TMTask SET deadline = ?1 WHERE uuid = 'todo-1'",
                [deadline],
            )
            .expect("deadline");
    }
    let server = fixture.server(true);

    let tagged = call(
        &server,
        "search",
        json!({"query": "report", "types": ["todo"], "tag": "focus"}),
    );
    assert_eq!(tagged["total"], 1);
    assert_eq!(tagged["items"][0]["id"], "todo-1");

    let wrong_tag = call(
        &server,
        "search",
        json!({"query": "report", "tag": "urgent-not-a-real-title"}),
    );
    assert_eq!(wrong_tag["total"], 0);

    let dated = call(
        &server,
        "search",
        json!({
            "query": "report",
            "deadline_from": "2026-10-01",
            "deadline_to": "2026-10-31",
            "has_deadline": true
        }),
    );
    assert_eq!(dated["total"], 1);
    assert_eq!(dated["items"][0]["id"], "todo-1");
}

#[tokio::test]
async fn edit_schema_exposes_the_full_partial_update_surface() {
    let fixture = Fixture::new();
    let server = fixture.server(false);
    let edit = server
        .tool_definitions()
        .into_iter()
        .find(|tool| tool.name == "edit")
        .expect("edit tool");
    let properties = edit
        .input_schema
        .get("properties")
        .and_then(Value::as_object)
        .expect("edit properties");
    for field in [
        "prepend_notes",
        "clear_when",
        "prepend_checklist",
        "duplicate",
        "reveal",
    ] {
        assert!(properties.contains_key(field), "missing edit field {field}");
    }

    let result = call(
        &server,
        "edit",
        json!({
            "target": "todo-1",
            "prepend_notes": "Prefix\n",
            "prepend_checklist": ["First"],
            "options": {"dry_run": true}
        }),
    );
    let url = result["url"].as_str().expect("edit URL");
    assert!(url.contains("prepend-notes=Prefix%0A"));
    assert!(url.contains("prepend-checklist-items=First"));
}

#[tokio::test]
async fn reminder_schemas_and_dry_runs_share_the_core_scheduling_rules() {
    let fixture = Fixture::new();
    let server = fixture.server(false);
    let tools = server.tool_definitions();
    for name in ["add_todo", "add_project", "edit"] {
        let tool = tools
            .iter()
            .find(|tool| tool.name == name)
            .unwrap_or_else(|| panic!("missing tool {name}"));
        let properties = tool
            .input_schema
            .get("properties")
            .and_then(Value::as_object)
            .expect("properties");
        assert!(properties.contains_key("reminder"), "{name}");
    }
    let edit = tools.iter().find(|tool| tool.name == "edit").expect("edit");
    let properties = edit
        .input_schema
        .get("properties")
        .and_then(Value::as_object)
        .expect("edit properties");
    assert!(properties.contains_key("clear_reminder"));

    let todo = call(
        &server,
        "add_todo",
        json!({
            "title": "Reminder todo",
            "when": "2026-10-09",
            "reminder": "09:00",
            "options": {"dry_run": true}
        }),
    );
    assert!(
        todo["url"]
            .as_str()
            .expect("todo url")
            .contains("when=2026-10-09%4009%3A00")
    );

    let project = call(
        &server,
        "add_project",
        json!({
            "title": "Reminder project",
            "reminder": "18:30",
            "options": {"dry_run": true}
        }),
    );
    let today = chrono::Local::now().date_naive().format("%Y-%m-%d");
    assert!(
        project["url"]
            .as_str()
            .expect("project url")
            .contains(&format!("when={today}%4018%3A30"))
    );
}

#[tokio::test]
async fn edit_reminder_keeps_an_existing_date_and_rejects_unscheduled_items() {
    let fixture = Fixture::new();
    let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 19).expect("date");
    {
        let path = fixture.service.database().path();
        let connection = rusqlite::Connection::open(path).expect("fixture db");
        connection
            .execute(
                "UPDATE TMTask SET startDate = ?1 WHERE uuid = 'todo-1'",
                [things_core::dates::encode_date(date)],
            )
            .expect("start date");
    }
    let server = fixture.server(false);
    let edited = call(
        &server,
        "edit",
        json!({
            "target": "todo-1",
            "reminder": "10:15",
            "options": {"dry_run": true}
        }),
    );
    let url = edited["url"].as_str().expect("edit url");
    assert!(url.contains("when=2026-10-19%4010%3A15"), "{url}");

    let unscheduled = server
        .call_result(
            "edit",
            json!({
                "target": "todo-2",
                "reminder": "09:00",
                "options": {"dry_run": true}
            }),
        )
        .expect("tool result");
    assert_eq!(unscheduled.is_error, Some(true));
    assert_eq!(
        unscheduled.structured_content.expect("error data")["code"],
        "conflict"
    );

    let clearing = server
        .call_result(
            "edit",
            json!({
                "target": "todo-1",
                "clear_reminder": true,
                "options": {"dry_run": true}
            }),
        )
        .expect("tool result");
    assert_eq!(clearing.is_error, Some(true));
    assert_eq!(
        clearing.structured_content.expect("error data")["code"],
        "unsupported"
    );
}

#[tokio::test]
async fn list_and_search_preview_notes_and_get_returns_full_text() {
    let fixture = Fixture::new();
    let long_notes = format!("{} tail", "日".repeat(250));
    {
        let path = fixture.service.database().path();
        let connection = rusqlite::Connection::open(path).expect("fixture db");
        connection
            .execute(
                "UPDATE TMTask SET notes = ?1 WHERE uuid = 'todo-1'",
                [&long_notes],
            )
            .expect("long notes");
    }
    let server = fixture.server(true);

    let listed = call(&server, "list", json!({"types": ["todo"]}));
    let todo = listed["items"]
        .as_array()
        .expect("items")
        .iter()
        .find(|item| item["id"] == "todo-1")
        .expect("todo");
    assert_eq!(todo["notes"].as_str().expect("notes").chars().count(), 200);
    assert_eq!(todo["notes_truncated"], true);

    let searched = call(&server, "search", json!({"query": "report"}));
    assert_eq!(searched["items"][0]["notes_truncated"], true);

    let detailed = call(&server, "get", json!({"target": "todo-1"}));
    assert_eq!(detailed["notes"], long_notes);
    assert_eq!(detailed["notes_truncated"], false);
}

#[tokio::test]
async fn search_rejects_an_empty_query() {
    let fixture = Fixture::new();
    let server = fixture.server(true);
    let result = server
        .call_result("search", json!({"query": "  "}))
        .expect("empty search is a tool error");
    assert_eq!(result.is_error, Some(true));
    let data = result.structured_content.expect("structured error");
    assert_eq!(data["code"], "invalid_input");
}

#[tokio::test]
async fn search_schema_requires_query() {
    let fixture = Fixture::new();
    let server = fixture.server(true);
    let tool = server
        .tool_definitions()
        .into_iter()
        .find(|tool| tool.name == "search")
        .expect("search tool");
    let required = tool
        .input_schema
        .get("required")
        .and_then(Value::as_array)
        .expect("required fields");
    assert!(required.iter().any(|field| field == "query"));

    let error = server
        .call_json("search", json!({}))
        .expect_err("missing query must not search the whole database");
    assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
}

#[tokio::test]
async fn mutation_schemas_mark_required_identifiers() {
    let fixture = Fixture::new();
    let server = fixture.server(false);
    let tools = server.tool_definitions();
    for (name, required) in [
        ("add_todo", &["title"][..]),
        ("add_project", &["title"][..]),
        ("edit", &["target"][..]),
        ("set_status", &["target", "status"][..]),
        ("delete", &["target"][..]),
        ("add_tag", &["name"][..]),
        ("rename_tag", &["target", "new_name"][..]),
        ("delete_tag", &["target"][..]),
        ("add_area", &["name"][..]),
        ("rename_area", &["target", "new_name"][..]),
        ("delete_area", &["target"][..]),
        ("import", &["data"][..]),
        ("open_repeat_editor", &["target"][..]),
    ] {
        let tool = tools
            .iter()
            .find(|tool| tool.name == name)
            .unwrap_or_else(|| panic!("missing tool {name}"));
        let fields = tool
            .input_schema
            .get("required")
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("{name} has no required fields"));
        for field in required {
            assert!(
                fields.iter().any(|value| value == field),
                "{name} does not require {field}"
            );
        }
    }
}

#[tokio::test]
async fn import_dry_run_returns_per_item_created_results() {
    let fixture = Fixture::new();
    let server = fixture.server(false);
    let result = call(
        &server,
        "import",
        json!({
            "data": "[{\"type\":\"project\",\"attributes\":{\"title\":\"Launch\",\"items\":[{\"type\":\"to-do\",\"attributes\":{\"title\":\"Book venue\"}}]}}]",
            "options": {"dry_run": true}
        }),
    );
    assert_eq!(result["sent"], false);
    assert_eq!(result["created"][0]["path"], "[0]");
    assert_eq!(result["created"][0]["type"], "project");
    assert_eq!(result["created"][0]["reason"], "dry-run");
    assert_eq!(result["created"][1]["path"], "[0].attributes.items[0]");
    assert_eq!(result["created"][1]["type"], "todo");
}

#[tokio::test]
async fn import_dry_run_returns_nested_updated_results() {
    let fixture = Fixture::new();
    let server = fixture.server(false);
    let result = call(
        &server,
        "import",
        json!({
            "data": "[{\"type\":\"project\",\"attributes\":{\"title\":\"Launch\",\"items\":[{\"type\":\"to-do\",\"operation\":\"update\",\"id\":\"todo-1\",\"attributes\":{\"completed\":true}}]}}]",
            "options": {"dry_run": true}
        }),
    );
    assert_eq!(result["sent"], false);
    assert_eq!(result["updated"][0]["path"], "[0].attributes.items[0]");
    assert_eq!(result["updated"][0]["id"], "todo-1");
    assert_eq!(result["updated"][0]["title"], "Write report");
    assert_eq!(result["updated"][0]["wanted_status"], "completed");
    assert_eq!(result["updated"][0]["reason"], "dry-run");
}

#[tokio::test]
async fn area_and_trash_writes_require_confirmation() {
    let fixture = Fixture::new();
    let server = fixture.server(false);

    let trash = server
        .call_result("empty_trash", json!({}))
        .expect("tool error");
    assert_eq!(trash.is_error, Some(true));
    assert_eq!(
        trash.structured_content.expect("data")["code"],
        "conflict",
        "empty_trash must refuse without an explicit confirmation"
    );

    let planned = call(
        &server,
        "empty_trash",
        json!({"dry_run": true, "yes": true}),
    );
    assert_eq!(planned["action"], "empty_trash");
    assert_eq!(planned["sent"], false);

    let area = call(&server, "add_area", json!({"name": "Work"}));
    assert_eq!(
        area["id"], "area-1",
        "existing areas are reused, not duplicated"
    );
}

#[tokio::test]
async fn list_and_stats_return_structured_content() {
    let fixture = Fixture::new();
    let server = fixture.server(true);
    let listed = call(&server, "list", json!({"view": "inbox", "limit": 10}));
    assert_eq!(listed["total"], 1);
    assert_eq!(listed["items"][0]["title"], "Buy milk");

    let stats = call(&server, "stats", json!({}));
    assert_eq!(stats["schema_version"], 29);
    assert_eq!(stats["read_only"], true);
    assert_eq!(stats["counts"]["todos"], 2);
    assert_eq!(stats["database_source"], "flag");
}

#[tokio::test]
async fn invalid_arguments_are_protocol_errors() {
    let fixture = Fixture::new();
    let server = fixture.server(true);
    let error = server
        .call_json("get", json!({"unknown": true}))
        .expect_err("unknown fields must fail");
    assert_eq!(error.code, rmcp::model::ErrorCode::INVALID_PARAMS);
}

#[tokio::test]
async fn service_validation_errors_are_tool_errors() {
    let fixture = Fixture::new();
    let server = fixture.server(false);
    let result = server
        .call_result("add_todo", json!({"title": ""}))
        .expect("validation failure is a tool result");
    assert_eq!(result.is_error, Some(true));
    let data = result.structured_content.expect("structured error");
    assert_eq!(data["code"], "invalid_input");
}

#[tokio::test]
async fn ambiguous_targets_return_candidates() {
    let fixture = Fixture::new();
    let server = fixture.server(true);
    let error = server
        .call_json("get", json!({"target": "o"}))
        .expect_err("ambiguous target");
    let data = error.data.expect("candidate data");
    assert_eq!(data["code"], "ambiguous");
    assert_eq!(data["candidates_truncated"], false);
    assert!(
        data["candidates"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
}

#[tokio::test]
async fn ambiguous_targets_are_tool_errors_not_protocol_errors() {
    let fixture = Fixture::new();
    let server = fixture.server(true);
    let result = server
        .call_result("get", json!({"target": "o"}))
        .expect("ambiguous target is a tool result");
    assert_eq!(result.is_error, Some(true));
    let data = result.structured_content.expect("structured error");
    assert_eq!(data["code"], "ambiguous");
    assert!(
        data["candidates"]
            .as_array()
            .is_some_and(|items| !items.is_empty())
    );
}

fn call(server: &ThingsServer, name: &str, arguments: Value) -> Value {
    server.call_json(name, arguments).expect("tool call")
}

fn read_arguments(name: &str) -> Value {
    match name {
        "get" => json!({"target": "todo-1"}),
        "search" => json!({"query": "report"}),
        "list_headings" => json!({"project": "project-1"}),
        _ => json!({}),
    }
}
