use std::path::{Path, PathBuf};

use chrono::{Local, NaiveDate};
use regex::Regex;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params_from_iter, types::Value};

use crate::dates::{decode_date, encode_date};
use crate::error::{Candidate, Error, Result};
use crate::models::{
    Area, ChecklistItem, Heading, Item, ItemStatus, ItemType, Project, StartBucket, Tag, Todo, View,
};
use crate::query::{ItemQuery, NOTE_PREVIEW_CHARS, Page, SortField, SortOrder};

const REQUIRED_COLUMNS: &[(&str, &[&str])] = &[
    ("Meta", &["key", "value"]),
    ("TMSettings", &["uuid", "uriSchemeAuthenticationToken"]),
    ("TMArea", &["uuid", "title", "visible", "index"]),
    ("TMTag", &["uuid", "title", "shortcut", "parent", "index"]),
    (
        "TMTask",
        &[
            "uuid",
            "title",
            "notes",
            "type",
            "status",
            "start",
            "startBucket",
            "trashed",
            "startDate",
            "reminderTime",
            "deadline",
            "deadlineSuppressionDate",
            "stopDate",
            "creationDate",
            "userModificationDate",
            "index",
            "todayIndex",
            "todayIndexReferenceDate",
            "project",
            "area",
            "heading",
            "rt1_repeatingTemplate",
            "rt1_recurrenceRule",
            "rt1_nextInstanceStartDate",
        ],
    ),
    ("TMTaskTag", &["tasks", "tags"]),
    ("TMAreaTag", &["areas", "tags"]),
    (
        "TMChecklistItem",
        &["uuid", "task", "title", "status", "stopDate", "index"],
    ),
];

const TASK_COLUMNS_PREFIX: &str = r#"
    t.uuid,
    COALESCE(t.title, ''),
    COALESCE(t.notes, ''),
    COALESCE(t.type, 0),
    COALESCE(t.status, 0),
    COALESCE(t.start, 0),
    COALESCE(t.startBucket, 0),
    COALESCE(t.trashed, 0),
    t.startDate,
    t.reminderTime,
    t.deadline,
    t.stopDate,
    t.creationDate,
    t.userModificationDate,
    COALESCE(t."index", 0),
    COALESCE(t.todayIndex, 0),
    t.todayIndexReferenceDate,
    COALESCE(t.project, ''),
    COALESCE(p.title, ''),
    COALESCE(p.trashed, 0),
    COALESCE(t.area, ''),
    COALESCE(a.title, ''),
    COALESCE(t.heading, ''),
    COALESCE(h.title, ''),
    COALESCE(t.rt1_repeatingTemplate, ''),
"#;

const TASK_FROM: &str = r"
    FROM TMTask t
    LEFT JOIN TMTask h ON h.uuid = t.heading
    LEFT JOIN TMTask p ON p.uuid = COALESCE(t.project, h.project)
    LEFT JOIN TMArea a ON a.uuid = COALESCE(t.area, p.area)
";

const MAX_CANDIDATES: usize = 25;

/// Read-only handle to one Things 3 `SQLite` database.
#[derive(Debug)]
pub struct Database {
    connection: Connection,
    path: PathBuf,
    has_legacy_repeater: bool,
}

impl Database {
    fn task_columns(&self) -> String {
        format!("{TASK_COLUMNS_PREFIX}{}", self.repeating_case("t", "p"))
    }

    fn repeating_case(&self, task_alias: &str, project_alias: &str) -> String {
        let recurrence = format!(
            "CASE WHEN {task_alias}.rt1_recurrenceRule IS NOT NULL OR {project_alias}.rt1_recurrenceRule IS NOT NULL"
        );
        let legacy = if self.has_legacy_repeater {
            format!(" OR {task_alias}.repeater IS NOT NULL OR {project_alias}.repeater IS NOT NULL")
        } else {
            String::new()
        };
        format!("{recurrence}{legacy} THEN 1 ELSE 0 END")
    }

    fn repeating_predicate(&self, task_alias: &str, project_alias: &str) -> String {
        let recurrence = format!(
            "({task_alias}.rt1_recurrenceRule IS NOT NULL OR {project_alias}.rt1_recurrenceRule IS NOT NULL"
        );
        let legacy = if self.has_legacy_repeater {
            format!(" OR {task_alias}.repeater IS NOT NULL OR {project_alias}.repeater IS NOT NULL")
        } else {
            String::new()
        };
        format!("{recurrence}{legacy})")
    }

    fn own_repeating_predicate(&self, task_alias: &str) -> String {
        let recurrence = format!("({task_alias}.rt1_recurrenceRule IS NOT NULL");
        let legacy = if self.has_legacy_repeater {
            format!(" OR {task_alias}.repeater IS NOT NULL")
        } else {
            String::new()
        };
        format!("{recurrence}{legacy})")
    }

    /// Rows that are not ordinary work: a repeating template itself and any
    /// to-do inside a repeating project template.
    fn template_predicate(&self, task_alias: &str, project_alias: &str) -> String {
        self.repeating_predicate(task_alias, project_alias)
    }
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let connection = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|source| match source {
            rusqlite::Error::SqliteFailure(error, _)
                if matches!(
                    error.code,
                    rusqlite::ErrorCode::CannotOpen | rusqlite::ErrorCode::PermissionDenied
                ) =>
            {
                Error::Permission(format!("{}", path.display()))
            }
            source => Error::DatabaseRead {
                path: path.clone(),
                source,
            },
        })?;
        connection.busy_timeout(std::time::Duration::from_secs(2))?;
        let mut database = Self {
            connection,
            path,
            has_legacy_repeater: false,
        };
        database.verify_schema()?;
        Ok(database)
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn schema_version(&self) -> Result<Option<i64>> {
        let value: Option<String> = self
            .connection
            .query_row(
                "SELECT value FROM Meta WHERE key = 'databaseVersion'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value.and_then(|text| {
            text.split("<integer>")
                .nth(1)
                .and_then(|tail| tail.split("</integer>").next())
                .and_then(|number| number.trim().parse().ok())
        }))
    }

    pub fn auth_token(&self) -> Result<Option<String>> {
        let token: Option<String> = self
            .connection
            .query_row(
                "SELECT uriSchemeAuthenticationToken FROM TMSettings WHERE uuid = 'RhAzEf6qDxCD5PmnZVtBZR'",
                [],
                |row| row.get(0),
            )
            .optional()?;
        Ok(token.filter(|value| !value.is_empty()))
    }

    pub fn counts(&self) -> Result<DatabaseCounts> {
        let task = |kind: i64, status: i64| -> Result<i64> {
            Ok(self.connection.query_row(
                "SELECT COUNT(*) FROM TMTask WHERE type = ?1 AND status = ?2 AND COALESCE(trashed, 0) = 0",
                [kind, status],
                |row| row.get(0),
            )?)
        };
        Ok(DatabaseCounts {
            todos: task(0, 0)?,
            projects: task(1, 0)?,
            areas: self
                .connection
                .query_row("SELECT COUNT(*) FROM TMArea", [], |row| row.get(0))?,
            tags: self
                .connection
                .query_row("SELECT COUNT(*) FROM TMTag", [], |row| row.get(0))?,
        })
    }

    /// Number of to-dos and projects currently sitting in Things Trash.
    pub fn trash_count(&self) -> Result<i64> {
        Ok(self.connection.query_row(
            "SELECT COUNT(*) FROM TMTask WHERE type IN (0, 1) AND COALESCE(trashed, 0) = 1",
            [],
            |row| row.get(0),
        )?)
    }

    fn verify_schema(&mut self) -> Result<()> {
        let mut task_columns = std::collections::HashSet::new();
        for (table, required) in REQUIRED_COLUMNS {
            let mut statement = self
                .connection
                .prepare(&format!("PRAGMA table_info(\"{table}\")"))?;
            let columns = statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<rusqlite::Result<std::collections::HashSet<_>>>()?;
            if columns.is_empty() {
                return Err(Error::Unsupported(format!(
                    "Things database schema is incompatible: table {table} is missing"
                )));
            }
            let missing = required
                .iter()
                .filter(|column| !columns.contains(**column))
                .copied()
                .collect::<Vec<_>>();
            if !missing.is_empty() {
                return Err(Error::Unsupported(format!(
                    "Things database schema is incompatible: table {table} is missing required column(s) {}",
                    missing.join(", ")
                )));
            }
            if *table == "TMTask" {
                task_columns = columns;
            }
        }
        self.has_legacy_repeater = task_columns.contains("repeater");
        Ok(())
    }

    pub fn list_todos(&self, query: &ItemQuery) -> Result<Page<Todo>> {
        let mut query = query.clone();
        query.filter.item_types = vec![ItemType::Todo];
        let (where_sql, values) = self.build_task_where(&query)?;
        let total = self.count(
            &format!("SELECT COUNT(DISTINCT t.uuid) {TASK_FROM} WHERE {where_sql}"),
            &values,
        )?;
        let limit = query.limit.unwrap_or(usize::MAX);
        let task_columns = self.task_columns();
        let sql = format!(
            "SELECT DISTINCT {task_columns} {TASK_FROM} WHERE {where_sql} ORDER BY {} LIMIT {} OFFSET {}",
            order_by_clause(query.sort, query.order, query.view),
            if limit == usize::MAX {
                -1_i64
            } else {
                i64::try_from(limit).unwrap_or(i64::MAX)
            },
            i64::try_from(query.offset).unwrap_or(i64::MAX)
        );
        let mut statement = self.connection.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(values.iter()))?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            let mut todo: Todo = scan_todo(row)?.into();
            if query.include_checklist {
                todo.checklist = self.checklist(&todo.id)?;
            }
            (todo.notes, todo.notes_truncated) = note_preview(&todo.notes, query.include_notes);
            todo.tags = self.tags_for_task(&todo.id)?;
            items.push(todo);
        }
        Ok(Page::new(items, total, limit, query.offset))
    }

    pub fn list_projects(&self, query: &ItemQuery) -> Result<Page<Project>> {
        let mut query = query.clone();
        // A projects query must never admit headings or to-dos through a
        // multi-type filter such as `--type todo,project`.
        query.filter.item_types = vec![ItemType::Project];
        let (where_sql, values) = self.build_task_where(&query)?;
        let total = self.count(
            &format!("SELECT COUNT(DISTINCT t.uuid) {TASK_FROM} WHERE {where_sql}"),
            &values,
        )?;
        let limit = query.limit.unwrap_or(usize::MAX);
        let task_columns = self.task_columns();
        let sql = format!(
            "SELECT DISTINCT {task_columns} {TASK_FROM} WHERE {where_sql} ORDER BY {} LIMIT {} OFFSET {}",
            order_by_clause(query.sort, query.order, query.view),
            if limit == usize::MAX {
                -1_i64
            } else {
                i64::try_from(limit).unwrap_or(i64::MAX)
            },
            i64::try_from(query.offset).unwrap_or(i64::MAX)
        );
        let mut statement = self.connection.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(values.iter()))?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            let todo: Todo = scan_todo(row)?.into();
            let mut project = project_from_todo(todo);
            (project.notes, project.notes_truncated) =
                note_preview(&project.notes, query.include_notes);
            project.tags = self.tags_for_task(&project.id)?;
            (project.task_count, project.open_task_count) = self.project_counts(&project.id)?;
            (project.checklist_count, project.open_checklist_count) =
                self.checklist_counts(&project.id)?;
            items.push(project);
        }
        Ok(Page::new(items, total, limit, query.offset))
    }

    pub fn list_headings(&self, project_id: &str) -> Result<Vec<Heading>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT h.uuid, COALESCE(h.title, ''), COALESCE(h.status, 0),
                   COALESCE(h.trashed, 0), COALESCE(h.project, ''),
                   COALESCE(p.title, ''), COALESCE(h."index", 0)
            FROM TMTask h
            LEFT JOIN TMTask p ON p.uuid = h.project
            WHERE h.type = 2 AND h.project = ?1
            ORDER BY h."index", h.uuid
            "#,
        )?;
        let rows = statement.query_map([project_id], |row| {
            Ok(Heading {
                id: row.get(0)?,
                title: row.get(1)?,
                status: ItemStatus::from_code(row.get(2)?),
                trashed: row.get::<_, i64>(3)? != 0,
                project_id: non_empty(row.get(4)?),
                project_title: non_empty(row.get(5)?),
                index: row.get(6)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Query headings across every project with the same filters as tasks.
    pub fn list_headings_query(&self, query: &ItemQuery) -> Result<Page<Heading>> {
        let mut query = query.clone();
        query.filter.item_types = vec![ItemType::Heading];
        let (where_sql, values) = self.build_task_where(&query)?;
        let total = self.count(
            &format!("SELECT COUNT(DISTINCT t.uuid) {TASK_FROM} WHERE {where_sql}"),
            &values,
        )?;
        let limit = query.limit.unwrap_or(usize::MAX);
        let task_columns = self.task_columns();
        let sql = format!(
            "SELECT DISTINCT {task_columns} {TASK_FROM} WHERE {where_sql} ORDER BY {} LIMIT {} OFFSET {}",
            order_by_clause(query.sort, query.order, query.view),
            if limit == usize::MAX {
                -1_i64
            } else {
                i64::try_from(limit).unwrap_or(i64::MAX)
            },
            i64::try_from(query.offset).unwrap_or(i64::MAX)
        );
        let mut statement = self.connection.prepare(&sql)?;
        let mut rows = statement.query(params_from_iter(values.iter()))?;
        let mut items = Vec::new();
        while let Some(row) = rows.next()? {
            let heading = scan_todo(row)?;
            items.push(Heading {
                id: heading.id,
                title: heading.title,
                status: heading.status,
                trashed: heading.trashed,
                project_id: heading.project_id,
                project_title: heading.project_title,
                index: heading.index,
            });
        }
        Ok(Page::new(items, total, limit, query.offset))
    }

    pub fn list_areas(&self) -> Result<Vec<Area>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT a.uuid, COALESCE(a.title, ''), COALESCE(a.visible, 1),
                   COALESCE(a."index", 0),
                   (SELECT COUNT(*) FROM TMTask p WHERE p.type = 1
                      AND COALESCE(p.trashed, 0) = 0 AND p.area = a.uuid),
                   (SELECT COUNT(*) FROM TMTask t
                      LEFT JOIN TMTask p ON p.uuid = COALESCE(t.project, t.heading)
                      WHERE t.type = 0 AND t.status = 0 AND COALESCE(t.trashed, 0) = 0
                        AND COALESCE(t.area, p.area) = a.uuid)
            FROM TMArea a
            ORDER BY a."index", a.uuid
            "#,
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Area {
                id: row.get(0)?,
                title: row.get(1)?,
                visible: row.get::<_, i64>(2)? != 0,
                index: row.get(3)?,
                tags: Vec::new(),
                project_count: row.get(4)?,
                open_task_count: row.get(5)?,
            })
        })?;
        let mut items = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        for area in &mut items {
            area.tags = self.tags_for_area(&area.id)?;
        }
        Ok(items)
    }

    pub fn list_tags(&self) -> Result<Vec<Tag>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT g.uuid, COALESCE(g.title, ''), g.shortcut, g.parent,
                   COALESCE(g."index", 0),
                   (SELECT COUNT(*) FROM TMTaskTag tt WHERE tt.tags = g.uuid),
                   (SELECT COUNT(*) FROM TMAreaTag at WHERE at.tags = g.uuid)
            FROM TMTag g
            ORDER BY g."index", g.uuid
            "#,
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Tag {
                id: row.get(0)?,
                title: row.get(1)?,
                shortcut: row.get(2)?,
                parent_id: row.get(3)?,
                index: row.get(4)?,
                task_count: row.get(5)?,
                area_count: row.get(6)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Resolve a UUID, UUID prefix, exact title, or unique title substring.
    pub fn resolve(&self, reference: &str) -> Result<Item> {
        self.resolve_as(reference, None)
    }

    /// Resolve a reference within one item type.
    ///
    /// Container filters are typed: `--area Work` must never become a tag
    /// merely because a tag called Work also exists.
    pub fn resolve_typed(&self, reference: &str, item_type: ItemType) -> Result<Item> {
        self.resolve_as(reference, Some(item_type))
    }

    fn resolve_as(&self, reference: &str, item_type: Option<ItemType>) -> Result<Item> {
        let reference = reference.trim();
        if reference.is_empty() {
            return Err(Error::InvalidInput("target cannot be empty".to_string()));
        }
        if let Some(item) = self.get_by_id(reference)?
            && item_type.is_none_or(|expected| expected == item.item_type())
        {
            return Ok(item);
        }
        let (candidates, candidates_truncated) = self.find_candidates(reference, item_type)?;
        match candidates.len() {
            0 => Err(Error::NotFound(format!(
                "no {} matches {reference:?}",
                item_type.map_or("task, project, heading, area, or tag", ItemType::as_str)
            ))),
            1 => self
                .get_by_id(&candidates[0].id)?
                .ok_or_else(|| Error::Internal("resolved candidate disappeared".to_string())),
            _ => Err(Error::Ambiguous {
                message: format!("{reference:?} is ambiguous; retry with one of the returned ids"),
                candidates,
                candidates_truncated,
            }),
        }
    }

    pub fn get(&self, id: &str) -> Result<Item> {
        self.get_by_id(id)?
            .ok_or_else(|| Error::NotFound(format!("no item has id {id:?}")))
    }

    pub fn get_by_id(&self, id: &str) -> Result<Option<Item>> {
        if let Some(todo) = self.get_todo(id)? {
            return Ok(Some(Item::Todo(todo)));
        }
        if let Some(project) = self.get_project(id)? {
            return Ok(Some(Item::Project(project)));
        }
        if let Some(heading) = self.get_heading(id)? {
            return Ok(Some(Item::Heading(heading)));
        }
        if let Some(area) = self.get_area(id)? {
            return Ok(Some(Item::Area(area)));
        }
        self.get_tag(id).map(|tag| tag.map(Item::Tag))
    }

    pub fn get_tag_public(&self, id: &str) -> Result<Option<Tag>> {
        self.get_tag(id)
    }

    pub fn get_area_public(&self, id: &str) -> Result<Option<Area>> {
        self.get_area(id)
    }

    pub fn get_todo(&self, id: &str) -> Result<Option<Todo>> {
        let Some(task) = self.get_task_by_type(id, 0)? else {
            return Ok(None);
        };
        let mut todo = Todo::from(task);
        todo.tags = self.tags_for_task(&todo.id)?;
        todo.checklist = self.checklist(&todo.id)?;
        Ok(Some(todo))
    }

    pub fn get_project(&self, id: &str) -> Result<Option<Project>> {
        let Some(task) = self.get_task_by_type(id, 1)? else {
            return Ok(None);
        };
        let mut project = project_from_todo(task.into());
        project.tags = self.tags_for_task(&project.id)?;
        (project.task_count, project.open_task_count) = self.project_counts(&project.id)?;
        (project.checklist_count, project.open_checklist_count) =
            self.checklist_counts(&project.id)?;
        Ok(Some(project))
    }

    fn get_heading(&self, id: &str) -> Result<Option<Heading>> {
        self.connection
            .query_row(
                r#"
                SELECT h.uuid, COALESCE(h.title, ''), COALESCE(h.status, 0),
                       COALESCE(h.trashed, 0), COALESCE(h.project, ''),
                       COALESCE(p.title, ''), COALESCE(h."index", 0)
                FROM TMTask h LEFT JOIN TMTask p ON p.uuid = h.project
                WHERE h.type = 2 AND h.uuid = ?1
                "#,
                [id],
                |row| {
                    Ok(Heading {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        status: ItemStatus::from_code(row.get(2)?),
                        trashed: row.get::<_, i64>(3)? != 0,
                        project_id: non_empty(row.get(4)?),
                        project_title: non_empty(row.get(5)?),
                        index: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    fn get_area(&self, id: &str) -> Result<Option<Area>> {
        let mut area = self
            .connection
            .query_row(
                r#"
                SELECT a.uuid, COALESCE(a.title, ''), COALESCE(a.visible, 1),
                       COALESCE(a."index", 0),
                       (SELECT COUNT(*) FROM TMTask p WHERE p.type = 1
                          AND COALESCE(p.trashed, 0) = 0 AND p.area = a.uuid),
                       (SELECT COUNT(*) FROM TMTask t
                          LEFT JOIN TMTask p ON p.uuid = COALESCE(t.project, t.heading)
                          WHERE t.type = 0 AND t.status = 0 AND COALESCE(t.trashed, 0) = 0
                            AND COALESCE(t.area, p.area) = a.uuid)
                FROM TMArea a WHERE a.uuid = ?1
                "#,
                [id],
                |row| {
                    Ok(Area {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        visible: row.get::<_, i64>(2)? != 0,
                        index: row.get(3)?,
                        tags: Vec::new(),
                        project_count: row.get(4)?,
                        open_task_count: row.get(5)?,
                    })
                },
            )
            .optional()?;
        if let Some(area) = &mut area {
            area.tags = self.tags_for_area(&area.id)?;
        }
        Ok(area)
    }

    fn get_tag(&self, id: &str) -> Result<Option<Tag>> {
        self.connection
            .query_row(
                r#"
                SELECT g.uuid, COALESCE(g.title, ''), g.shortcut, g.parent,
                       COALESCE(g."index", 0),
                       (SELECT COUNT(*) FROM TMTaskTag tt WHERE tt.tags = g.uuid),
                       (SELECT COUNT(*) FROM TMAreaTag at WHERE at.tags = g.uuid)
                FROM TMTag g WHERE g.uuid = ?1
                "#,
                [id],
                |row| {
                    Ok(Tag {
                        id: row.get(0)?,
                        title: row.get(1)?,
                        shortcut: row.get(2)?,
                        parent_id: row.get(3)?,
                        index: row.get(4)?,
                        task_count: row.get(5)?,
                        area_count: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    fn get_task_by_type(&self, id: &str, task_type: i64) -> Result<Option<TaskRow>> {
        let task_columns = self.task_columns();
        let sql = format!("SELECT {task_columns} {TASK_FROM} WHERE t.uuid = ?1 AND t.type = ?2");
        let mut statement = self.connection.prepare(&sql)?;
        statement
            .query_row([id, &task_type.to_string()], scan_todo)
            .optional()
            .map_err(Into::into)
    }

    fn find_candidates(
        &self,
        reference: &str,
        item_type: Option<ItemType>,
    ) -> Result<(Vec<Candidate>, bool)> {
        let lowered = reference.to_lowercase();
        let mut candidates = Vec::new();

        let wants = |kind: ItemType| item_type.is_none_or(|expected| expected == kind);
        if wants(ItemType::Todo) || wants(ItemType::Project) || wants(ItemType::Heading) {
            let type_predicate = match item_type {
                Some(ItemType::Todo) => "t.type = 0",
                Some(ItemType::Project) => "t.type = 1",
                Some(ItemType::Heading) => "t.type = 2",
                _ => "t.type IN (0, 1, 2)",
            };
            let task_columns = self.task_columns();
            let sql = format!(
                r#"SELECT {task_columns} {TASK_FROM}
                 WHERE {type_predicate}
                   AND COALESCE(t.trashed, 0) = 0
                   AND (t.uuid LIKE ?1 OR lower(COALESCE(t.title, '')) = ?2
                        OR lower(COALESCE(t.title, '')) LIKE ?3)
                 ORDER BY CASE WHEN lower(COALESCE(t.title, '')) = ?2 THEN 0 ELSE 1 END,
                          t.type,
                          CASE WHEN {template} THEN 1 ELSE 0 END,
                          t."index", t.uuid
                 LIMIT {task_limit}"#,
                template = self.template_predicate("t", "p"),
                task_limit = MAX_CANDIDATES + 1,
            );
            let prefix = format!("{reference}%");
            let contains = format!("%{lowered}%");
            let mut statement = self.connection.prepare(&sql)?;
            let rows = statement.query_map((&prefix, &lowered, &contains), scan_todo)?;
            let mut task_rows = Vec::new();
            for row in rows {
                task_rows.push(row?);
            }
            let tasks_truncated = task_rows.len() > MAX_CANDIDATES;
            let mut instance_types = std::collections::HashSet::new();
            for row in &task_rows {
                if !row.repeating {
                    instance_types.insert(row.item_type);
                }
            }
            let mut rows = task_rows
                .into_iter()
                .filter(|row| !row.repeating || !instance_types.contains(&row.item_type))
                .collect::<Vec<_>>();
            rows.truncate(MAX_CANDIDATES);
            for todo in rows {
                candidates.push(Candidate {
                    id: todo.id,
                    item_type: todo.item_type,
                    title: todo.title,
                    project: todo.project_title,
                    status: Some(todo.status),
                });
            }
            candidates.truncate(MAX_CANDIDATES);
            if tasks_truncated {
                return Ok((candidates, true));
            }
        }

        if wants(ItemType::Area) {
            for (id, title) in self.simple_named_rows("TMArea", "a")? {
                if title.to_lowercase().contains(&lowered)
                    || id.to_lowercase().starts_with(&lowered)
                {
                    candidates.push(Candidate {
                        id,
                        item_type: ItemType::Area,
                        title,
                        project: None,
                        status: None,
                    });
                }
            }
        }
        if wants(ItemType::Tag) {
            for (id, title) in self.simple_named_rows("TMTag", "g")? {
                if title.to_lowercase().contains(&lowered)
                    || id.to_lowercase().starts_with(&lowered)
                {
                    candidates.push(Candidate {
                        id,
                        item_type: ItemType::Tag,
                        title,
                        project: None,
                        status: None,
                    });
                }
            }
        }
        let truncated = candidates.len() > MAX_CANDIDATES;
        candidates.truncate(MAX_CANDIDATES);
        Ok((candidates, truncated))
    }

    fn simple_named_rows(&self, table: &str, alias: &str) -> Result<Vec<(String, String)>> {
        let sql = format!("SELECT {alias}.uuid, COALESCE({alias}.title, '') FROM {table} {alias}");
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn tags_for_task(&self, id: &str) -> Result<Vec<String>> {
        self.named_join(
            "SELECT g.title FROM TMTaskTag tt JOIN TMTag g ON g.uuid = tt.tags
             WHERE tt.tasks = ?1 ORDER BY g.\"index\", g.uuid",
            id,
        )
    }

    pub fn tags_for_area(&self, id: &str) -> Result<Vec<String>> {
        self.named_join(
            "SELECT g.title FROM TMAreaTag at JOIN TMTag g ON g.uuid = at.tags
             WHERE at.areas = ?1 ORDER BY g.\"index\", g.uuid",
            id,
        )
    }

    fn named_join(&self, sql: &str, id: &str) -> Result<Vec<String>> {
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map([id], |row| row.get(0))?;
        rows.collect::<rusqlite::Result<Vec<String>>>()
            .map_err(Into::into)
    }

    pub fn checklist(&self, task_id: &str) -> Result<Vec<ChecklistItem>> {
        let mut statement = self.connection.prepare(
            r#"
            SELECT uuid, COALESCE(title, ''), COALESCE(status, 0), stopDate,
                   COALESCE("index", 0)
            FROM TMChecklistItem WHERE task = ?1 ORDER BY "index", uuid
            "#,
        )?;
        let rows = statement.query_map([task_id], |row| {
            Ok(ChecklistItem {
                id: row.get(0)?,
                title: row.get(1)?,
                status: ItemStatus::from_code(row.get(2)?),
                stop_date: crate::models::timestamp(row.get(3)?),
                index: row.get(4)?,
            })
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    /// Return the parent to-do and checklist row for a checklist item id.
    pub fn get_checklist_item(&self, id: &str) -> Result<Option<(Todo, ChecklistItem)>> {
        let parent_id: Option<String> = self
            .connection
            .query_row(
                "SELECT task FROM TMChecklistItem WHERE uuid = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(parent_id) = parent_id else {
            return Ok(None);
        };
        let Some(todo) = self.get_todo(&parent_id)? else {
            return Ok(None);
        };
        let Some(item) = todo.checklist.iter().find(|item| item.id == id).cloned() else {
            return Ok(None);
        };
        Ok(Some((todo, item)))
    }

    fn checklist_counts(&self, task_id: &str) -> Result<(i64, i64)> {
        self.connection
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(CASE WHEN COALESCE(status, 0) = 0 THEN 1 ELSE 0 END), 0)
                 FROM TMChecklistItem WHERE task = ?1",
                [task_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(Into::into)
    }

    fn project_counts(&self, project_id: &str) -> Result<(i64, i64)> {
        self.connection
            .query_row(
                r"
                SELECT COUNT(*), COALESCE(SUM(CASE WHEN COALESCE(t.status, 0) = 0 THEN 1 ELSE 0 END), 0)
                FROM TMTask t
                LEFT JOIN TMTask h ON h.uuid = t.heading
                WHERE t.type = 0 AND COALESCE(t.trashed, 0) = 0
                  AND COALESCE(t.project, h.project) = ?1
                ",
                [project_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(Into::into)
    }

    fn count(&self, sql: &str, values: &[Value]) -> Result<usize> {
        let count: i64 =
            self.connection
                .query_row(sql, params_from_iter(values.iter()), |row| row.get(0))?;
        usize::try_from(count).map_err(|_| Error::Internal("negative row count".to_string()))
    }

    fn build_task_where(&self, query: &ItemQuery) -> Result<(String, Vec<Value>)> {
        let mut clauses = vec!["1 = 1".to_string()];
        let mut values = Vec::new();
        let filter = &query.filter;

        if let Some(view) = query.view {
            apply_view(
                view,
                &mut clauses,
                &self.own_repeating_predicate("t"),
                &self.template_predicate("t", "p"),
            );
        } else if filter.trashed != Some(true) {
            // A view-less listing covers active items unless Trash was asked
            // for explicitly. Templates are not ordinary work, so they stay
            // out here just as they do in every non-template sidebar view.
            clauses.push("COALESCE(t.trashed, 0) = 0".to_string());
            let template = if filter.item_types.as_slice() == [ItemType::Project] {
                self.own_repeating_predicate("t")
            } else {
                self.template_predicate("t", "p")
            };
            clauses.push(format!("NOT {template}"));
        }
        if !filter.item_types.is_empty() {
            let codes: Vec<String> = filter
                .item_types
                .iter()
                .map(|kind| match kind {
                    ItemType::Todo => "0",
                    ItemType::Project => "1",
                    ItemType::Heading => "2",
                    ItemType::Area | ItemType::Tag | ItemType::ChecklistItem => {
                        unreachable!("area, tag, and checklist rows are not TMTask rows")
                    }
                })
                .map(str::to_string)
                .collect();
            clauses.push(format!("t.type IN ({})", codes.join(",")));
        }
        if !filter.status.is_empty() {
            let codes: Vec<String> = filter
                .status
                .iter()
                .map(|status| match status {
                    ItemStatus::Open => "0",
                    ItemStatus::Canceled => "2",
                    ItemStatus::Completed => "3",
                })
                .map(str::to_string)
                .collect();
            clauses.push(format!("COALESCE(t.status, 0) IN ({})", codes.join(",")));
        }
        if let Some(trashed) = filter.trashed {
            clauses.push(format!("COALESCE(t.trashed, 0) = {}", i64::from(trashed)));
        }
        if let Some(repeating) = filter.repeating {
            let predicate = self.repeating_predicate("t", "p");
            clauses.push(if repeating {
                predicate
            } else {
                format!("NOT {predicate}")
            });
        }
        // Things archives a project together with any to-do left open inside
        // it: closing or trashing the project hides those children from the
        // active lists even though their own status is still `open`. Only
        // open children are hidden, so items closed with their project remain
        // visible in the Logbook.
        if !filter.include_closed_project_tasks {
            clauses.push(
                "(COALESCE(t.status, 0) <> 0
                  OR (COALESCE(p.trashed, 0) = 0 AND COALESCE(p.status, 0) = 0))"
                    .to_string(),
            );
        }
        if !filter.start_buckets.is_empty() {
            let codes: Vec<String> = filter
                .start_buckets
                .iter()
                .map(|bucket| match bucket {
                    StartBucket::Inbox => "0",
                    StartBucket::Anytime => "1",
                    StartBucket::Someday => "2",
                })
                .map(str::to_string)
                .collect();
            clauses.push(format!("COALESCE(t.start, 0) IN ({})", codes.join(",")));
        }
        if let Some(project) = &filter.project {
            clauses.push("(t.project = ? OR p.uuid = ? OR p.title = ? COLLATE NOCASE)".to_string());
            values.extend([project, project, project].map(|value| Value::Text(value.clone())));
        }
        if let Some(area) = &filter.area {
            clauses.push("COALESCE(t.area, p.area) = ?".to_string());
            values.push(Value::Text(area.clone()));
        }
        if let Some(heading) = &filter.heading {
            clauses.push("(t.heading = ? OR h.title = ? COLLATE NOCASE)".to_string());
            values.extend([heading, heading].map(|value| Value::Text(value.clone())));
        }
        if let Some(text) = &filter.text {
            let contains = format!("%{}%", escape_like(text));
            clauses.push(
                r"(t.title LIKE ? ESCAPE '\' COLLATE NOCASE
                  OR t.notes LIKE ? ESCAPE '\' COLLATE NOCASE)"
                    .to_string(),
            );
            values.extend([contains.clone(), contains].map(Value::Text));
        }
        if let Some(pattern) = &filter.title_regex {
            let regex = Regex::new(pattern)
                .map_err(|error| Error::InvalidInput(format!("invalid title regex: {error}")))?;
            self.apply_regex_filter(&mut clauses, "t.title", &regex)?;
        }
        if let Some(pattern) = &filter.notes_regex {
            let regex = Regex::new(pattern)
                .map_err(|error| Error::InvalidInput(format!("invalid notes regex: {error}")))?;
            self.apply_regex_filter(&mut clauses, "t.notes", &regex)?;
        }
        if !filter.tags_all.is_empty() {
            for tag in &filter.tags_all {
                clauses.push(
                    "EXISTS (SELECT 1 FROM TMTaskTag tt JOIN TMTag g ON g.uuid = tt.tags
                     WHERE tt.tasks = t.uuid AND g.title = ? COLLATE NOCASE)"
                        .to_string(),
                );
                values.push(Value::Text(tag.clone()));
            }
        }
        if !filter.tags_any.is_empty() {
            let placeholders = vec!["?"; filter.tags_any.len()].join(",");
            clauses.push(format!(
                "EXISTS (SELECT 1 FROM TMTaskTag tt JOIN TMTag g ON g.uuid = tt.tags
                 WHERE tt.tasks = t.uuid AND g.title COLLATE NOCASE IN ({placeholders}))"
            ));
            values.extend(filter.tags_any.iter().cloned().map(Value::Text));
        }
        if !filter.tags_none.is_empty() {
            let placeholders = vec!["?"; filter.tags_none.len()].join(",");
            clauses.push(format!(
                "NOT EXISTS (SELECT 1 FROM TMTaskTag tt JOIN TMTag g ON g.uuid = tt.tags
                 WHERE tt.tasks = t.uuid AND g.title COLLATE NOCASE IN ({placeholders}))"
            ));
            values.extend(filter.tags_none.iter().cloned().map(Value::Text));
        }
        if let Some(has) = filter.has_deadline {
            clauses.push(format!(
                "t.deadline IS {} NULL",
                if has { "NOT" } else { "" }
            ));
        }
        if let Some(has) = filter.has_reminder {
            clauses.push(format!(
                "t.reminderTime IS {} NULL",
                if has { "NOT" } else { "" }
            ));
        }
        if let Some(has) = filter.has_tags {
            clauses.push(format!(
                "{} EXISTS (SELECT 1 FROM TMTaskTag tt WHERE tt.tasks = t.uuid)",
                if has { "" } else { "NOT" }
            ));
        }
        if let Some(has) = filter.has_checklist {
            clauses.push(format!(
                "{} EXISTS (SELECT 1 FROM TMChecklistItem ci WHERE ci.task = t.uuid)",
                if has { "" } else { "NOT" }
            ));
        }
        Self::apply_date_range(
            &mut clauses,
            "t.creationDate",
            filter.created_from,
            filter.created_to,
        );
        Self::apply_date_range(
            &mut clauses,
            "t.userModificationDate",
            filter.modified_from,
            filter.modified_to,
        );
        Self::apply_date_range(&mut clauses, "t.stopDate", filter.stop_from, filter.stop_to);
        Self::apply_packed_date_range(
            &mut clauses,
            "t.startDate",
            filter.start_from,
            filter.start_to,
        );
        Self::apply_packed_date_range(
            &mut clauses,
            "t.deadline",
            filter.deadline_from,
            filter.deadline_to,
        );
        if filter.overdue {
            let today = Local::now().date_naive();
            clauses.push("t.deadline IS NOT NULL AND t.deadline < ?".to_string());
            values.push(Value::Integer(encode_date(today)));
            clauses.push("COALESCE(t.status, 0) = 0".to_string());
        }
        Ok((clauses.join(" AND "), values))
    }

    fn apply_regex_filter(
        &self,
        clauses: &mut Vec<String>,
        column: &str,
        regex: &Regex,
    ) -> Result<()> {
        // SQLite has no regexp() implementation by default. Pull the small set
        // of candidate titles and let the Rust engine match exactly.
        let sql = format!("SELECT t.uuid, COALESCE({column}, '') FROM TMTask t");
        let mut statement = self.connection.prepare(&sql)?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut ids = Vec::new();
        for row in rows {
            let (id, text) = row?;
            if regex.is_match(&text) {
                ids.push(id);
            }
        }
        if ids.is_empty() {
            clauses.push("0 = 1".to_string());
        } else {
            let quoted = ids
                .iter()
                .map(|id| format!("'{}'", id.replace('\'', "''")))
                .collect::<Vec<_>>()
                .join(",");
            clauses.push(format!("t.uuid IN ({quoted})"));
        }
        Ok(())
    }

    fn apply_date_range(
        clauses: &mut Vec<String>,
        column: &str,
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
    ) {
        if let Some(from) = from {
            let seconds = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
                from.and_hms_opt(0, 0, 0).unwrap(),
                chrono::Utc,
            )
            .timestamp();
            clauses.push(format!("{column} >= {seconds}"));
        }
        if let Some(to) = to {
            let seconds = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(
                (to + chrono::Duration::days(1))
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
                chrono::Utc,
            )
            .timestamp();
            clauses.push(format!("{column} < {seconds}"));
        }
    }

    fn apply_packed_date_range(
        clauses: &mut Vec<String>,
        column: &str,
        from: Option<NaiveDate>,
        to: Option<NaiveDate>,
    ) {
        if let Some(from) = from {
            clauses.push(format!("{column} >= {}", encode_date(from)));
        }
        if let Some(to) = to {
            clauses.push(format!("{column} <= {}", encode_date(to)));
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct DatabaseCounts {
    pub todos: i64,
    pub projects: i64,
    pub areas: i64,
    pub tags: i64,
}

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone)]
struct TaskRow {
    id: String,
    title: String,
    notes: String,
    item_type: ItemType,
    status: ItemStatus,
    start: i64,
    start_bucket_code: i64,
    trashed: bool,
    start_date: Option<NaiveDate>,
    reminder: Option<String>,
    deadline: Option<NaiveDate>,
    stop_date: Option<chrono::DateTime<chrono::Utc>>,
    created_at: Option<chrono::DateTime<chrono::Utc>>,
    modified_at: Option<chrono::DateTime<chrono::Utc>>,
    index: i64,
    today_index: i64,
    today_index_reference_date: Option<NaiveDate>,
    project_id: Option<String>,
    project_title: Option<String>,
    project_trashed: bool,
    area_id: Option<String>,
    area_title: Option<String>,
    heading_id: Option<String>,
    heading_title: Option<String>,
    template_id: Option<String>,
    repeating: bool,
}

impl From<TaskRow> for Todo {
    fn from(row: TaskRow) -> Self {
        Self {
            id: row.id,
            title: row.title,
            notes: row.notes,
            notes_truncated: false,
            status: row.status,
            start_bucket: StartBucket::from_code(row.start),
            start_bucket_code: row.start,
            evening: row.start_bucket_code == 1,
            trashed: row.trashed,
            repeating: row.repeating,
            template_id: row.template_id,
            project_id: row.project_id,
            project_title: row.project_title,
            project_trashed: row.project_trashed,
            area_id: row.area_id,
            area_title: row.area_title,
            heading_id: row.heading_id,
            heading_title: row.heading_title,
            start_date: row.start_date,
            deadline: row.deadline,
            reminder: row.reminder,
            stop_date: row.stop_date,
            created_at: row.created_at,
            modified_at: row.modified_at,
            index: row.index,
            today_index: row.today_index,
            today_index_reference_date: row.today_index_reference_date,
            tags: Vec::new(),
            checklist: Vec::new(),
        }
    }
}

fn project_from_todo(todo: Todo) -> Project {
    Project {
        id: todo.id,
        title: todo.title,
        notes: todo.notes,
        notes_truncated: todo.notes_truncated,
        status: todo.status,
        start_bucket: todo.start_bucket,
        start_bucket_code: todo.start_bucket_code,
        evening: todo.evening,
        trashed: todo.trashed,
        repeating: todo.repeating,
        area_id: todo.area_id,
        area_title: todo.area_title,
        start_date: todo.start_date,
        deadline: todo.deadline,
        reminder: todo.reminder,
        stop_date: todo.stop_date,
        created_at: todo.created_at,
        modified_at: todo.modified_at,
        index: todo.index,
        today_index: todo.today_index,
        today_index_reference_date: todo.today_index_reference_date,
        tags: todo.tags,
        task_count: 0,
        open_task_count: 0,
        checklist_count: 0,
        open_checklist_count: 0,
    }
}

fn scan_todo(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRow> {
    let type_code: i64 = row.get(3)?;
    Ok(TaskRow {
        id: row.get(0)?,
        title: row.get(1)?,
        notes: row.get(2)?,
        item_type: match type_code {
            1 => ItemType::Project,
            2 => ItemType::Heading,
            _ => ItemType::Todo,
        },
        status: ItemStatus::from_code(row.get(4)?),
        start: row.get(5)?,
        start_bucket_code: row.get(6)?,
        trashed: row.get::<_, i64>(7)? != 0,
        start_date: decode_date_opt(row.get(8)?),
        reminder: decode_reminder_opt(row.get(9)?),
        deadline: decode_date_opt(row.get(10)?),
        stop_date: timestamp_opt(row.get(11)?),
        created_at: timestamp_opt(row.get(12)?),
        modified_at: timestamp_opt(row.get(13)?),
        index: row.get(14)?,
        today_index: row.get(15)?,
        today_index_reference_date: decode_date_opt(row.get(16)?),
        project_id: non_empty(row.get(17)?),
        project_title: non_empty(row.get(18)?),
        project_trashed: row.get::<_, i64>(19)? != 0,
        area_id: non_empty(row.get(20)?),
        area_title: non_empty(row.get(21)?),
        heading_id: non_empty(row.get(22)?),
        heading_title: non_empty(row.get(23)?),
        template_id: non_empty(row.get(24)?),
        repeating: row.get::<_, i64>(25)? != 0,
    })
}

fn non_empty(value: String) -> Option<String> {
    (!value.is_empty()).then_some(value)
}

fn note_preview(notes: &str, include_full: bool) -> (String, bool) {
    if include_full {
        return (notes.to_string(), false);
    }
    let mut characters = notes.chars();
    let preview = characters.by_ref().take(NOTE_PREVIEW_CHARS).collect();
    (preview, characters.next().is_some())
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', r"\\")
        .replace('%', r"\%")
        .replace('_', r"\_")
}

fn decode_date_opt(raw: Option<i64>) -> Option<NaiveDate> {
    raw.and_then(decode_date)
}

fn decode_reminder_opt(raw: Option<i64>) -> Option<String> {
    raw.and_then(crate::dates::decode_reminder)
        .map(|(hour, minute)| format!("{hour:02}:{minute:02}"))
}

fn timestamp_opt(raw: Option<f64>) -> Option<chrono::DateTime<chrono::Utc>> {
    crate::models::timestamp(raw)
}

fn apply_view(view: View, clauses: &mut Vec<String>, own_repeating: &str, template: &str) {
    match view {
        View::Inbox => {
            clauses.push("t.type = 0".to_string());
            clauses.push("COALESCE(t.status, 0) = 0".to_string());
            clauses.push("COALESCE(t.trashed, 0) = 0".to_string());
            clauses.push("COALESCE(t.start, 0) = 0".to_string());
            clauses.push("t.project IS NULL AND t.heading IS NULL".to_string());
            clauses.push(format!("NOT {template}"));
        }
        View::Today => {
            let today = encode_date(Local::now().date_naive());
            clauses.push("t.type IN (0, 1)".to_string());
            clauses.push("COALESCE(t.status, 0) = 0".to_string());
            clauses.push("COALESCE(t.trashed, 0) = 0".to_string());
            clauses.push(format!("NOT {template}"));
            clauses.push(format!(
                "((t.start IN (1, 2) AND t.startDate IS NOT NULL AND t.startDate <= {today})
                  OR (t.startDate IS NULL AND t.deadline IS NOT NULL
                      AND t.deadline <= {today} AND t.deadlineSuppressionDate IS NULL))"
            ));
        }
        View::Upcoming => {
            let tomorrow = encode_date(Local::now().date_naive() + chrono::Duration::days(1));
            clauses.push("t.type IN (0, 1)".to_string());
            clauses.push("COALESCE(t.status, 0) = 0".to_string());
            clauses.push("COALESCE(t.trashed, 0) = 0".to_string());
            clauses.push(format!("NOT {template}"));
            clauses.push(format!(
                "((t.start = 2 AND t.startDate IS NOT NULL AND t.startDate >= {tomorrow})
                  OR (t.startDate IS NULL AND t.deadline IS NOT NULL AND t.deadline >= {tomorrow}))"
            ));
        }
        View::Anytime => {
            clauses.push("t.type = 0".to_string());
            clauses.push("COALESCE(t.status, 0) = 0".to_string());
            clauses.push("COALESCE(t.trashed, 0) = 0".to_string());
            clauses.push("COALESCE(t.start, 1) = 1".to_string());
            clauses.push(format!("NOT {template}"));
        }
        View::Someday => {
            clauses.push("t.type = 0".to_string());
            clauses.push("COALESCE(t.status, 0) = 0".to_string());
            clauses.push("COALESCE(t.trashed, 0) = 0".to_string());
            clauses.push("COALESCE(t.start, 0) = 2".to_string());
            clauses.push(format!("NOT {template}"));
            // A Someday item with a concrete date is scheduled, so Things moves
            // it to Upcoming/Today. `todayIndexReferenceDate` alone is not a
            // schedule; it records when the item entered Today.
            clauses.push("t.startDate IS NULL".to_string());
        }
        View::Logbook => {
            clauses.push("COALESCE(t.status, 0) IN (2, 3)".to_string());
            clauses.push("COALESCE(t.trashed, 0) = 0".to_string());
        }
        View::Trash => clauses.push("COALESCE(t.trashed, 0) = 1".to_string()),
        View::Deadlines => {
            clauses.push("t.deadline IS NOT NULL".to_string());
            clauses.push("COALESCE(t.status, 0) = 0".to_string());
            clauses.push("COALESCE(t.trashed, 0) = 0".to_string());
            clauses.push(format!("NOT {template}"));
        }
        View::Repeating => clauses.push(own_repeating.to_string()),
        View::Projects => {
            clauses.push("t.type = 1".to_string());
            clauses.push("COALESCE(t.trashed, 0) = 0".to_string());
            clauses.push(format!("NOT {own_repeating}"));
        }
    }
}

/// Build the full `ORDER BY` body for a query.
///
/// `SortField::Index` means "the view's natural order" rather than the raw
/// `t."index"` column: Things stores a per-list position that is meaningless for
/// these date-scoped views. See [`natural_order_clause`]. Every clause ends with
/// a stable `uuid` tiebreak so pagination never reorders equal keys.
fn order_by_clause(field: SortField, order: SortOrder, view: Option<View>) -> String {
    if field == SortField::Index
        && let Some(clause) = natural_order_clause(view, order)
    {
        return clause;
    }
    let direction = order.sql();
    let column = if field == SortField::Deadline {
        "t.deadline IS NULL, t.deadline".to_string()
    } else {
        field.sql_column().to_string()
    };
    format!("{column} {direction}, t.uuid {direction}")
}

/// The order Things itself presents for a date-scoped view, or `None` for views
/// where the stored `t."index"` is already the natural order.
fn natural_order_clause(view: Option<View>, order: SortOrder) -> Option<String> {
    let direction = order.sql();
    let clause = match view? {
        View::Today => {
            // `startBucket = 1` marks This Evening, so `ASC` pushes those last.
            let reference_direction = match order {
                SortOrder::Asc => "DESC",
                SortOrder::Desc => "ASC",
            };
            format!(
                "COALESCE(t.startBucket, 0) {direction}, \
                 t.todayIndexReferenceDate {reference_direction}, \
                 COALESCE(t.todayIndex, 0) {direction}, \
                 t.uuid {direction}"
            )
        }
        View::Upcoming => format!(
            "t.startDate IS NULL {direction}, t.startDate {direction}, \
             t.deadline IS NULL {direction}, t.deadline {direction}, \
             t.uuid {direction}"
        ),
        View::Deadlines => format!(
            "t.deadline IS NULL {direction}, t.deadline {direction}, \
             t.uuid {direction}"
        ),
        View::Logbook => {
            // Things lists the most recently closed items first.
            let stop_direction = match order {
                SortOrder::Asc => "DESC",
                SortOrder::Desc => "ASC",
            };
            format!("t.stopDate {stop_direction}, t.uuid {stop_direction}")
        }
        _ => return None,
    };
    Some(clause)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_codes_match_things() {
        assert_eq!(ItemStatus::from_code(0), ItemStatus::Open);
        assert_eq!(ItemStatus::from_code(2), ItemStatus::Canceled);
        assert_eq!(ItemStatus::from_code(3), ItemStatus::Completed);
    }
}
