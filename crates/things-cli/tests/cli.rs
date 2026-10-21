use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

struct Fixture {
    _directory: TempDir,
    database: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("tempdir");
        let database = directory.path().join("main.sqlite");
        let connection = rusqlite::Connection::open(&database).expect("fixture db");
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
                INSERT INTO TMTag(uuid, title, "index") VALUES ('tag-1', 'focus', 0);
                INSERT INTO TMTask(
                    uuid, title, notes, type, status, start, "index", project, area
                ) VALUES
                    ('todo-1', 'Write report', 'Quarterly draft', 0, 0, 1, 1, NULL, 'area-1'),
                    ('todo-2', 'Buy milk', 'Corner shop', 0, 0, 0, 2, NULL, NULL),
                    ('project-1', 'Planning', '', 1, 0, 1, 3, NULL, 'area-1'),
                    ('heading-1', 'Plan section', '', 2, 0, 1, 4, 'project-1', NULL);
                "#,
            )
            .expect("fixture schema");
        drop(connection);
        Self {
            _directory: directory,
            database,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::cargo_bin("things").expect("binary");
        command
            .arg("--db")
            .arg(&self.database)
            .env_remove("THINGSDB")
            .env_remove("THINGS_READ_ONLY")
            .env_remove("THINGS_AUTH_TOKEN");
        command
    }

    fn path(&self) -> &Path {
        &self.database
    }
}

#[test]
fn stats_json_reports_schema_and_source() {
    let fixture = Fixture::new();
    let output = fixture
        .command()
        .args(["stats", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).expect("stats json");
    assert_eq!(value["schema_version"], 29);
    assert_eq!(value["database_source"], "flag");
    assert_eq!(value["counts"]["todos"], 2);
    assert_eq!(value["database"], fixture.path().display().to_string());
}

#[test]
fn project_add_dry_run_builds_a_verified_plan_without_sending() {
    let fixture = Fixture::new();
    let output = fixture
        .command()
        .args([
            "project",
            "add",
            "Launch",
            "--area",
            "Work",
            "--todo",
            "Draft plan",
            "--dry-run",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).expect("project JSON");
    assert_eq!(value["action"], "add_project");
    assert_eq!(value["sent"], false);
    assert_eq!(value["verification"]["verified"], false);
    let url = value["url"].as_str().expect("url");
    assert!(url.contains("things:///add-project?"));
    assert!(url.contains("area-id=area-1"));

    let connection = rusqlite::Connection::open(&fixture.database).expect("fixture db");
    let projects: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM TMTask WHERE type = 1 AND title = 'Launch'",
            [],
            |row| row.get(0),
        )
        .expect("count projects");
    assert_eq!(projects, 0);
}

#[test]
fn destructive_dry_run_requires_confirmation_before_producing_a_plan() {
    let fixture = Fixture::new();
    fixture
        .command()
        .args(["delete", "todo-1", "--dry-run", "--json"])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("\"error\": \"conflict\""));
}

#[test]
fn ndjson_lists_one_compact_object_per_line() {
    let fixture = Fixture::new();
    let output = fixture
        .command()
        .args(["inbox", "--format", "ndjson"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).expect("utf8");
    let lines = text.lines().collect::<Vec<_>>();
    assert_eq!(lines.len(), 1);
    assert!(!lines[0].contains('\n'));
    let value: serde_json::Value = serde_json::from_str(lines[0]).expect("ndjson item");
    assert_eq!(value["title"], "Buy milk");
}

#[test]
fn listings_preview_long_notes_unless_full_notes_are_requested() {
    let fixture = Fixture::new();
    let long_notes = format!("{} tail", "日".repeat(250));
    let connection = rusqlite::Connection::open(&fixture.database).expect("fixture db");
    connection
        .execute(
            "UPDATE TMTask SET notes = ?1 WHERE uuid = 'todo-1'",
            [&long_notes],
        )
        .expect("long notes");
    drop(connection);

    let compact = fixture
        .command()
        .args(["anytime", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let compact: serde_json::Value = serde_json::from_slice(&compact).expect("json");
    let preview = compact["items"][0]["notes"].as_str().expect("notes");
    assert_eq!(preview.chars().count(), 200);
    assert_eq!(preview, "日".repeat(200));
    assert_eq!(compact["items"][0]["notes_truncated"], true);

    let full = fixture
        .command()
        .args(["anytime", "--notes", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let full: serde_json::Value = serde_json::from_slice(&full).expect("json");
    assert_eq!(full["items"][0]["notes"], long_notes);
    assert_eq!(full["items"][0]["notes_truncated"], false);

    let shown = fixture
        .command()
        .args(["show", "todo-1", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let shown: serde_json::Value = serde_json::from_slice(&shown).expect("show json");
    assert_eq!(shown["notes"], long_notes);
    assert_eq!(shown["notes_truncated"], false);
}

#[test]
fn format_json_argument_errors_are_json_on_stderr() {
    let fixture = Fixture::new();
    fixture
        .command()
        .args(["show", "--format", "json", "--not-a-real-flag"])
        .assert()
        .code(2)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("\"error\": \"invalid_input\""));
}

#[test]
fn not_found_errors_use_exit_code_four_and_json_stderr() {
    let fixture = Fixture::new();
    let assertion = fixture
        .command()
        .args(["show", "missing-item", "--json"])
        .assert()
        .code(4)
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("\"error\": \"not_found\""));
    drop(assertion);
}

#[test]
fn ambiguous_errors_include_candidates() {
    let fixture = Fixture::new();
    fixture
        .command()
        .args(["show", "o", "--json"])
        .assert()
        .code(4)
        .stderr(
            predicate::str::contains("\"error\": \"ambiguous\"")
                .and(predicate::str::contains("\"candidates\""))
                .and(predicate::str::contains("\"candidates_truncated\": false")),
        );
}

#[test]
fn agent_brief_explains_repeating_template_resolution() {
    let fixture = Fixture::new();
    let output = fixture
        .command()
        .args(["agent-brief", "--json"])
        .output()
        .expect("agent brief");
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    let principles = value["principles"].as_array().expect("principles");
    assert!(
        principles.iter().any(|principle| {
            let text = principle.as_str().unwrap_or_default();
            text.contains("title lookups prefer the instance")
                && text.contains("address the template by id")
        }),
        "CLI agent brief must explain repeating-template resolution"
    );
}

#[test]
fn read_only_rejects_writes_before_automation() {
    let fixture = Fixture::new();
    fixture
        .command()
        .args(["--read-only", "add", "Never created", "--json"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("\"error\": \"unsupported\""));
}

#[test]
fn import_validation_runs_before_url_dispatch() {
    let fixture = Fixture::new();
    let payload = fixture.path().with_extension("json");
    fs::write(&payload, r#"{"title":"not an array"}"#).expect("payload");
    fixture
        .command()
        .args([
            "import",
            payload.to_str().expect("utf8 path"),
            "--dry-run",
            "--json",
        ])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("\"error\": \"invalid_input\""));
}

#[test]
fn import_dry_run_accepts_a_valid_payload() {
    let fixture = Fixture::new();
    let payload = fixture.path().with_extension("valid.json");
    fs::write(
        &payload,
        r#"[{"type":"to-do","attributes":{"title":"Imported task","notes":"hello"}}]"#,
    )
    .expect("payload");
    let output = fixture
        .command()
        .args([
            "import",
            payload.to_str().expect("utf8 path"),
            "--dry-run",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).expect("import json");
    assert_eq!(value["action"], "import");
    assert_eq!(value["sent"], false);
    assert!(value["url"].as_str().expect("url").contains("auth-token="));
    assert_eq!(value["created"][0]["path"], "[0]");
    assert_eq!(value["created"][0]["confirmed"], false);
    assert_eq!(value["created"][0]["reason"], "dry-run");
}

#[test]
fn import_dry_run_returns_nested_updated_results() {
    let fixture = Fixture::new();
    let payload = fixture.path().with_extension("updates.json");
    fs::write(
        &payload,
        r#"[{"type":"project","attributes":{"title":"Launch","items":[
            {"type":"to-do","operation":"update","id":"todo-1","attributes":{"completed":true}}
        ]}}]"#,
    )
    .expect("payload");
    let output = fixture
        .command()
        .args([
            "import",
            payload.to_str().expect("utf8 path"),
            "--dry-run",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).expect("import json");
    assert_eq!(value["updated"][0]["path"], "[0].attributes.items[0]");
    assert_eq!(value["updated"][0]["id"], "todo-1");
    assert_eq!(value["updated"][0]["title"], "Write report");
    assert_eq!(value["updated"][0]["wanted_status"], "completed");
    assert_eq!(value["updated"][0]["reason"], "dry-run");
}

#[test]
fn heading_type_filter_lists_headings_but_not_by_default() {
    let fixture = Fixture::new();
    let output = fixture
        .command()
        .args(["list", "--type", "heading", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).expect("heading json");
    assert_eq!(value["total"], 1);
    assert_eq!(value["items"][0]["title"], "Plan section");

    let output = fixture
        .command()
        .args(["list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).expect("list json");
    let titles = value["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|item| item["title"].as_str().expect("title").to_string())
        .collect::<Vec<_>>();
    assert!(!titles.iter().any(|title| title == "Plan section"));
}

#[test]
fn area_commands_accept_both_spellings() {
    let fixture = Fixture::new();
    for spelling in ["area", "areas"] {
        let output = fixture
            .command()
            .args([spelling, "--json"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let value: serde_json::Value = serde_json::from_slice(&output).expect("area json");
        let titles = value
            .as_array()
            .expect("areas")
            .iter()
            .map(|area| area["title"].as_str().expect("title").to_string())
            .collect::<Vec<_>>();
        assert_eq!(titles, vec!["Work"], "{spelling}");
    }
}

#[test]
fn empty_trash_requires_confirmation_before_automation() {
    let fixture = Fixture::new();
    fixture
        .command()
        .args(["empty-trash", "--json"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("\"error\": \"conflict\""));
}

#[test]
fn reminder_dry_runs_encode_the_schedule_and_preserve_an_existing_date() {
    let fixture = Fixture::new();
    let today = chrono::Local::now().date_naive();
    let today_text = today.format("%Y-%m-%d").to_string();
    let explicit = fixture
        .command()
        .args([
            "add",
            "Reminder create",
            "--when",
            "2026-10-09",
            "--reminder",
            "09:00",
            "--dry-run",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let explicit: serde_json::Value =
        serde_json::from_slice(&explicit).expect("explicit reminder json");
    assert!(
        explicit["url"]
            .as_str()
            .expect("url")
            .contains("when=2026-10-09%4009%3A00")
    );

    let defaulted = fixture
        .command()
        .args([
            "add",
            "Reminder today",
            "--reminder",
            "18:30",
            "--dry-run",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let defaulted: serde_json::Value =
        serde_json::from_slice(&defaulted).expect("default reminder json");
    assert!(
        defaulted["url"]
            .as_str()
            .expect("url")
            .contains(&format!("when={today_text}%4018%3A30"))
    );
}

#[test]
fn edit_reminder_dry_run_preserves_the_existing_schedule() {
    let fixture = Fixture::new();
    let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 19).expect("date");
    let encoded = things_core::dates::encode_date(date);
    let connection = rusqlite::Connection::open(&fixture.database).expect("fixture db");
    connection
        .execute(
            "UPDATE TMTask SET startDate = ?1 WHERE uuid = 'todo-1'",
            [encoded],
        )
        .expect("start date");
    drop(connection);

    let output = fixture
        .command()
        .args([
            "edit",
            "todo-1",
            "--reminder",
            "10:15",
            "--dry-run",
            "--json",
        ])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).expect("edit reminder json");
    let url = value["url"].as_str().expect("url");
    assert!(url.contains("when=2026-10-19%4010%3A15"), "{url}");
    assert!(!url.contains("when=today"), "{url}");
}

#[test]
fn reminder_conflicts_and_invalid_times_fail_before_dispatch() {
    let fixture = Fixture::new();
    for args in [
        vec![
            "add",
            "Wrong time",
            "--reminder",
            "9:00",
            "--dry-run",
            "--json",
        ],
        vec![
            "add",
            "No date",
            "--when",
            "anytime",
            "--reminder",
            "09:00",
            "--dry-run",
            "--json",
        ],
        vec![
            "edit",
            "todo-2",
            "--reminder",
            "09:00",
            "--dry-run",
            "--json",
        ],
        vec!["edit", "todo-1", "--clear-reminder", "--dry-run", "--json"],
    ] {
        fixture
            .command()
            .args(&args)
            .assert()
            .code(2)
            .stdout(predicate::str::is_empty())
            .stderr(predicate::str::contains("\"error\":"));
    }
}
