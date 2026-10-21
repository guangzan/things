use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::automation;
use crate::config::{Config, DatabaseSelection};
use crate::database::{Database, DatabaseCounts};
use crate::dates;
use crate::error::{Error, Result};
use crate::models::{Area, Item, ItemStatus, ItemType, Project, Tag, Todo};
use crate::query::{ItemQuery, Page};
use crate::url_scheme::{
    AddProject, AddTodo, UpdateProject, UpdateTodo, add_project as build_add_project,
    add_todo as build_add_todo, update_project as build_update_project,
    update_todo as build_update_todo,
};
use crate::verify::{
    Verification, VerifyOptions, wait_for_area, wait_for_status, wait_for_tag, wait_for_title,
};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default)]
pub struct WriteOptions {
    pub dry_run: bool,
    pub verify: bool,
    pub foreground: bool,
    pub create_missing_tags: bool,
    pub strict_tags: bool,
    pub yes: bool,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            verify: true,
            foreground: false,
            create_missing_tags: false,
            strict_tags: false,
            yes: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct WriteResult {
    pub action: String,
    #[serde(rename = "type")]
    pub item_type: ItemType,
    pub id: Option<String>,
    pub title: String,
    pub sent: bool,
    pub url: String,
    pub verification: Verification,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Per-item results for bulk creates such as JSON imports.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub created: Vec<CreatedItemResult>,
    /// Per-item results for status changes requested by a JSON import.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub updated: Vec<UpdatedItemResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<Item>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct CreatedItemResult {
    pub path: String,
    #[serde(rename = "type")]
    pub item_type: ItemType,
    pub title: String,
    pub confirmed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<crate::error::Candidate>,
}

/// Per-item result for an `operation: update` in a bulk import.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct UpdatedItemResult {
    pub path: String,
    #[serde(rename = "type")]
    pub item_type: ItemType,
    pub id: String,
    pub title: String,
    pub confirmed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wanted_status: Option<ItemStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_status: Option<ItemStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ServiceStats {
    pub database: std::path::PathBuf,
    pub database_source: String,
    pub schema_version: Option<i64>,
    pub counts: DatabaseCounts,
    pub read_only: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default)]
pub struct CreateTodo {
    pub title: String,
    pub notes: Option<String>,
    pub when: Option<String>,
    pub deadline: Option<String>,
    pub reminder: Option<String>,
    pub tags: Vec<String>,
    pub checklist: Vec<String>,
    pub project: Option<String>,
    pub area: Option<String>,
    pub heading: Option<String>,
    pub reveal: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default)]
pub struct CreateProject {
    pub title: String,
    pub notes: Option<String>,
    pub when: Option<String>,
    pub deadline: Option<String>,
    pub reminder: Option<String>,
    pub tags: Vec<String>,
    pub area: Option<String>,
    pub todos: Vec<String>,
    pub reveal: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default)]
pub struct EditItem {
    pub target: String,
    pub title: Option<String>,
    pub notes: Option<String>,
    pub prepend_notes: Option<String>,
    pub append_notes: Option<String>,
    pub when: Option<String>,
    pub clear_when: bool,
    pub deadline: Option<String>,
    pub clear_deadline: bool,
    pub reminder: Option<String>,
    pub clear_reminder: bool,
    pub tags: Option<Vec<String>>,
    pub add_tags: Vec<String>,
    pub checklist: Option<Vec<String>>,
    pub append_checklist: Vec<String>,
    pub prepend_checklist: Vec<String>,
    pub complete_checklist: Vec<String>,
    pub cancel_checklist: Vec<String>,
    pub reopen_checklist: Vec<String>,
    pub project: Option<String>,
    pub area: Option<String>,
    pub heading: Option<String>,
    pub status: Option<ItemStatus>,
    pub duplicate: bool,
    pub reveal: bool,
}

/// Shared application service used by the CLI and MCP server.
#[derive(Debug)]
pub struct Service {
    config: Config,
    database: Database,
    selection: DatabaseSelection,
}

impl Service {
    pub fn open(config: Config, explicit_database: Option<&std::path::Path>) -> Result<Self> {
        let selection = config.select_database(explicit_database)?;
        let database = Database::open(&selection.path)?;
        Ok(Self {
            config,
            database,
            selection,
        })
    }

    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    #[must_use]
    pub fn database(&self) -> &Database {
        &self.database
    }

    #[must_use]
    pub fn selection(&self) -> &DatabaseSelection {
        &self.selection
    }

    pub fn token(&self) -> Result<String> {
        if let Some(token) = self.config.token() {
            return Ok(token.to_string());
        }
        self.database.auth_token()?.ok_or(Error::AuthRequired)
    }

    pub fn counts(&self) -> Result<DatabaseCounts> {
        self.database.counts()
    }

    pub fn stats(&self) -> Result<ServiceStats> {
        Ok(ServiceStats {
            database: self.database.path().to_path_buf(),
            database_source: self.selection.source_name().to_string(),
            schema_version: self.database.schema_version()?,
            counts: self.database.counts()?,
            read_only: self.config.read_only,
        })
    }

    pub fn list(&self, query: &ItemQuery) -> Result<Page<Item>> {
        let query = self.normalize_scope_references(query)?;
        let Some(view) = query.view else {
            return self.search_with_query(&query);
        };
        let wants = |kind: ItemType| {
            query.filter.item_types.is_empty() || query.filter.item_types.contains(&kind)
        };
        if view == crate::models::View::Projects {
            if !wants(ItemType::Project) {
                return Ok(Page::new(
                    Vec::new(),
                    0,
                    query.limit.unwrap_or(0),
                    query.offset,
                ));
            }
            let page = self.database.list_projects(&query)?;
            let total = page.total;
            let limit = query.limit.unwrap_or(page.items.len());
            let offset = page.offset;
            Ok(Page::new(
                page.items.into_iter().map(Item::Project).collect(),
                total,
                limit,
                offset,
            ))
        } else {
            let mut items = Vec::new();
            let mut total = 0;
            if wants(ItemType::Todo) {
                let mut todos_query = query.clone();
                todos_query.filter.item_types = vec![ItemType::Todo];
                todos_query.limit = None;
                todos_query.offset = 0;
                let todos = self.database.list_todos(&todos_query)?;
                items.extend(todos.items.into_iter().map(Item::Todo));
                total += todos.total;
            }
            let view_holds_projects = matches!(
                view,
                crate::models::View::Logbook
                    | crate::models::View::Trash
                    | crate::models::View::Deadlines
                    | crate::models::View::Repeating
                    | crate::models::View::Today
                    | crate::models::View::Upcoming
            );
            if view_holds_projects && wants(ItemType::Project) {
                let mut project_query = query.clone();
                project_query.filter.item_types = vec![ItemType::Project];
                project_query.limit = None;
                project_query.offset = 0;
                let projects = self.database.list_projects(&project_query)?;
                total += projects.total;
                items.extend(projects.items.into_iter().map(Item::Project));
            }
            sort_items(&mut items, query.sort, query.order, query.view);
            let offset = query.offset;
            let limit = query.limit.unwrap_or(total);
            let items = items.into_iter().skip(offset).take(limit).collect();
            Ok(Page::new(items, total, limit, offset))
        }
    }

    fn search_with_query(&self, query: &ItemQuery) -> Result<Page<Item>> {
        let mut items = Vec::new();
        let wants = |kind: ItemType| {
            query.filter.item_types.is_empty() || query.filter.item_types.contains(&kind)
        };
        if wants(ItemType::Todo) {
            let mut todo_query = query.clone();
            todo_query.filter.item_types = vec![ItemType::Todo];
            todo_query.limit = None;
            items.extend(
                self.database
                    .list_todos(&todo_query)?
                    .items
                    .into_iter()
                    .map(Item::Todo),
            );
        }
        if wants(ItemType::Project) {
            let mut project_query = query.clone();
            project_query.filter.item_types = vec![ItemType::Project];
            project_query.limit = None;
            items.extend(
                self.database
                    .list_projects(&project_query)?
                    .items
                    .into_iter()
                    .map(Item::Project),
            );
        }
        // Headings belong to a project and are never part of an implicit
        // listing, so they only appear when a caller asks for the type.
        if query.filter.item_types.contains(&ItemType::Heading) {
            let mut heading_query = query.clone();
            heading_query.filter.item_types = vec![ItemType::Heading];
            heading_query.limit = None;
            items.extend(
                self.database
                    .list_headings_query(&heading_query)?
                    .items
                    .into_iter()
                    .map(Item::Heading),
            );
        }
        sort_items(&mut items, query.sort, query.order, query.view);
        let total = items.len();
        let offset = query.offset;
        let limit = query.limit.unwrap_or(total);
        let items = items.into_iter().skip(offset).take(limit).collect();
        Ok(Page::new(items, total, limit, offset))
    }

    pub fn search(&self, text: &str, query: &ItemQuery) -> Result<Page<Item>> {
        if text.trim().is_empty() {
            return Err(Error::InvalidInput(
                "search text cannot be empty".to_string(),
            ));
        }
        let mut query = self.normalize_scope_references_with(query, true)?;
        query.filter.text = Some(text.to_string());
        self.search_with_query(&query)
    }

    fn normalize_scope_references(&self, query: &ItemQuery) -> Result<ItemQuery> {
        self.normalize_scope_references_with(query, false)
    }

    /// Read filters treat an unknown tag as a non-match, matching Things' URL
    /// behavior and the existing list contract. Explicit tag mutations still
    /// use strict typed resolution.
    fn normalize_scope_references_with(
        &self,
        query: &ItemQuery,
        allow_unknown_tags: bool,
    ) -> Result<ItemQuery> {
        let mut query = query.clone();
        if let Some(reference) = query.filter.project.clone() {
            let item = self.database.resolve_typed(&reference, ItemType::Project)?;
            let Item::Project(project) = item else {
                return Err(Error::InvalidInput(format!(
                    "{reference:?} does not resolve to a project"
                )));
            };
            query.filter.project = Some(project.id);
        }
        if let Some(reference) = query.filter.area.clone() {
            let item = self.database.resolve_typed(&reference, ItemType::Area)?;
            let Item::Area(area) = item else {
                return Err(Error::InvalidInput(format!(
                    "{reference:?} does not resolve to an area"
                )));
            };
            query.filter.area = Some(area.id);
        }
        if let Some(reference) = query.filter.heading.clone() {
            let project = query.filter.project.clone();
            let (_, heading_id) = self.resolve_heading(project.as_deref(), Some(&reference))?;
            query.filter.heading = heading_id;
        }
        query.filter.tags_all =
            self.resolve_tag_references(&query.filter.tags_all, allow_unknown_tags)?;
        query.filter.tags_any =
            self.resolve_tag_references(&query.filter.tags_any, allow_unknown_tags)?;
        query.filter.tags_none =
            self.resolve_tag_references(&query.filter.tags_none, allow_unknown_tags)?;
        Ok(query)
    }

    fn resolve_tag_references(
        &self,
        references: &[String],
        allow_unknown: bool,
    ) -> Result<Vec<String>> {
        references
            .iter()
            .map(|reference| {
                match self.database.resolve_typed(reference, ItemType::Tag) {
                    // The SQL filter compares tag titles; resolving to the
                    // canonical title also makes id prefixes usable here.
                    Ok(item) => Ok(item.title().to_string()),
                    Err(Error::NotFound(_)) if allow_unknown => Ok(reference.clone()),
                    Err(error) => Err(error),
                }
            })
            .collect()
    }

    pub fn get(&self, reference: &str) -> Result<Item> {
        self.database.resolve(reference)
    }

    pub fn areas(&self) -> Result<Vec<Area>> {
        self.database.list_areas()
    }

    pub fn tags(&self) -> Result<Vec<Tag>> {
        self.database.list_tags()
    }

    pub fn headings(&self, project: &str) -> Result<Vec<crate::models::Heading>> {
        let item = self.database.resolve_typed(project, ItemType::Project)?;
        match item {
            Item::Project(project) => self.database.list_headings(&project.id),
            _ => Err(Error::InvalidInput(format!(
                "{project:?} does not resolve to a project"
            ))),
        }
    }

    pub fn create_todo(&self, input: CreateTodo, options: &WriteOptions) -> Result<WriteResult> {
        self.ensure_writable()?;
        if input.title.trim().is_empty() {
            return Err(Error::InvalidInput("title cannot be empty".to_string()));
        }
        if input.project.is_some() && input.area.is_some() {
            return Err(Error::Conflict(
                "project and area are mutually exclusive".to_string(),
            ));
        }
        let today = dates::today();
        let base_when = input
            .when
            .as_deref()
            .map(|value| dates::normalize_when(value, today));
        if let Some(value) = &base_when {
            validate_when(value, today)?;
        }
        let when = dates::with_reminder(
            base_when,
            input.reminder.as_deref(),
            Some(today.format("%Y-%m-%d").to_string()),
        )?;
        let deadline = input
            .deadline
            .as_deref()
            .map(|value| {
                dates::parse_date(value, today).map(|date| date.format("%Y-%m-%d").to_string())
            })
            .transpose()?;

        let warnings = self.prepare_tags(&input.tags, options)?;
        let (list, list_id) =
            self.resolve_destination(input.project.as_deref(), input.area.as_deref())?;
        let (heading, heading_id) =
            self.resolve_heading(input.project.as_deref(), input.heading.as_deref())?;

        let url = build_add_todo(&AddTodo {
            title: input.title.clone(),
            notes: input.notes.clone(),
            when: when.clone(),
            deadline: deadline.clone(),
            tags: input.tags.clone(),
            checklist: input.checklist.clone(),
            list,
            list_id,
            heading,
            heading_id,
            reveal: input.reveal,
        })?;

        if options.dry_run {
            return Ok(WriteResult {
                action: "add_todo".to_string(),
                item_type: ItemType::Todo,
                id: None,
                title: input.title,
                sent: false,
                url,
                verification: Verification {
                    verified: false,
                    attempts: 0,
                    elapsed_ms: 0,
                    observed: None,
                },
                warnings,
                created: Vec::new(),
                updated: Vec::new(),
                item: None,
            });
        }

        let existing_ids = if options.verify {
            Some(self.task_ids()?)
        } else {
            None
        };
        automation::open_url(&url, options.foreground)?;
        let verification = if options.verify {
            let empty = std::collections::HashSet::new();
            self.verify_created_todo(
                &input,
                when.as_deref(),
                deadline.as_deref(),
                &warnings,
                existing_ids.as_ref().unwrap_or(&empty),
            )?
        } else {
            Verification {
                verified: false,
                attempts: 0,
                elapsed_ms: 0,
                observed: None,
            }
        };
        let item = if verification.verified {
            verification
                .observed
                .as_deref()
                .and_then(|id| self.database.get(id).ok())
        } else {
            None
        };
        Ok(WriteResult {
            action: "add_todo".to_string(),
            item_type: ItemType::Todo,
            id: item.as_ref().map(|item| item.id().to_string()),
            title: input.title,
            sent: true,
            url,
            verification,
            warnings,
            created: Vec::new(),
            updated: Vec::new(),
            item,
        })
    }

    pub fn create_project(
        &self,
        input: CreateProject,
        options: &WriteOptions,
    ) -> Result<WriteResult> {
        self.ensure_writable()?;
        if input.title.trim().is_empty() {
            return Err(Error::InvalidInput("title cannot be empty".to_string()));
        }
        let today = dates::today();
        let base_when = input
            .when
            .as_deref()
            .map(|value| dates::normalize_when(value, today));
        if let Some(value) = &base_when {
            validate_when(value, today)?;
        }
        let when = dates::with_reminder(
            base_when,
            input.reminder.as_deref(),
            Some(today.format("%Y-%m-%d").to_string()),
        )?;
        let deadline = input
            .deadline
            .as_deref()
            .map(|value| {
                dates::parse_date(value, today).map(|date| date.format("%Y-%m-%d").to_string())
            })
            .transpose()?;
        let warnings = self.prepare_tags(&input.tags, options)?;
        let area = if let Some(reference) = &input.area {
            let item = self.database.resolve_typed(reference, ItemType::Area)?;
            match item {
                Item::Area(area) => Some(area.id),
                _ => {
                    return Err(Error::InvalidInput(format!(
                        "{reference:?} does not resolve to an area"
                    )));
                }
            }
        } else {
            None
        };
        let url = build_add_project(&AddProject {
            title: input.title.clone(),
            notes: input.notes.clone(),
            when: when.clone(),
            deadline: deadline.clone(),
            tags: input.tags.clone(),
            area: None,
            area_id: area,
            todos: input.todos.clone(),
            reveal: input.reveal,
        })?;
        if options.dry_run {
            return Ok(WriteResult {
                action: "add_project".to_string(),
                item_type: ItemType::Project,
                id: None,
                title: input.title,
                sent: false,
                url,
                verification: Verification {
                    verified: false,
                    attempts: 0,
                    elapsed_ms: 0,
                    observed: None,
                },
                warnings,
                created: Vec::new(),
                updated: Vec::new(),
                item: None,
            });
        }
        let existing_ids = if options.verify {
            Some(self.task_ids()?)
        } else {
            None
        };
        automation::open_url(&url, options.foreground)?;
        let verification = if options.verify {
            let empty = std::collections::HashSet::new();
            self.verify_created_project(
                &input,
                when.as_deref(),
                deadline.as_deref(),
                &warnings,
                existing_ids.as_ref().unwrap_or(&empty),
            )?
        } else {
            Verification {
                verified: false,
                attempts: 0,
                elapsed_ms: 0,
                observed: None,
            }
        };
        let item = verification
            .observed
            .as_deref()
            .and_then(|id| self.database.get(id).ok());
        Ok(WriteResult {
            action: "add_project".to_string(),
            item_type: ItemType::Project,
            id: item.as_ref().map(|item| item.id().to_string()),
            title: input.title,
            sent: true,
            url,
            verification,
            warnings,
            created: Vec::new(),
            updated: Vec::new(),
            item,
        })
    }

    pub fn edit(&self, input: &EditItem, options: &WriteOptions) -> Result<WriteResult> {
        self.ensure_writable()?;
        let item = self.database.resolve(&input.target)?;
        let item_type = item.item_type();
        if !matches!(item_type, ItemType::Todo | ItemType::Project) {
            return Err(Error::InvalidInput(format!(
                "{:?} resolves to a {}; only todos and projects can be edited",
                input.target,
                item_type.as_str()
            )));
        }
        if item
            .status()
            .is_some_and(|status| status != ItemStatus::Open)
            && input.status.is_none()
        {
            return Err(Error::Conflict(format!(
                "{:?} is closed; reopen it with `things reopen` before editing",
                item.title()
            )));
        }
        if item_type == ItemType::Project
            && input.status.is_some()
            && item.status() != input.status
            && !options.yes
        {
            return Err(Error::Conflict(
                "changing a project's status changes every task inside it; pass --yes to confirm"
                    .to_string(),
            ));
        }
        let is_repeating = match &item {
            Item::Todo(todo) => todo.repeating,
            Item::Project(project) => project.repeating,
            _ => false,
        };
        if is_repeating
            && (input.when.is_some()
                || input.clear_when
                || input.deadline.is_some()
                || input.clear_deadline
                || input.reminder.is_some()
                || input.status.is_some())
        {
            return Err(Error::Unsupported(
                "Things does not allow when, reminder, deadline, or status changes on repeating items; edit the repeat in Things"
                    .to_string(),
            ));
        }
        if input.reminder.is_some() && input.clear_reminder {
            return Err(Error::Conflict(
                "reminder and clear_reminder are mutually exclusive".to_string(),
            ));
        }
        if input.clear_reminder {
            return Err(Error::Unsupported(
                "the Things URL scheme cannot clear a reminder without changing its schedule; use the Things app"
                    .to_string(),
            ));
        }
        if input.reminder.is_some() && input.clear_when {
            return Err(Error::Conflict(
                "reminder and clear_when are mutually exclusive".to_string(),
            ));
        }
        if input.when.is_some() && input.clear_when {
            return Err(Error::Conflict(
                "when and clear_when are mutually exclusive".to_string(),
            ));
        }
        if input.deadline.is_some() && input.clear_deadline {
            return Err(Error::Conflict(
                "deadline and clear_deadline are mutually exclusive".to_string(),
            ));
        }
        if input.project.is_some() && input.area.is_some() {
            return Err(Error::Conflict(
                "project and area are mutually exclusive".to_string(),
            ));
        }
        if let Some(status) = input.status
            && item.status() == Some(status)
        {
            return Err(Error::Conflict(format!(
                "{:?} already has status {}",
                item.title(),
                status.as_str()
            )));
        }
        if item_type == ItemType::Project {
            if input.project.is_some() {
                return Err(Error::InvalidInput(
                    "project edit cannot move a project into another project; use --area for area placement"
                        .to_string(),
                ));
            }
            if input.heading.is_some() {
                return Err(Error::InvalidInput(
                    "heading is only valid when editing a to-do".to_string(),
                ));
            }
            if input.checklist.is_some()
                || !input.append_checklist.is_empty()
                || !input.prepend_checklist.is_empty()
                || !input.complete_checklist.is_empty()
                || !input.cancel_checklist.is_empty()
                || !input.reopen_checklist.is_empty()
            {
                return Err(Error::InvalidInput(
                    "checklist changes are only valid when editing a to-do".to_string(),
                ));
            }
            if input.duplicate {
                return Err(Error::Unsupported(
                    "duplicating a project is not supported by the Things URL scheme".to_string(),
                ));
            }
        }
        let has_checklist_status_change = !input.complete_checklist.is_empty()
            || !input.cancel_checklist.is_empty()
            || !input.reopen_checklist.is_empty();
        if has_checklist_status_change {
            if item_type != ItemType::Todo {
                return Err(Error::InvalidInput(
                    "checklist status changes are only valid for to-dos".to_string(),
                ));
            }
            if input.title.is_some()
                || input.notes.is_some()
                || input.prepend_notes.is_some()
                || input.append_notes.is_some()
                || input.when.is_some()
                || input.clear_when
                || input.deadline.is_some()
                || input.clear_deadline
                || input.reminder.is_some()
                || input.clear_reminder
                || input.tags.is_some()
                || !input.add_tags.is_empty()
                || input.checklist.is_some()
                || !input.append_checklist.is_empty()
                || !input.prepend_checklist.is_empty()
                || input.project.is_some()
                || input.area.is_some()
                || input.heading.is_some()
                || input.status.is_some()
                || input.duplicate
            {
                return Err(Error::Conflict(
                    "checklist status changes must be sent separately from other edits".to_string(),
                ));
            }
            return self.edit_checklist_status(&item, input, options);
        }
        let today = dates::today();
        let when = if input.clear_when {
            Some("anytime".to_string())
        } else {
            input
                .when
                .as_deref()
                .map(|value| dates::normalize_when(value, today))
        };
        if let Some(value) = &when {
            validate_when(value, today)?;
        }
        let when = dates::with_reminder(when, input.reminder.as_deref(), scheduled_when(&item))?;
        if let Some(value) = &when {
            validate_when(value, today)?;
        }
        let deadline = if input.clear_deadline {
            Some(String::new())
        } else {
            input
                .deadline
                .as_deref()
                .map(|value| {
                    dates::parse_date(value, today).map(|date| date.format("%Y-%m-%d").to_string())
                })
                .transpose()?
        };

        let mut tags = input.tags.as_ref().map(|tags| tags.join(","));
        let add_tags = (!input.add_tags.is_empty()).then(|| input.add_tags.join(","));
        let requested_tags = input
            .tags
            .clone()
            .unwrap_or_default()
            .into_iter()
            .chain(input.add_tags.iter().cloned())
            .collect::<Vec<_>>();
        let warnings = self.prepare_tags(&requested_tags, options)?;
        let (list, list_id) = if let Some(project) = &input.project {
            let item = self.database.resolve_typed(project, ItemType::Project)?;
            match item {
                Item::Project(project) => (None, Some(project.id)),
                _ => {
                    return Err(Error::InvalidInput(format!(
                        "{project:?} does not resolve to a project"
                    )));
                }
            }
        } else if let Some(area) = &input.area {
            let item = self.database.resolve_typed(area, ItemType::Area)?;
            match item {
                Item::Area(area) => (None, Some(area.id)),
                _ => {
                    return Err(Error::InvalidInput(format!(
                        "{area:?} does not resolve to an area"
                    )));
                }
            }
        } else {
            (None, None)
        };
        let (heading, heading_id) =
            self.resolve_heading(input.project.as_deref(), input.heading.as_deref())?;

        let (url, action) = match item_type {
            ItemType::Todo => {
                let update = UpdateTodo {
                    id: item.id().to_string(),
                    title: input.title.clone(),
                    notes: input.notes.clone(),
                    prepend_notes: input.prepend_notes.clone(),
                    append_notes: input.append_notes.clone(),
                    when: when.clone(),
                    deadline: deadline.clone(),
                    tags: tags.take(),
                    add_tags,
                    checklist: input.checklist.as_ref().map(|items| items.join("\n")),
                    prepend_checklist: (!input.prepend_checklist.is_empty())
                        .then(|| input.prepend_checklist.join("\n")),
                    append_checklist: (!input.append_checklist.is_empty())
                        .then(|| input.append_checklist.join("\n")),
                    list,
                    list_id,
                    heading,
                    heading_id,
                    completed: input.status == Some(ItemStatus::Completed),
                    canceled: input.status == Some(ItemStatus::Canceled),
                    duplicate: input.duplicate,
                    reveal: input.reveal,
                };
                (build_update_todo(&update, &self.token()?)?, "edit_todo")
            }
            ItemType::Project => {
                let update = UpdateProject {
                    id: item.id().to_string(),
                    title: input.title.clone(),
                    notes: input.notes.clone(),
                    prepend_notes: input.prepend_notes.clone(),
                    append_notes: input.append_notes.clone(),
                    when: when.clone(),
                    deadline: deadline.clone(),
                    tags: tags.take(),
                    add_tags,
                    area: None,
                    area_id: list_id,
                    completed: input.status == Some(ItemStatus::Completed),
                    canceled: input.status == Some(ItemStatus::Canceled),
                    reveal: input.reveal,
                };
                (
                    build_update_project(&update, &self.token()?)?,
                    "edit_project",
                )
            }
            _ => unreachable!(),
        };

        if options.dry_run {
            return Ok(WriteResult {
                action: action.to_string(),
                item_type,
                id: Some(item.id().to_string()),
                title: item.title().to_string(),
                sent: false,
                url,
                verification: Verification {
                    verified: false,
                    attempts: 0,
                    elapsed_ms: 0,
                    observed: None,
                },
                warnings,
                created: Vec::new(),
                updated: Vec::new(),
                item: Some(item),
            });
        }
        let baseline = item.clone();
        automation::open_url(&url, options.foreground)?;
        let verification = if options.verify {
            self.verify_edit(
                &baseline,
                input,
                when.as_deref(),
                deadline.as_deref(),
                &warnings,
            )?
        } else {
            Verification {
                verified: false,
                attempts: 0,
                elapsed_ms: 0,
                observed: None,
            }
        };
        Ok(WriteResult {
            action: action.to_string(),
            item_type,
            id: Some(item.id().to_string()),
            title: item.title().to_string(),
            sent: true,
            url,
            verification,
            warnings,
            created: Vec::new(),
            updated: Vec::new(),
            item: self.database.get(item.id()).ok(),
        })
    }

    pub fn set_status(
        &self,
        target: &str,
        status: ItemStatus,
        options: &WriteOptions,
    ) -> Result<WriteResult> {
        self.ensure_writable()?;
        let item = self.database.resolve(target)?;
        let item_type = item.item_type();
        if !matches!(item_type, ItemType::Todo | ItemType::Project) {
            return Err(Error::InvalidInput(format!(
                "{target:?} resolves to a {}; only todos and projects have status",
                item_type.as_str()
            )));
        }
        if matches!(&item, Item::Project(_)) && !options.yes {
            return Err(Error::Conflict(
                "closing or reopening a project changes every task inside it; pass --yes to confirm"
                    .to_string(),
            ));
        }
        let repeating = match &item {
            Item::Todo(todo) => todo.repeating,
            Item::Project(project) => project.repeating,
            _ => false,
        };
        if repeating {
            return Err(Error::Unsupported(
                "Things does not allow status changes on repeating items; complete the generated occurrence in Things"
                    .to_string(),
            ));
        }
        let url = format!("applescript://status/{}/{}", item.id(), status.as_str());
        if options.dry_run {
            return Ok(WriteResult {
                action: format!("set_status_{}", status.as_str()),
                item_type,
                id: Some(item.id().to_string()),
                title: item.title().to_string(),
                sent: false,
                url,
                verification: Verification {
                    verified: false,
                    attempts: 0,
                    elapsed_ms: 0,
                    observed: None,
                },
                warnings: Vec::new(),
                created: Vec::new(),
                updated: Vec::new(),
                item: Some(item),
            });
        }
        automation::set_status(item.id(), item_type.as_str(), status.as_str())?;
        let verification = if options.verify {
            wait_for_status(&self.database, item.id(), status, self.verify_options())?
        } else {
            Verification {
                verified: false,
                attempts: 0,
                elapsed_ms: 0,
                observed: None,
            }
        };
        Ok(WriteResult {
            action: format!("set_status_{}", status.as_str()),
            item_type,
            id: Some(item.id().to_string()),
            title: item.title().to_string(),
            sent: true,
            url,
            verification,
            warnings: Vec::new(),
            created: Vec::new(),
            updated: Vec::new(),
            item: self.database.get(item.id()).ok(),
        })
    }

    pub fn delete(&self, target: &str, options: &WriteOptions) -> Result<WriteResult> {
        self.ensure_writable()?;
        let item = self.database.resolve(target)?;
        let item_type = item.item_type();
        if !matches!(item_type, ItemType::Todo | ItemType::Project) {
            return Err(Error::Unsupported(format!(
                "deleting a {} is not supported; only todos and projects can be moved to Trash",
                item_type.as_str()
            )));
        }
        // Things' AppleScript dictionary has no way to address an item that
        // already sits in the Trash (`to do id` fails with -1728), so the
        // delete would surface as an opaque `app_unavailable`. Report the
        // real state instead.
        let already_trashed = match &item {
            Item::Todo(todo) => todo.trashed,
            Item::Project(project) => project.trashed,
            _ => false,
        };
        if already_trashed {
            return Err(Error::Conflict(format!(
                "{target:?} is already in Things Trash; use `things empty-trash --yes` to remove it permanently"
            )));
        }
        if !options.yes {
            return Err(Error::Conflict(
                "delete moves the item to Things Trash; pass --yes to confirm".to_string(),
            ));
        }
        if options.dry_run {
            return Ok(WriteResult {
                action: "delete".to_string(),
                item_type,
                id: Some(item.id().to_string()),
                title: item.title().to_string(),
                sent: false,
                url: format!("applescript://delete/{}", item.id()),
                verification: Verification {
                    verified: false,
                    attempts: 0,
                    elapsed_ms: 0,
                    observed: None,
                },
                warnings: Vec::new(),
                created: Vec::new(),
                updated: Vec::new(),
                item: Some(item),
            });
        }
        automation::delete_item(item.id(), item_type.as_str())?;
        let verification = if options.verify {
            crate::verify::wait_for(self.verify_options(), || {
                let item = self.database.get(item.id())?;
                let trashed = match &item {
                    Item::Todo(todo) => todo.trashed,
                    Item::Project(project) => project.trashed,
                    _ => false,
                };
                Ok(trashed.then(|| "trashed".to_string()))
            })?
        } else {
            Verification {
                verified: false,
                attempts: 0,
                elapsed_ms: 0,
                observed: None,
            }
        };
        Ok(WriteResult {
            action: "delete".to_string(),
            item_type,
            id: Some(item.id().to_string()),
            title: item.title().to_string(),
            sent: true,
            url: format!("applescript://delete/{}", item.id()),
            verification,
            warnings: Vec::new(),
            created: Vec::new(),
            updated: Vec::new(),
            item: self.database.get(item.id()).ok(),
        })
    }

    pub fn add_tag(&self, name: &str, options: &WriteOptions) -> Result<Tag> {
        self.ensure_writable()?;
        if name.trim().is_empty() {
            return Err(Error::InvalidInput("tag name cannot be empty".to_string()));
        }
        if name.contains(',') {
            return Err(Error::InvalidInput(
                "Things tag names cannot contain a comma".to_string(),
            ));
        }
        if let Some(existing) = self
            .database
            .list_tags()?
            .into_iter()
            .find(|tag| tag.title.eq_ignore_ascii_case(name))
        {
            return Ok(existing);
        }
        if options.dry_run {
            return Ok(Tag {
                id: "dry-run".to_string(),
                title: name.to_string(),
                shortcut: None,
                parent_id: None,
                index: 0,
                task_count: 0,
                area_count: 0,
            });
        }
        let id = automation::create_tag(name)?;
        if options.verify {
            let verification = wait_for_tag(&self.database, name, true, self.verify_options())?;
            if !verification.verified {
                return Err(Error::VerificationFailed(format!(
                    "tag {name:?} was created but did not appear in the database"
                )));
            }
            return self
                .database
                .list_tags()?
                .into_iter()
                .find(|tag| tag.title.eq_ignore_ascii_case(name))
                .ok_or_else(|| Error::Internal("created tag disappeared".to_string()));
        }
        // Verification is disabled, so describe the tag from what AppleScript
        // returned instead of waiting for the database to catch up.
        Ok(Tag {
            id,
            title: name.to_string(),
            shortcut: None,
            parent_id: None,
            index: 0,
            task_count: 0,
            area_count: 0,
        })
    }

    pub fn rename_tag(&self, target: &str, new_name: &str, options: &WriteOptions) -> Result<Tag> {
        self.ensure_writable()?;
        let item = self.database.resolve_typed(target, ItemType::Tag)?;
        let Item::Tag(tag) = item else {
            return Err(Error::InvalidInput(format!(
                "{target:?} does not resolve to a tag"
            )));
        };
        if new_name.trim().is_empty() || new_name.contains(',') {
            return Err(Error::InvalidInput(
                "tag name cannot be empty or contain a comma".to_string(),
            ));
        }
        if options.dry_run {
            return Ok(Tag {
                title: new_name.to_string(),
                ..tag
            });
        }
        automation::rename_tag(&tag.id, new_name)?;
        if options.verify {
            let verification =
                wait_for_title(&self.database, &tag.id, new_name, self.verify_options())?;
            if !verification.verified {
                return Err(Error::VerificationFailed(format!(
                    "tag rename to {new_name:?} was sent but not observed"
                )));
            }
        }
        Ok(self
            .database
            .get_tag_public(&tag.id)?
            .unwrap_or_else(|| Tag {
                title: new_name.to_string(),
                ..tag
            }))
    }

    pub fn delete_tag(&self, target: &str, options: &WriteOptions) -> Result<Tag> {
        self.ensure_writable()?;
        let item = self.database.resolve_typed(target, ItemType::Tag)?;
        let Item::Tag(tag) = item else {
            return Err(Error::InvalidInput(format!(
                "{target:?} does not resolve to a tag"
            )));
        };
        if !options.yes {
            return Err(Error::Conflict(
                "deleting a tag removes it from every task and area; pass --yes to confirm"
                    .to_string(),
            ));
        }
        if !options.dry_run {
            automation::delete_tag(&tag.id)?;
            if options.verify {
                let verification =
                    wait_for_tag(&self.database, &tag.title, false, self.verify_options())?;
                if !verification.verified {
                    return Err(Error::VerificationFailed(format!(
                        "tag {:?} was deleted but is still present",
                        tag.title
                    )));
                }
            }
        }
        Ok(tag)
    }

    pub fn add_area(&self, name: &str, options: &WriteOptions) -> Result<Area> {
        self.ensure_writable()?;
        if name.trim().is_empty() {
            return Err(Error::InvalidInput("area name cannot be empty".to_string()));
        }
        if let Some(existing) = self
            .database
            .list_areas()?
            .into_iter()
            .find(|area| area.title.eq_ignore_ascii_case(name))
        {
            return Ok(existing);
        }
        if options.dry_run {
            return Ok(Area {
                id: "dry-run".to_string(),
                title: name.to_string(),
                visible: true,
                index: 0,
                tags: Vec::new(),
                project_count: 0,
                open_task_count: 0,
            });
        }
        let id = automation::create_area(name)?;
        if options.verify {
            let verification = wait_for_area(&self.database, name, true, self.verify_options())?;
            if !verification.verified {
                return Err(Error::VerificationFailed(format!(
                    "area {name:?} was created but did not appear in the database"
                )));
            }
            return self
                .database
                .list_areas()?
                .into_iter()
                .find(|area| area.title.eq_ignore_ascii_case(name))
                .ok_or_else(|| Error::Internal("created area disappeared".to_string()));
        }
        Ok(Area {
            id,
            title: name.to_string(),
            visible: true,
            index: 0,
            tags: Vec::new(),
            project_count: 0,
            open_task_count: 0,
        })
    }

    pub fn rename_area(
        &self,
        target: &str,
        new_name: &str,
        options: &WriteOptions,
    ) -> Result<Area> {
        self.ensure_writable()?;
        let item = self.database.resolve_typed(target, ItemType::Area)?;
        let Item::Area(area) = item else {
            return Err(Error::InvalidInput(format!(
                "{target:?} does not resolve to an area"
            )));
        };
        if new_name.trim().is_empty() {
            return Err(Error::InvalidInput("area name cannot be empty".to_string()));
        }
        if options.dry_run {
            return Ok(Area {
                title: new_name.to_string(),
                ..area
            });
        }
        automation::rename_area(&area.id, new_name)?;
        if options.verify {
            let verification =
                wait_for_title(&self.database, &area.id, new_name, self.verify_options())?;
            if !verification.verified {
                return Err(Error::VerificationFailed(format!(
                    "area rename to {new_name:?} was sent but not observed"
                )));
            }
        }
        Ok(self
            .database
            .get_area_public(&area.id)?
            .unwrap_or_else(|| Area {
                title: new_name.to_string(),
                ..area
            }))
    }

    pub fn delete_area(&self, target: &str, options: &WriteOptions) -> Result<Area> {
        self.ensure_writable()?;
        let item = self.database.resolve_typed(target, ItemType::Area)?;
        let Item::Area(area) = item else {
            return Err(Error::InvalidInput(format!(
                "{target:?} does not resolve to an area"
            )));
        };
        if !options.yes {
            return Err(Error::Conflict(
                "deleting an area moves it to Things Trash; pass --yes to confirm".to_string(),
            ));
        }
        if !options.dry_run {
            automation::delete_area(&area.id)?;
            if options.verify {
                let verification =
                    wait_for_area(&self.database, &area.title, false, self.verify_options())?;
                if !verification.verified {
                    return Err(Error::VerificationFailed(format!(
                        "area {:?} was deleted but is still present",
                        area.title
                    )));
                }
            }
        }
        Ok(area)
    }

    /// Permanently empty the Things Trash. This cannot be undone.
    pub fn empty_trash(&self, options: &WriteOptions) -> Result<WriteResult> {
        self.ensure_writable()?;
        if !options.yes {
            return Err(Error::Conflict(
                "emptying the Trash permanently deletes every item in it; pass --yes to confirm"
                    .to_string(),
            ));
        }
        let trashed = self.database.trash_count()?;
        let url = "applescript://empty-trash".to_string();
        if options.dry_run {
            return Ok(WriteResult {
                action: "empty_trash".to_string(),
                item_type: ItemType::Todo,
                id: None,
                title: "Things Trash".to_string(),
                sent: false,
                url,
                verification: Verification {
                    verified: false,
                    attempts: 0,
                    elapsed_ms: 0,
                    observed: None,
                },
                warnings: vec![format!(
                    "{trashed} trashed item(s) would be permanently deleted"
                )],
                created: Vec::new(),
                updated: Vec::new(),
                item: None,
            });
        }
        automation::empty_trash()?;
        let verification = if options.verify {
            crate::verify::wait_for(self.verify_options(), || {
                let remaining = self.database.trash_count()?;
                Ok((remaining == 0).then(|| "empty".to_string()))
            })?
        } else {
            Verification {
                verified: false,
                attempts: 0,
                elapsed_ms: 0,
                observed: None,
            }
        };
        Ok(WriteResult {
            action: "empty_trash".to_string(),
            item_type: ItemType::Todo,
            id: None,
            title: "Things Trash".to_string(),
            sent: true,
            url,
            verification,
            warnings: Vec::new(),
            created: Vec::new(),
            updated: Vec::new(),
            item: None,
        })
    }

    pub fn import(&self, payload: &str, options: &WriteOptions) -> Result<WriteResult> {
        self.ensure_writable()?;
        reject_duplicate_json_keys(payload)?;
        reject_invalid_import_attributes(payload)?;
        let value: serde_json::Value = serde_json::from_str(payload)
            .map_err(|error| Error::InvalidInput(format!("invalid import JSON: {error}")))?;
        let Value::Array(items) = &value else {
            return Err(Error::InvalidInput(
                "Things JSON import must be an array".to_string(),
            ));
        };
        let mut create_count = 0usize;
        for (index, item) in items.iter().enumerate() {
            validate_import_item(
                item,
                &format!("[{index}]"),
                ImportSlot::Top,
                &mut create_count,
            )?;
        }
        if create_count > 200 {
            return Err(Error::InvalidInput(format!(
                "import creates {create_count} items; split it into batches of at most 200"
            )));
        }
        let import_updates = self.prepare_import_updates(items)?;
        // The `json` command requires an auth token. Resolve it through the
        // service so a token stored only in TMSettings still works.
        let token = self.token()?;
        let url = crate::url_scheme::import_json(payload, Some(&token), options.foreground)?;
        if options.dry_run {
            return Ok(WriteResult {
                action: "import".to_string(),
                item_type: ItemType::Todo,
                id: None,
                title: "JSON import".to_string(),
                sent: false,
                url,
                verification: Verification {
                    verified: false,
                    attempts: 0,
                    elapsed_ms: 0,
                    observed: None,
                },
                warnings: Vec::new(),
                created: unverified_import_results(items, "dry-run"),
                updated: unverified_updated_results(&import_updates, "dry-run"),
                item: None,
            });
        }
        let baseline = if options.verify {
            Some(self.task_ids()?)
        } else {
            None
        };
        automation::open_url(&url, options.foreground)?;
        let (verification, created, updated_items) = if options.verify {
            let (mut verification, created) =
                self.verify_import(items, baseline.as_ref().expect("baseline"))?;
            let (status_verification, updated_items) =
                self.verify_import_updates(&import_updates)?;
            match status_verification {
                Some(status_verification) => {
                    verification = merge_import_verification(verification, status_verification);
                }
                None if !updated_items.is_empty() => {
                    // No update requested a status change, so there is no
                    // field read-back for those updates. Never let an empty
                    // created-item check report the whole import as verified.
                    verification.verified = false;
                    verification.observed = Some(format!(
                        "{} update item(s) sent; update fields were not verified",
                        updated_items.len()
                    ));
                }
                None => {}
            }
            (verification, created, updated_items)
        } else {
            (
                Verification {
                    verified: false,
                    attempts: 0,
                    elapsed_ms: 0,
                    observed: None,
                },
                unverified_import_results(items, "no-verify"),
                unverified_updated_results(&import_updates, "no-verify"),
            )
        };
        Ok(WriteResult {
            action: "import".to_string(),
            item_type: ItemType::Todo,
            id: None,
            title: "JSON import".to_string(),
            sent: true,
            url,
            verification,
            warnings: Vec::new(),
            created,
            updated: updated_items,
            item: None,
        })
    }

    pub fn reveal(
        &self,
        target: Option<&str>,
        view: Option<&str>,
        background: bool,
    ) -> Result<String> {
        let url = if let Some(target) = target {
            let item = self.database.resolve(target)?;
            crate::url_scheme::show(Some(item.id()), None, None)?
        } else {
            let id = view.ok_or_else(|| {
                Error::InvalidInput("open requires a target or built-in view".to_string())
            })?;
            crate::url_scheme::show(Some(id), None, None)?
        };
        automation::open_url(&url, !background)?;
        Ok(url)
    }

    pub fn repeat_editor(&self, target: &str) -> Result<String> {
        let item = self.database.resolve(target)?;
        if !matches!(item.item_type(), ItemType::Todo | ItemType::Project) {
            return Err(Error::InvalidInput(
                "only a todo or project can have a repeat rule".to_string(),
            ));
        }
        let url = crate::url_scheme::show(Some(item.id()), None, None)?;
        automation::open_url(&url, true)?;
        Ok(url)
    }

    fn verify_options(&self) -> VerifyOptions {
        VerifyOptions {
            timeout: Duration::from_millis(self.config.write.verify_timeout_ms),
            poll: Duration::from_millis(self.config.write.verify_poll_ms.max(50)),
        }
    }

    fn ensure_writable(&self) -> Result<()> {
        if self.config.read_only {
            Err(Error::Unsupported(
                "write operations are disabled by read_only configuration".to_string(),
            ))
        } else {
            Ok(())
        }
    }

    fn prepare_tags(&self, tags: &[String], options: &WriteOptions) -> Result<Vec<String>> {
        if tags.is_empty() {
            return Ok(Vec::new());
        }
        let existing = self.database.list_tags()?;
        let mut warnings = Vec::new();
        for tag in tags {
            if tag.contains(',') {
                return Err(Error::InvalidInput(format!(
                    "tag {tag:?} contains a comma, which Things uses as a separator"
                )));
            }
            if existing
                .iter()
                .any(|value| value.title.eq_ignore_ascii_case(tag))
            {
                continue;
            }
            if options.dry_run {
                warnings.push(format!(
                    "tag {tag:?} does not exist; it would be created when this write is sent"
                ));
            } else if options.create_missing_tags || self.config.write.create_missing_tags {
                automation::create_tag(tag)?;
            } else if options.strict_tags {
                return Err(Error::InvalidInput(format!(
                    "tag {tag:?} does not exist in Things; create it first or pass --create-tags"
                )));
            } else {
                warnings.push(format!(
                    "tag {tag:?} does not exist and Things will ignore it"
                ));
            }
        }
        Ok(warnings)
    }

    fn edit_checklist_status(
        &self,
        item: &Item,
        input: &EditItem,
        options: &WriteOptions,
    ) -> Result<WriteResult> {
        let Item::Todo(todo) = item else {
            return Err(Error::InvalidInput(
                "checklist status changes are only valid for to-dos".to_string(),
            ));
        };
        if todo.checklist.is_empty() {
            return Err(Error::Conflict(format!(
                "{:?} has no checklist items",
                todo.title
            )));
        }
        let requested = input
            .complete_checklist
            .iter()
            .map(|title| (title, ItemStatus::Completed))
            .chain(
                input
                    .cancel_checklist
                    .iter()
                    .map(|title| (title, ItemStatus::Canceled)),
            )
            .chain(
                input
                    .reopen_checklist
                    .iter()
                    .map(|title| (title, ItemStatus::Open)),
            )
            .collect::<Vec<_>>();
        let mut seen = std::collections::HashSet::new();
        for (title, _) in &requested {
            if !seen.insert((*title).clone()) {
                return Err(Error::Conflict(format!(
                    "checklist item {title:?} was requested more than once"
                )));
            }
            let matches = todo
                .checklist
                .iter()
                .filter(|item| item.title == title.as_str())
                .collect::<Vec<_>>();
            match matches.as_slice() {
                [] => {
                    return Err(Error::NotFound(format!(
                        "todo {:?} has no checklist item named {title:?}",
                        todo.title
                    )));
                }
                [_] => {}
                _ => {
                    return Err(Error::Ambiguous {
                        message: format!(
                            "todo {:?} has multiple checklist items named {title:?}",
                            todo.title
                        ),
                        candidates: Vec::new(),
                        candidates_truncated: false,
                    });
                }
            }
        }
        let requested = requested
            .into_iter()
            .map(|(title, status)| (title.as_str(), status))
            .collect::<std::collections::HashMap<_, _>>();
        let checklist = todo
            .checklist
            .iter()
            .map(|item| {
                let status = requested
                    .get(item.title.as_str())
                    .copied()
                    .unwrap_or(item.status);
                serde_json::json!({
                    "type": "checklist-item",
                    "attributes": {
                        "title": item.title,
                        "completed": status == ItemStatus::Completed,
                        "canceled": status == ItemStatus::Canceled,
                    }
                })
            })
            .collect::<Vec<_>>();
        let data = serde_json::to_string(&vec![serde_json::json!({
            "type": "to-do",
            "operation": "update",
            "id": todo.id,
            "attributes": {"checklist-items": checklist}
        })])
        .map_err(|error| Error::Internal(error.to_string()))?;
        let url = crate::url_scheme::import_json(&data, Some(&self.token()?), options.foreground)?;
        if options.dry_run {
            return Ok(WriteResult {
                action: "edit_checklist_status".to_string(),
                item_type: ItemType::Todo,
                id: Some(todo.id.clone()),
                title: todo.title.clone(),
                sent: false,
                url,
                verification: Verification {
                    verified: false,
                    attempts: 0,
                    elapsed_ms: 0,
                    observed: None,
                },
                warnings: Vec::new(),
                created: Vec::new(),
                updated: Vec::new(),
                item: Some(item.clone()),
            });
        }
        let baseline = item.clone();
        automation::open_url(&url, options.foreground)?;
        let verification = if options.verify {
            self.verify_edit(&baseline, input, None, None, &[])?
        } else {
            Verification {
                verified: false,
                attempts: 0,
                elapsed_ms: 0,
                observed: None,
            }
        };
        Ok(WriteResult {
            action: "edit_checklist_status".to_string(),
            item_type: ItemType::Todo,
            id: Some(todo.id.clone()),
            title: todo.title.clone(),
            sent: true,
            url,
            verification,
            warnings: Vec::new(),
            created: Vec::new(),
            updated: Vec::new(),
            item: self.database.get(&todo.id).ok(),
        })
    }

    fn resolve_destination(
        &self,
        project: Option<&str>,
        area: Option<&str>,
    ) -> Result<(Option<String>, Option<String>)> {
        if let Some(project) = project {
            let item = self.database.resolve_typed(project, ItemType::Project)?;
            return match item {
                Item::Project(project) => Ok((None, Some(project.id))),
                _ => Err(Error::InvalidInput(format!(
                    "{project:?} does not resolve to a project"
                ))),
            };
        }
        if let Some(area) = area {
            let item = self.database.resolve_typed(area, ItemType::Area)?;
            return match item {
                Item::Area(area) => Ok((None, Some(area.id))),
                _ => Err(Error::InvalidInput(format!(
                    "{area:?} does not resolve to an area"
                ))),
            };
        }
        Ok((None, None))
    }

    fn resolve_heading(
        &self,
        project: Option<&str>,
        heading: Option<&str>,
    ) -> Result<(Option<String>, Option<String>)> {
        let Some(heading) = heading else {
            return Ok((None, None));
        };
        let Some(project) = project else {
            return Err(Error::InvalidInput(
                "a heading requires a project".to_string(),
            ));
        };
        let item = self.database.resolve_typed(project, ItemType::Project)?;
        let Item::Project(project) = item else {
            return Err(Error::InvalidInput(format!(
                "{project:?} does not resolve to a project"
            )));
        };
        let headings = self.database.list_headings(&project.id)?;
        let lowered = heading.to_lowercase();
        let matched = headings
            .into_iter()
            .filter(|value| {
                value.title.eq_ignore_ascii_case(heading)
                    || value.id.eq_ignore_ascii_case(heading)
                    || (heading.len() >= 4 && value.id.to_lowercase().starts_with(&lowered))
            })
            .collect::<Vec<_>>();
        match matched.as_slice() {
            [] => Err(Error::NotFound(format!(
                "project {:?} has no heading named {heading:?}",
                project.title
            ))),
            [one] => Ok((None, Some(one.id.clone()))),
            _ => Err(Error::Ambiguous {
                message: format!(
                    "project {:?} has multiple headings named {heading:?}",
                    project.title
                ),
                candidates: matched
                    .into_iter()
                    .map(|value| crate::error::Candidate {
                        id: value.id,
                        item_type: ItemType::Heading,
                        title: value.title,
                        project: Some(project.title.clone()),
                        status: Some(value.status),
                    })
                    .collect(),
                candidates_truncated: false,
            }),
        }
    }

    fn task_ids(&self) -> Result<std::collections::HashSet<String>> {
        Ok(self
            .database
            .list_todos(&ItemQuery {
                include_notes: false,
                include_checklist: false,
                ..ItemQuery::default()
            })?
            .items
            .into_iter()
            .map(|todo| todo.id)
            .chain(
                self.database
                    .list_projects(&ItemQuery {
                        include_notes: false,
                        include_checklist: false,
                        ..ItemQuery::default()
                    })?
                    .items
                    .into_iter()
                    .map(|project| project.id),
            )
            .collect())
    }

    fn prepare_import_updates(&self, payload: &[Value]) -> Result<Vec<ImportUpdateItem>> {
        let mut updates = Vec::new();
        collect_import_updates(payload, &mut updates)?;
        for update in &mut updates {
            if update.item_type == ItemType::ChecklistItem {
                let Some((todo, item)) = self.database.get_checklist_item(&update.id)? else {
                    update.title = String::new();
                    continue;
                };
                validate_import_update_context(&Item::Todo(todo), update)?;
                update.title = item.title;
                continue;
            }
            let Some(item) = self.database.get_by_id(&update.id)? else {
                update.title = String::new();
                continue;
            };
            if item.item_type() != update.item_type {
                return Err(Error::InvalidInput(format!(
                    "{}: type {:?} does not match the item at id {:?}",
                    update.path, update.item_type, update.id
                )));
            }
            let repeating = match &item {
                Item::Todo(todo) => todo.repeating,
                Item::Project(project) => project.repeating,
                _ => false,
            };
            if repeating
                && (update.wanted_status.is_some()
                    || update.attributes_have_restricted_repeat_fields)
            {
                return Err(Error::Unsupported(format!(
                    "{}: Things does not allow when, deadline, or status changes on repeating items",
                    update.path
                )));
            }
            validate_import_update_destination(&self.database, &item, update)?;
            update.title = item.title().to_string();
        }
        // Checklist rows only expose status to this read-back path. Reject
        // other fields instead of sending them and reporting a false success.
        for update in &updates {
            if update.item_type == ItemType::ChecklistItem && update.has_non_status_fields {
                return Err(Error::Unsupported(format!(
                    "{}: checklist-item updates can only change completed or canceled; other fields have no read-back",
                    update.path
                )));
            }
        }
        Ok(updates)
    }

    fn verify_import_updates(
        &self,
        updates: &[ImportUpdateItem],
    ) -> Result<(Option<Verification>, Vec<UpdatedItemResult>)> {
        let status_updates = updates
            .iter()
            .filter(|update| update.wanted_status.is_some())
            .collect::<Vec<_>>();
        if status_updates.is_empty() {
            let results = updates
                .iter()
                .map(|update| UpdatedItemResult {
                    path: update.path.clone(),
                    item_type: update.item_type,
                    id: update.id.clone(),
                    title: update.title.clone(),
                    confirmed: false,
                    wanted_status: update.wanted_status,
                    observed_status: None,
                    reason: if update.title.is_empty() {
                        Some("target-not-found".to_string())
                    } else {
                        Some("not-verified".to_string())
                    },
                })
                .collect();
            return Ok((None, results));
        }
        let verification = crate::verify::wait_for(self.verify_options(), || {
            let mut all_confirmed = true;
            for update in &status_updates {
                let wanted = update.wanted_status.expect("filtered status update");
                let Some(observed) = self.import_update_status(update)? else {
                    all_confirmed = false;
                    break;
                };
                if observed != wanted || !update.other_fields_verified() {
                    all_confirmed = false;
                    break;
                }
            }
            Ok(all_confirmed.then(|| {
                format!(
                    "{}/{} import status changes confirmed",
                    status_updates.len(),
                    status_updates.len()
                )
            }))
        })?;
        let results = updates
            .iter()
            .map(|update| {
                let observed = self.import_update_status(update).ok().flatten();
                let (confirmed, reason) = classify_import_update(update, observed);
                UpdatedItemResult {
                    path: update.path.clone(),
                    item_type: update.item_type,
                    id: update.id.clone(),
                    title: update.title.clone(),
                    confirmed,
                    wanted_status: update.wanted_status,
                    observed_status: observed,
                    reason: reason.map(str::to_string),
                }
            })
            .collect::<Vec<_>>();
        Ok((
            Some(finalize_import_update_verification(verification, &results)),
            results,
        ))
    }

    fn import_update_status(&self, update: &ImportUpdateItem) -> Result<Option<ItemStatus>> {
        if update.item_type == ItemType::ChecklistItem {
            return Ok(self
                .database
                .get_checklist_item(&update.id)?
                .map(|(_, item)| item.status));
        }
        Ok(self
            .database
            .get_by_id(&update.id)?
            .and_then(|item| item.status()))
    }

    fn verify_import(
        &self,
        payload: &[Value],
        baseline: &std::collections::HashSet<String>,
    ) -> Result<(Verification, Vec<CreatedItemResult>)> {
        let expected = import_created_items(payload)?;
        let mut verification = crate::verify::wait_for(self.verify_options(), || {
            let created = self.import_created_results(&expected, baseline)?;
            let confirmed = created.iter().filter(|item| item.confirmed).count();
            if created
                .iter()
                .any(|item| item.reason.as_deref() == Some("ambiguous"))
            {
                return Ok(Some(format!(
                    "{confirmed}/{} created items confirmed; ambiguous assignment cannot improve by polling",
                    expected.len()
                )));
            }
            Ok((confirmed == expected.len())
                .then(|| format!("{confirmed}/{} created items confirmed", expected.len())))
        })?;
        let created = self.import_created_results(&expected, baseline)?;
        if created.iter().any(|item| !item.confirmed) {
            verification.verified = false;
            verification.observed = None;
        }
        Ok((verification, created))
    }

    fn import_created_results(
        &self,
        expected: &[ImportCreatedItem],
        baseline: &std::collections::HashSet<String>,
    ) -> Result<Vec<CreatedItemResult>> {
        if expected.is_empty() {
            return Ok(Vec::new());
        }

        // Read each type once. The previous implementation scanned the whole
        // database for every expected item, which made a 200-item import
        // quadratic and could mix in unrelated same-title rows.
        let todos = self
            .database
            .list_todos(&ItemQuery {
                filter: crate::query::Filter {
                    item_types: vec![ItemType::Todo],
                    ..crate::query::Filter::default()
                },
                sort: crate::query::SortField::Created,
                order: crate::query::SortOrder::Asc,
                include_notes: false,
                include_checklist: false,
                ..ItemQuery::default()
            })?
            .items
            .into_iter()
            .filter(|todo| !baseline.contains(&todo.id))
            .map(Item::Todo)
            .collect::<Vec<_>>();
        let projects = self
            .database
            .list_projects(&ItemQuery {
                filter: crate::query::Filter {
                    item_types: vec![ItemType::Project],
                    ..crate::query::Filter::default()
                },
                sort: crate::query::SortField::Created,
                order: crate::query::SortOrder::Asc,
                include_notes: false,
                include_checklist: false,
                ..ItemQuery::default()
            })?
            .items
            .into_iter()
            .filter(|project| !baseline.contains(&project.id))
            .map(Item::Project)
            .collect::<Vec<_>>();

        let mut available: std::collections::HashMap<(ItemType, String), Vec<Item>> =
            std::collections::HashMap::new();
        for item in todos.into_iter().chain(projects) {
            available
                .entry((item.item_type(), item.title().to_string()))
                .or_default()
                .push(item);
        }
        for candidates in available.values_mut() {
            candidates.sort_by(|left, right| {
                left.created_at()
                    .cmp(&right.created_at())
                    .then_with(|| left.id().cmp(right.id()))
            });
        }
        let mut results = pair_import_created_results(expected, available);
        let mut positions = std::collections::HashMap::new();
        for (index, item) in expected.iter().enumerate() {
            positions.insert(item.path.clone(), index);
        }
        results.sort_by_key(|result| positions.get(&result.path).copied().unwrap_or(usize::MAX));
        Ok(results)
    }

    fn verify_created_todo(
        &self,
        input: &CreateTodo,
        when: Option<&str>,
        deadline: Option<&str>,
        warnings: &[String],
        existing_ids: &std::collections::HashSet<String>,
    ) -> Result<Verification> {
        let title = input.title.clone();
        let project = input.project.clone();
        crate::verify::wait_for(self.verify_options(), || {
            let matches = self.database.list_todos(&ItemQuery {
                filter: crate::query::Filter {
                    text: Some(title.clone()),
                    item_types: vec![ItemType::Todo],
                    project: project.clone(),
                    ..crate::query::Filter::default()
                },
                sort: crate::query::SortField::Created,
                order: crate::query::SortOrder::Desc,
                ..ItemQuery::default()
            })?;
            let found = matches.items.into_iter().find(|todo| {
                todo.title == title
                    && !existing_ids.contains(&todo.id)
                    && todo_matches_request(todo, when, deadline, &input.tags, warnings)
            });
            Ok(found.map(|todo| todo.id))
        })
    }

    fn verify_created_project(
        &self,
        input: &CreateProject,
        when: Option<&str>,
        deadline: Option<&str>,
        warnings: &[String],
        existing_ids: &std::collections::HashSet<String>,
    ) -> Result<Verification> {
        let title = input.title.clone();
        crate::verify::wait_for(self.verify_options(), || {
            let matches = self.database.list_projects(&ItemQuery {
                filter: crate::query::Filter {
                    item_types: vec![ItemType::Project],
                    ..crate::query::Filter::default()
                },
                sort: crate::query::SortField::Created,
                order: crate::query::SortOrder::Desc,
                ..ItemQuery::default()
            })?;
            let found = matches.items.into_iter().find(|project| {
                project.title == title
                    && !existing_ids.contains(&project.id)
                    && project_matches_request(project, when, deadline, &input.tags, warnings)
            });
            Ok(found.map(|project| project.id))
        })
    }

    fn verify_edit(
        &self,
        baseline: &Item,
        input: &EditItem,
        when: Option<&str>,
        deadline: Option<&str>,
        warnings: &[String],
    ) -> Result<Verification> {
        crate::verify::wait_for(self.verify_options(), || {
            let current = self.database.get(baseline.id())?;
            let changed = match (baseline, &current) {
                (Item::Todo(before), Item::Todo(after)) => {
                    todo_changed(before, after, input, when, deadline, warnings)
                }
                (Item::Project(before), Item::Project(after)) => {
                    project_changed(before, after, input, when, deadline, warnings)
                }
                _ => false,
            };
            Ok(changed.then(|| current.id().to_string()))
        })
    }
}

fn pair_import_created_results(
    expected: &[ImportCreatedItem],
    mut available: std::collections::HashMap<(ItemType, String), Vec<Item>>,
) -> Vec<CreatedItemResult> {
    // Group expected items by type and exact title, then by destination.
    // Take each type/title bucket from `available` exactly once: filtering
    // it per destination and removing it repeatedly would make the first
    // destination consume rows that belong to a later one.
    let mut groups: std::collections::HashMap<
        (ItemType, String),
        std::collections::HashMap<ImportDestination, Vec<&ImportCreatedItem>>,
    > = std::collections::HashMap::new();
    for item in expected {
        groups
            .entry((item.item_type, item.title.clone()))
            .or_default()
            .entry(item.destination.clone())
            .or_default()
            .push(item);
    }
    let mut results = Vec::with_capacity(expected.len());
    for ((item_type, title), destinations) in groups {
        let candidates = available
            .remove(&(item_type, title.clone()))
            .unwrap_or_default();
        // A loose destination (no project/area/heading) can claim every
        // row of that type and title. A specific destination can claim
        // only rows filed there. Compute the overlap once so a row that
        // fits two destinations cannot be confirmed twice.
        let fits = destinations
            .keys()
            .map(|destination| {
                let mut matched = candidates
                    .iter()
                    .filter(|candidate| {
                        import_candidate_matches_destination(candidate, destination)
                    })
                    .collect::<Vec<_>>();
                matched.sort_by(|left, right| {
                    left.created_at()
                        .cmp(&right.created_at())
                        .then_with(|| left.id().cmp(right.id()))
                });
                (destination.clone(), matched)
            })
            .collect::<std::collections::HashMap<_, _>>();
        let shared = fits
            .iter()
            .filter(|(destination, rows)| {
                fits.iter().any(|(other, other_rows)| {
                    other != *destination
                        && rows
                            .iter()
                            .any(|row| other_rows.iter().any(|other| other.id() == row.id()))
                })
            })
            .map(|(destination, _)| destination.clone())
            .collect::<std::collections::HashSet<_>>();
        for (destination, mut wanted) in destinations {
            let candidates = fits.get(&destination).map_or(&[][..], Vec::as_slice);
            let ambiguous = candidates.len() > wanted.len()
                || (candidates.len() > 1 && !distinct_created_at(candidates))
                || shared.contains(&destination);
            if ambiguous {
                for item in wanted {
                    results.push(CreatedItemResult {
                        path: item.path.clone(),
                        item_type,
                        title: title.clone(),
                        confirmed: false,
                        id: None,
                        reason: Some("ambiguous".to_string()),
                        candidates: candidates
                            .iter()
                            .map(|candidate| crate::error::Candidate {
                                id: candidate.id().to_string(),
                                item_type: candidate.item_type(),
                                title: candidate.title().to_string(),
                                project: candidate
                                    .project_title()
                                    .map(std::string::ToString::to_string),
                                status: candidate.status(),
                            })
                            .collect(),
                    });
                }
                continue;
            }
            let paired = wanted.len().min(candidates.len());
            for (item, candidate) in wanted.drain(..paired).zip(candidates.iter()) {
                results.push(CreatedItemResult {
                    path: item.path.clone(),
                    item_type,
                    title: title.clone(),
                    confirmed: true,
                    id: Some(candidate.id().to_string()),
                    reason: None,
                    candidates: Vec::new(),
                });
            }
            for item in wanted {
                results.push(CreatedItemResult {
                    path: item.path.clone(),
                    item_type,
                    title: title.clone(),
                    confirmed: false,
                    id: None,
                    reason: Some("not-found".to_string()),
                    candidates: candidates
                        .iter()
                        .map(|candidate| crate::error::Candidate {
                            id: candidate.id().to_string(),
                            item_type: candidate.item_type(),
                            title: candidate.title().to_string(),
                            project: candidate
                                .project_title()
                                .map(std::string::ToString::to_string),
                            status: candidate.status(),
                        })
                        .collect(),
                });
            }
        }
    }
    // Keep the public result in payload order, even though grouping above
    // visits by type/title.
    let positions = expected
        .iter()
        .enumerate()
        .map(|(index, item)| (item.path.clone(), index))
        .collect::<std::collections::HashMap<_, _>>();
    results.sort_by_key(|result| positions.get(&result.path).copied().unwrap_or(usize::MAX));
    results
}

fn distinct_created_at(items: &[&Item]) -> bool {
    items.windows(2).all(|pair| {
        let (Some(left), Some(right)) = (pair[0].created_at(), pair[1].created_at()) else {
            return false;
        };
        right > left
    })
}

fn import_candidate_matches_destination(candidate: &Item, destination: &ImportDestination) -> bool {
    match (candidate, destination) {
        (
            Item::Todo(todo),
            ImportDestination {
                project,
                area,
                heading,
                ..
            },
        ) => {
            if let Some(project) = project {
                let matches_project = todo
                    .project_id
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case(project))
                    || todo
                        .project_title
                        .as_deref()
                        .is_some_and(|value| value.eq_ignore_ascii_case(project));
                if !matches_project {
                    return false;
                }
            }
            if let Some(area) = area {
                let matches_area = todo
                    .area_id
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case(area))
                    || todo
                        .area_title
                        .as_deref()
                        .is_some_and(|value| value.eq_ignore_ascii_case(area));
                if !matches_area {
                    return false;
                }
            }
            if let Some(heading) = heading {
                let matches_heading = todo
                    .heading_id
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case(heading))
                    || todo
                        .heading_title
                        .as_deref()
                        .is_some_and(|value| value.eq_ignore_ascii_case(heading));
                if !matches_heading {
                    return false;
                }
            }
            true
        }
        (Item::Project(project), ImportDestination { area, .. }) => {
            area.as_ref().is_none_or(|area| {
                project
                    .area_id
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case(area))
                    || project
                        .area_title
                        .as_deref()
                        .is_some_and(|value| value.eq_ignore_ascii_case(area))
            })
        }
        _ => true,
    }
}

fn validate_import_update_destination(
    database: &Database,
    item: &Item,
    update: &ImportUpdateItem,
) -> Result<()> {
    validate_import_update_context(item, update)?;
    let destination = update.destination.as_ref();
    let Some(destination) = destination else {
        return Ok(());
    };
    match update.item_type {
        ItemType::Todo => {
            if update.has_area_id || destination.area.is_some() {
                return Err(Error::Conflict(format!(
                    "{}: area/area-id is not a valid destination for a to-do; use list/list-id",
                    update.path
                )));
            }
        }
        ItemType::Project => {
            if update.has_list_id || destination.project.is_some() {
                return Err(Error::Conflict(format!(
                    "{}: list/list-id is not a valid destination for a project; use area/area-id",
                    update.path
                )));
            }
            if update.has_heading_id || destination.heading.is_some() {
                return Err(Error::Conflict(format!(
                    "{}: heading/heading-id is not a valid destination for a project",
                    update.path
                )));
            }
        }
        ItemType::Heading | ItemType::ChecklistItem | ItemType::Area | ItemType::Tag => {
            return Err(Error::Conflict(format!(
                "{}: {:?} updates do not accept list, area, or heading destination fields",
                update.path, update.item_type
            )));
        }
    }
    if update.has_list_id {
        validate_import_uuid_reference(
            database,
            destination.project.as_deref().unwrap_or_default(),
            &[ItemType::Project, ItemType::Area],
            update,
            "list-id",
        )?;
    } else if let Some(reference) = destination.project.as_deref() {
        validate_import_list_reference(database, reference, update)?;
    }
    if update.has_area_id {
        validate_import_uuid_reference(
            database,
            destination.area.as_deref().unwrap_or_default(),
            &[ItemType::Area],
            update,
            "area-id",
        )?;
    } else if let Some(reference) = destination.area.as_deref() {
        validate_import_destination_reference(
            database,
            Some(reference),
            ItemType::Area,
            update,
            "area",
        )?;
    }
    if update.has_heading_id {
        validate_import_uuid_reference(
            database,
            destination.heading.as_deref().unwrap_or_default(),
            &[ItemType::Heading],
            update,
            "heading-id",
        )?;
    } else if let Some(reference) = destination.heading.as_deref() {
        validate_import_destination_reference(
            database,
            Some(reference),
            ItemType::Heading,
            update,
            "heading",
        )?;
    }
    Ok(())
}

fn validate_import_list_reference(
    database: &Database,
    reference: &str,
    update: &ImportUpdateItem,
) -> Result<()> {
    for item_type in [ItemType::Project, ItemType::Area] {
        match database.resolve_typed(reference, item_type) {
            Ok(_) => return Ok(()),
            Err(Error::NotFound(_)) => {}
            Err(Error::Ambiguous { .. }) => {
                return Err(Error::Conflict(format!(
                    "{}: update destination list {reference:?} is ambiguous; use a project or area id",
                    update.path
                )));
            }
            Err(error) => return Err(error),
        }
    }
    Err(Error::Conflict(format!(
        "{}: update destination list {reference:?} does not identify a unique project or area; fix it before sending",
        update.path
    )))
}

fn validate_import_uuid_reference(
    database: &Database,
    reference: &str,
    item_types: &[ItemType],
    update: &ImportUpdateItem,
    field: &str,
) -> Result<()> {
    if !is_things_uuid(reference) {
        return Err(Error::Conflict(format!(
            "{}: update destination {field} {reference:?} must be a 21-22 character base62 Things id or a hyphenated UUID",
            update.path
        )));
    }
    match database.get_by_id(reference)? {
        Some(item) if item_types.contains(&item.item_type()) => Ok(()),
        _ => Err(Error::Conflict(format!(
            "{}: update destination {field} {reference:?} does not identify an existing {}; fix it before sending",
            update.path,
            item_types
                .iter()
                .map(|item_type| item_type.as_str())
                .collect::<Vec<_>>()
                .join(" or ")
        ))),
    }
}

fn validate_import_destination_reference(
    database: &Database,
    reference: Option<&str>,
    item_type: ItemType,
    update: &ImportUpdateItem,
    field: &str,
) -> Result<()> {
    let Some(reference) = reference else {
        return Ok(());
    };
    match database.resolve_typed(reference, item_type) {
        Ok(_) => Ok(()),
        Err(Error::NotFound(_) | Error::Ambiguous { .. }) => Err(Error::Conflict(format!(
            "{}: update destination {field} {reference:?} does not identify a unique {}; fix it before sending",
            update.path,
            item_type.as_str()
        ))),
        Err(error) => Err(error),
    }
}

fn validate_import_update_context(item: &Item, update: &ImportUpdateItem) -> Result<()> {
    let Some(parent_id) = update.parent_id.as_deref() else {
        return Ok(());
    };
    match item {
        Item::Todo(todo) => {
            if update.item_type == ItemType::ChecklistItem && todo.id != parent_id {
                return Err(Error::Conflict(format!(
                    "{}: checklist item {:?} does not belong to to-do {:?}",
                    update.path, update.id, parent_id
                )));
            }
            if update.item_type == ItemType::Todo && todo.project_id.as_deref() != Some(parent_id) {
                return Err(Error::Conflict(format!(
                    "{}: to-do {:?} does not belong to project {:?}",
                    update.path, update.id, parent_id
                )));
            }
            Ok(())
        }
        Item::Project(_) if update.item_type == ItemType::Todo => Err(Error::Conflict(format!(
            "{}: to-do {:?} does not belong to project {:?}",
            update.path, update.id, parent_id
        ))),
        _ => Ok(()),
    }
}

fn is_things_uuid(value: &str) -> bool {
    match value.len() {
        21 | 22 => value.bytes().all(|byte| byte.is_ascii_alphanumeric()),
        36 => value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        }),
        _ => false,
    }
}

fn validate_when(value: &str, today: chrono::NaiveDate) -> Result<()> {
    match value {
        "anytime" | "someday" | "evening" => Ok(()),
        _ => dates::parse_when_time(value, today).map(|_| ()),
    }
}

fn scheduled_when(item: &Item) -> Option<String> {
    match item {
        Item::Todo(todo) => todo
            .start_date
            .map(|date| date.format("%Y-%m-%d").to_string())
            .or_else(|| todo.evening.then(|| "evening".to_string())),
        Item::Project(project) => project
            .start_date
            .map(|date| date.format("%Y-%m-%d").to_string())
            .or_else(|| project.evening.then(|| "evening".to_string())),
        _ => None,
    }
}

fn sort_items(
    items: &mut [Item],
    field: crate::query::SortField,
    order: crate::query::SortOrder,
    view: Option<crate::models::View>,
) {
    let compare_strings = |left: &str, right: &str| {
        left.to_lowercase()
            .cmp(&right.to_lowercase())
            .then_with(|| left.cmp(right))
    };
    items.sort_by(|left, right| {
        let ordering = match field {
            // `index` is the view's natural order, mirroring the SQL clause.
            crate::query::SortField::Index => match view {
                Some(crate::models::View::Today) => {
                    today_sort_key(left).cmp(&today_sort_key(right))
                }
                Some(crate::models::View::Upcoming) => {
                    upcoming_sort_key(left).cmp(&upcoming_sort_key(right))
                }
                Some(crate::models::View::Deadlines) => {
                    deadline_sort_key(left).cmp(&deadline_sort_key(right))
                }
                Some(crate::models::View::Logbook) => {
                    std::cmp::Reverse(left.stop_date()).cmp(&std::cmp::Reverse(right.stop_date()))
                }
                _ => left.index().cmp(&right.index()),
            },
            crate::query::SortField::Title => compare_strings(left.title(), right.title()),
            crate::query::SortField::Created => left.created_at().cmp(&right.created_at()),
            crate::query::SortField::Modified => left.modified_at().cmp(&right.modified_at()),
            crate::query::SortField::StartDate => left.start_date().cmp(&right.start_date()),
            crate::query::SortField::Deadline => left.deadline().cmp(&right.deadline()),
            crate::query::SortField::StopDate => left.stop_date().cmp(&right.stop_date()),
        };
        let ordering = ordering.then_with(|| left.id().cmp(right.id()));
        if order == crate::query::SortOrder::Desc {
            ordering.reverse()
        } else {
            ordering
        }
    });
}

/// Key mirroring the SQL Today ordering: regular items before This Evening,
/// then the most recently added day, then `todayIndex`.
fn today_sort_key(item: &Item) -> (bool, std::cmp::Reverse<Option<chrono::NaiveDate>>, i64) {
    (
        item.evening(),
        std::cmp::Reverse(item.today_index_reference_date()),
        item.today_index(),
    )
}

/// Key mirroring the SQL Upcoming ordering: soonest start date first, then the
/// soonest deadline, ranking undated items last.
fn upcoming_sort_key(
    item: &Item,
) -> (
    bool,
    Option<chrono::NaiveDate>,
    bool,
    Option<chrono::NaiveDate>,
) {
    let start = item.start_date();
    let deadline = item.deadline();
    (start.is_none(), start, deadline.is_none(), deadline)
}

/// Key mirroring the SQL Deadlines ordering: soonest deadline first.
fn deadline_sort_key(item: &Item) -> (bool, Option<chrono::NaiveDate>) {
    let deadline = item.deadline();
    (deadline.is_none(), deadline)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportSlot {
    Top,
    ProjectItems,
    Checklist,
}

#[derive(Debug, Clone)]
struct ImportCreatedItem {
    path: String,
    item_type: ItemType,
    title: String,
    destination: ImportDestination,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
struct ImportDestination {
    project: Option<String>,
    area: Option<String>,
    heading: Option<String>,
}

#[derive(Debug, Clone)]
struct ImportUpdateItem {
    path: String,
    item_type: ItemType,
    id: String,
    title: String,
    parent_id: Option<String>,
    wanted_status: Option<ItemStatus>,
    attributes_have_restricted_repeat_fields: bool,
    has_non_status_fields: bool,
    destination: Option<ImportDestination>,
    has_list_id: bool,
    has_area_id: bool,
    has_heading_id: bool,
}

impl ImportUpdateItem {
    fn other_fields_verified(&self) -> bool {
        // Import verification can prove a requested status by reading the
        // row back. Other attributes have no field-level read-back here, so
        // never let a mixed update claim that the whole update was verified.
        !self.has_non_status_fields
    }
}

fn classify_import_update(
    update: &ImportUpdateItem,
    observed: Option<ItemStatus>,
) -> (bool, Option<&'static str>) {
    if update.title.is_empty() {
        return (false, Some("target-not-found"));
    }
    match update.wanted_status {
        Some(wanted) if observed == Some(wanted) && update.other_fields_verified() => (true, None),
        Some(wanted) if observed == Some(wanted) => (false, Some("fields-not-verified")),
        Some(_) => (false, Some("status-not-applied")),
        None => (false, Some("not-verified")),
    }
}

fn finalize_import_update_verification(
    mut verification: Verification,
    results: &[UpdatedItemResult],
) -> Verification {
    if results.is_empty() || results.iter().any(|update| !update.confirmed) {
        verification.verified = false;
        verification.observed = None;
    }
    verification
}

fn unverified_updated_results(
    updates: &[ImportUpdateItem],
    reason: &str,
) -> Vec<UpdatedItemResult> {
    updates
        .iter()
        .map(|item| UpdatedItemResult {
            path: item.path.clone(),
            item_type: item.item_type,
            id: item.id.clone(),
            title: item.title.clone(),
            confirmed: false,
            wanted_status: item.wanted_status,
            observed_status: None,
            reason: Some(reason.to_string()),
        })
        .collect()
}

fn unverified_import_results(payload: &[Value], reason: &str) -> Vec<CreatedItemResult> {
    import_created_items(payload)
        .map(|items| {
            items
                .into_iter()
                .map(|item| CreatedItemResult {
                    path: item.path,
                    item_type: item.item_type,
                    title: item.title,
                    confirmed: false,
                    id: None,
                    reason: Some(reason.to_string()),
                    candidates: Vec::new(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn import_created_items(payload: &[Value]) -> Result<Vec<ImportCreatedItem>> {
    let mut items = Vec::new();
    for (index, item) in payload.iter().enumerate() {
        collect_import_created_items(
            item,
            &format!("[{index}]"),
            ImportSlot::Top,
            None,
            &mut items,
        )?;
    }
    Ok(items)
}

fn collect_import_updates(payload: &[Value], updates: &mut Vec<ImportUpdateItem>) -> Result<()> {
    for (index, item) in payload.iter().enumerate() {
        collect_import_updates_from_item(item, &format!("[{index}]"), updates)?;
    }
    Ok(())
}

fn collect_import_updates_from_item(
    item: &Value,
    path: &str,
    updates: &mut Vec<ImportUpdateItem>,
) -> Result<()> {
    collect_import_updates_from_item_at(item, path, None, updates)
}

fn collect_import_updates_from_item_at(
    item: &Value,
    path: &str,
    parent_id: Option<&str>,
    updates: &mut Vec<ImportUpdateItem>,
) -> Result<()> {
    let object = item
        .as_object()
        .ok_or_else(|| Error::InvalidInput(format!("{path}: item must be an object")))?;
    let operation = object
        .get("operation")
        .and_then(Value::as_str)
        .unwrap_or("create");
    let item_type = match object.get("type").and_then(Value::as_str) {
        Some("to-do") => ItemType::Todo,
        Some("project") => ItemType::Project,
        Some("heading") => ItemType::Heading,
        Some("checklist-item") => ItemType::ChecklistItem,
        Some(other) => {
            return Err(Error::InvalidInput(format!(
                "{path}.type: unsupported import type {other:?}"
            )));
        }
        None => {
            return Err(Error::InvalidInput(format!(
                "{path}.type: update item requires a type"
            )));
        }
    };
    let attributes = object.get("attributes").and_then(Value::as_object);
    let mut update_id = None;
    if operation == "update" {
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| Error::InvalidInput(format!("{path}.id: update requires an id")))?;
        update_id = Some(id);
        let wanted_status = attributes.and_then(import_status_update);
        let destination = attributes.and_then(|attributes| {
            let project = attributes
                .get("list-id")
                .and_then(Value::as_str)
                .or_else(|| attributes.get("list").and_then(Value::as_str));
            let area = attributes
                .get("area-id")
                .and_then(Value::as_str)
                .or_else(|| attributes.get("area").and_then(Value::as_str));
            let heading = attributes
                .get("heading-id")
                .and_then(Value::as_str)
                .or_else(|| attributes.get("heading").and_then(Value::as_str));
            (project.is_some() || area.is_some() || heading.is_some()).then(|| ImportDestination {
                project: project.map(str::to_string),
                area: area.map(str::to_string),
                heading: heading.map(str::to_string),
            })
        });
        updates.push(ImportUpdateItem {
            path: path.to_string(),
            item_type,
            id: id.to_string(),
            title: String::new(),
            parent_id: parent_id.map(str::to_string),
            wanted_status,
            attributes_have_restricted_repeat_fields: attributes.is_some_and(|attributes| {
                ["when", "deadline", "completed", "canceled"]
                    .iter()
                    .any(|field| attributes.contains_key(*field))
            }),
            has_non_status_fields: attributes.is_some_and(|attributes| {
                attributes
                    .keys()
                    .any(|field| !matches!(field.as_str(), "completed" | "canceled"))
            }),
            destination,
            has_list_id: attributes
                .and_then(|attributes| attributes.get("list-id"))
                .and_then(Value::as_str)
                .is_some(),
            has_area_id: attributes
                .and_then(|attributes| attributes.get("area-id"))
                .and_then(Value::as_str)
                .is_some(),
            has_heading_id: attributes
                .and_then(|attributes| attributes.get("heading-id"))
                .and_then(Value::as_str)
                .is_some(),
        });
    }
    if let Some(children) = attributes
        .and_then(|attributes| attributes.get("items"))
        .and_then(Value::as_array)
    {
        for (index, child) in children.iter().enumerate() {
            collect_import_updates_from_item_at(
                child,
                &format!("{path}.attributes.items[{index}]"),
                if item_type == ItemType::Project {
                    update_id
                } else {
                    parent_id
                },
                updates,
            )?;
        }
    }
    if let Some(children) = attributes
        .and_then(|attributes| attributes.get("checklist-items"))
        .and_then(Value::as_array)
    {
        for (index, child) in children.iter().enumerate() {
            collect_import_updates_from_item_at(
                child,
                &format!("{path}.attributes.checklist-items[{index}]"),
                if item_type == ItemType::Todo {
                    update_id
                } else {
                    parent_id
                },
                updates,
            )?;
        }
    }
    Ok(())
}

fn import_status_update(attributes: &serde_json::Map<String, Value>) -> Option<ItemStatus> {
    let completed = attributes.get("completed").and_then(Value::as_bool);
    let canceled = attributes.get("canceled").and_then(Value::as_bool);
    match (completed, canceled) {
        // Things documents `canceled` as taking priority over `completed`,
        // except for `canceled: false` + `completed: true`, where the two
        // statements conflict. The reference implementation leaves that
        // combination unverified rather than guessing and reporting a
        // correct write as failed.
        (Some(true), Some(false)) | (None, None) => None,
        (_, Some(true)) => Some(ItemStatus::Canceled),
        (Some(false) | None, Some(false)) | (Some(false), None) => Some(ItemStatus::Open),
        (Some(true), None) => Some(ItemStatus::Completed),
    }
}

fn merge_import_verification(mut created: Verification, status: Verification) -> Verification {
    created.verified &= status.verified;
    created.attempts = created.attempts.max(status.attempts);
    created.elapsed_ms = created.elapsed_ms.max(status.elapsed_ms);
    if created.observed.is_none() {
        created.observed = status.observed;
    }
    created
}

fn collect_import_created_items(
    item: &Value,
    path: &str,
    slot: ImportSlot,
    parent_project: Option<&str>,
    items: &mut Vec<ImportCreatedItem>,
) -> Result<()> {
    let object = item
        .as_object()
        .ok_or_else(|| Error::InvalidInput(format!("{path}: item must be an object")))?;
    let operation = object
        .get("operation")
        .and_then(Value::as_str)
        .unwrap_or("create");
    let item_type = match object.get("type").and_then(Value::as_str) {
        Some("to-do") => ItemType::Todo,
        Some("project") => ItemType::Project,
        Some("heading") => ItemType::Heading,
        Some("checklist-item") => ItemType::ChecklistItem,
        Some(other) => {
            return Err(Error::InvalidInput(format!(
                "{path}.type: unsupported import type {other:?}"
            )));
        }
        None => {
            return Err(Error::InvalidInput(format!(
                "{path}.type: created import item requires a type"
            )));
        }
    };
    let attributes = object
        .get("attributes")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "{path}.attributes: created item requires an object"
            ))
        })?;
    if operation == "create" && matches!(item_type, ItemType::Todo | ItemType::Project) {
        let title = attributes
            .get("title")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::InvalidInput(format!("{path}.attributes.title is required")))?
            .to_string();
        let destination = if item_type == ItemType::Todo {
            let nested_project = (slot == ImportSlot::ProjectItems)
                .then_some(parent_project)
                .flatten();
            ImportDestination {
                project: nested_project.map(str::to_string).or_else(|| {
                    attributes
                        .get("list-id")
                        .and_then(Value::as_str)
                        .or_else(|| attributes.get("list").and_then(Value::as_str))
                        .map(str::to_string)
                }),
                area: None,
                heading: if nested_project.is_some() {
                    None
                } else {
                    attributes
                        .get("heading-id")
                        .and_then(Value::as_str)
                        .or_else(|| attributes.get("heading").and_then(Value::as_str))
                        .map(str::to_string)
                },
            }
        } else {
            ImportDestination {
                project: None,
                area: attributes
                    .get("area-id")
                    .and_then(Value::as_str)
                    .or_else(|| attributes.get("area").and_then(Value::as_str))
                    .map(str::to_string),
                heading: None,
            }
        };
        items.push(ImportCreatedItem {
            path: path.to_string(),
            item_type,
            title,
            destination,
        });
    }
    if let Some(children) = attributes.get("items").and_then(Value::as_array) {
        let nested_project = if slot == ImportSlot::Top && item_type == ItemType::Project {
            if operation == "update" {
                object
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|id| !id.is_empty())
                    .or(parent_project)
            } else {
                attributes
                    .get("title")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|title| !title.is_empty())
                    .or(parent_project)
            }
        } else {
            parent_project
        };
        for (index, child) in children.iter().enumerate() {
            collect_import_created_items(
                child,
                &format!("{path}.attributes.items[{index}]"),
                ImportSlot::ProjectItems,
                nested_project,
                items,
            )?;
        }
    }
    if let Some(children) = attributes.get("checklist-items").and_then(Value::as_array) {
        for (index, child) in children.iter().enumerate() {
            collect_import_created_items(
                child,
                &format!("{path}.attributes.checklist-items[{index}]"),
                ImportSlot::Checklist,
                parent_project,
                items,
            )?;
        }
    }
    Ok(())
}

fn validate_import_item(
    value: &Value,
    path: &str,
    slot: ImportSlot,
    create_count: &mut usize,
) -> Result<()> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::InvalidInput(format!("{path}: import item must be a JSON object")))?;
    let operation = object
        .get("operation")
        .and_then(Value::as_str)
        .unwrap_or("create");
    if !matches!(operation, "create" | "update") {
        return Err(Error::InvalidInput(format!(
            "{path}.operation: expected \"create\" or \"update\""
        )));
    }
    let item_type = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::InvalidInput(format!("{path}.type: expected a string")))?;
    let allowed: &[&str] = match (operation, slot) {
        ("create", ImportSlot::Top) => &["to-do", "project"],
        ("create", ImportSlot::ProjectItems) => &["to-do", "heading"],
        ("create", ImportSlot::Checklist) => &["checklist-item"],
        (_, _) => &["to-do", "project", "heading", "checklist-item"],
    };
    if !allowed.contains(&item_type) {
        return Err(Error::InvalidInput(format!(
            "{path}.type: {item_type:?} is not allowed in this position; expected {}",
            allowed.join(" or ")
        )));
    }
    if operation == "create" && !matches!(slot, ImportSlot::Checklist) {
        *create_count += 1;
    }
    let Some(attributes) = object.get("attributes") else {
        if operation == "create" {
            return Err(Error::InvalidInput(format!("{path}.attributes: missing")));
        }
        return Ok(());
    };
    let attributes = attributes
        .as_object()
        .ok_or_else(|| Error::InvalidInput(format!("{path}.attributes: expected an object")))?;
    if operation == "update" {
        object
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(|| Error::InvalidInput(format!("{path}.id: update requires an id")))?;
        for field in ["when", "deadline", "creation-date", "completion-date"] {
            if let Some(value) = attributes.get(field)
                && !value.is_string()
                && !value.is_null()
            {
                return Err(Error::InvalidInput(format!(
                    "{path}.attributes.{field}: expected a string"
                )));
            }
        }
        for field in ["completed", "canceled"] {
            if let Some(value) = attributes.get(field)
                && !value.is_boolean()
                && !value.is_null()
            {
                return Err(Error::InvalidInput(format!(
                    "{path}.attributes.{field}: expected a boolean"
                )));
            }
        }
        for field in [
            "list",
            "list-id",
            "heading",
            "heading-id",
            "area",
            "area-id",
        ] {
            if let Some(value) = attributes.get(field)
                && !value.is_string()
                && !value.is_null()
            {
                return Err(Error::InvalidInput(format!(
                    "{path}.attributes.{field}: expected a string"
                )));
            }
        }
        for field in ["title", "notes"] {
            if let Some(value) = attributes.get(field)
                && !value.is_string()
                && !value.is_null()
            {
                return Err(Error::InvalidInput(format!(
                    "{path}.attributes.{field}: expected a string"
                )));
            }
        }
        if let Some(tags) = attributes.get("tags")
            && !tags.is_null()
            && !tags
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_string))
        {
            return Err(Error::InvalidInput(format!(
                "{path}.attributes.tags: expected an array of strings"
            )));
        }
        if let Some(title) = attributes.get("title").and_then(Value::as_str)
            && title.chars().count() > crate::url_scheme::MAX_TITLE
        {
            return Err(Error::InvalidInput(format!(
                "{path}.attributes.title exceeds {} characters",
                crate::url_scheme::MAX_TITLE
            )));
        }
        if let Some(notes) = attributes.get("notes").and_then(Value::as_str)
            && notes.chars().count() > crate::url_scheme::MAX_NOTES
        {
            return Err(Error::InvalidInput(format!(
                "{path}.attributes.notes exceeds {} characters",
                crate::url_scheme::MAX_NOTES
            )));
        }
        for (field, slot) in [
            ("items", ImportSlot::ProjectItems),
            ("checklist-items", ImportSlot::Checklist),
        ] {
            let Some(items) = attributes.get(field) else {
                continue;
            };
            let items = items.as_array().ok_or_else(|| {
                Error::InvalidInput(format!("{path}.attributes.{field}: expected an array"))
            })?;
            for (index, item) in items.iter().enumerate() {
                validate_import_item(
                    item,
                    &format!("{path}.attributes.{field}[{index}]"),
                    slot,
                    create_count,
                )?;
            }
        }
        return Ok(());
    }
    for field in ["title", "notes", "when", "deadline"] {
        if let Some(value) = attributes.get(field)
            && !value.is_string()
            && !value.is_null()
        {
            return Err(Error::InvalidInput(format!(
                "{path}.attributes.{field}: expected a string"
            )));
        }
    }
    if let Some(tags) = attributes.get("tags")
        && !tags.is_null()
        && !tags
            .as_array()
            .is_some_and(|items| items.iter().all(Value::is_string))
    {
        return Err(Error::InvalidInput(format!(
            "{path}.attributes.tags: expected an array of strings"
        )));
    }
    for field in ["completed", "canceled"] {
        if let Some(value) = attributes.get(field)
            && !value.is_boolean()
            && !value.is_null()
        {
            return Err(Error::InvalidInput(format!(
                "{path}.attributes.{field}: expected a boolean"
            )));
        }
    }
    let title = attributes.get("title").and_then(Value::as_str);
    if matches!(item_type, "to-do" | "project") && title.is_none_or(|title| title.trim().is_empty())
    {
        return Err(Error::InvalidInput(format!(
            "{path}.attributes.title: a created {item_type} requires a non-empty title"
        )));
    }
    if let Some(title) = title
        && title.chars().count() > crate::url_scheme::MAX_TITLE
    {
        return Err(Error::InvalidInput(format!(
            "{path}.attributes.title exceeds {} characters",
            crate::url_scheme::MAX_TITLE
        )));
    }
    if let Some(notes) = attributes.get("notes").and_then(Value::as_str)
        && notes.chars().count() > crate::url_scheme::MAX_NOTES
    {
        return Err(Error::InvalidInput(format!(
            "{path}.attributes.notes exceeds {} characters",
            crate::url_scheme::MAX_NOTES
        )));
    }
    if item_type == "project"
        && let Some(items) = attributes.get("items")
    {
        let items = items.as_array().ok_or_else(|| {
            Error::InvalidInput(format!("{path}.attributes.items: expected an array"))
        })?;
        for (index, item) in items.iter().enumerate() {
            validate_import_item(
                item,
                &format!("{path}.attributes.items[{index}]"),
                ImportSlot::ProjectItems,
                create_count,
            )?;
        }
    }
    if item_type == "to-do"
        && let Some(items) = attributes.get("checklist-items")
    {
        let items = items.as_array().ok_or_else(|| {
            Error::InvalidInput(format!(
                "{path}.attributes.checklist-items: expected an array"
            ))
        })?;
        if items.len() > crate::url_scheme::MAX_CHECKLIST {
            return Err(Error::InvalidInput(format!(
                "{path}.attributes.checklist-items has {} items; maximum is {}",
                items.len(),
                crate::url_scheme::MAX_CHECKLIST
            )));
        }
        for (index, item) in items.iter().enumerate() {
            validate_import_item(
                item,
                &format!("{path}.attributes.checklist-items[{index}]"),
                ImportSlot::Checklist,
                create_count,
            )?;
        }
    }
    Ok(())
}

fn reject_duplicate_json_keys(payload: &str) -> Result<()> {
    use serde::de::DeserializeSeed as _;

    #[derive(Debug)]
    struct KeyTracker {
        path: Vec<String>,
        keys: Vec<std::collections::HashSet<String>>,
        error: Option<String>,
    }

    impl KeyTracker {
        fn render_path(&self) -> String {
            format!("${}", self.path.join(""))
        }
    }

    impl<'de> serde::de::Visitor<'de> for &mut KeyTracker {
        type Value = ();

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("valid JSON")
        }

        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            self.keys.push(std::collections::HashSet::new());
            while let Some(key) = map.next_key::<String>()? {
                let duplicate = !self
                    .keys
                    .last_mut()
                    .is_some_and(|keys| keys.insert(key.clone()));
                if duplicate {
                    self.error = Some(format!(
                        "{}: duplicate JSON key {key:?}",
                        self.render_path()
                    ));
                    return Ok(());
                }
                self.path.push(format!(".{key}"));
                map.next_value_seed(TrackerSeed {
                    tracker: &mut *self,
                })?;
                self.path.pop();
            }
            self.keys.pop();
            Ok(())
        }

        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> std::result::Result<Self::Value, A::Error> {
            for index in 0.. {
                self.path.push(format!("[{index}]"));
                let more = seq
                    .next_element_seed(TrackerSeed {
                        tracker: &mut *self,
                    })?
                    .is_some();
                self.path.pop();
                if !more {
                    break;
                }
            }
            Ok(())
        }

        // Scalars carry no keys, but `deserialize_any` must be able to walk
        // through them; without these visitors any string/number/bool value
        // would be rejected as "expected valid JSON".
        fn visit_bool<E: serde::de::Error>(
            self,
            _value: bool,
        ) -> std::result::Result<Self::Value, E> {
            Ok(())
        }

        fn visit_i64<E: serde::de::Error>(
            self,
            _value: i64,
        ) -> std::result::Result<Self::Value, E> {
            Ok(())
        }

        fn visit_u64<E: serde::de::Error>(
            self,
            _value: u64,
        ) -> std::result::Result<Self::Value, E> {
            Ok(())
        }

        fn visit_f64<E: serde::de::Error>(
            self,
            _value: f64,
        ) -> std::result::Result<Self::Value, E> {
            Ok(())
        }

        fn visit_str<E: serde::de::Error>(
            self,
            _value: &str,
        ) -> std::result::Result<Self::Value, E> {
            Ok(())
        }

        fn visit_string<E: serde::de::Error>(
            self,
            _value: String,
        ) -> std::result::Result<Self::Value, E> {
            Ok(())
        }

        fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(())
        }

        fn visit_none<E: serde::de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(())
        }

        fn visit_some<D: serde::Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> std::result::Result<Self::Value, D::Error> {
            deserializer.deserialize_any(self)
        }
    }

    struct TrackerSeed<'a> {
        tracker: &'a mut KeyTracker,
    }

    impl<'de> serde::de::DeserializeSeed<'de> for TrackerSeed<'_> {
        type Value = ();

        fn deserialize<D: serde::Deserializer<'de>>(
            self,
            deserializer: D,
        ) -> std::result::Result<Self::Value, D::Error> {
            deserializer.deserialize_any(self.tracker)
        }
    }

    let mut tracker = KeyTracker {
        path: Vec::new(),
        keys: Vec::new(),
        error: None,
    };
    let mut deserializer = serde_json::Deserializer::from_str(payload);
    TrackerSeed {
        tracker: &mut tracker,
    }
    .deserialize(&mut deserializer)
    .map_err(|error| Error::InvalidInput(format!("invalid import JSON: {error}")))?;
    if let Some(error) = tracker.error {
        return Err(Error::InvalidInput(error));
    }
    Ok(())
}

fn reject_invalid_import_attributes(payload: &str) -> Result<()> {
    let value: Value = serde_json::from_str(payload)
        .map_err(|error| Error::InvalidInput(format!("invalid import JSON: {error}")))?;
    let items = value
        .as_array()
        .ok_or_else(|| Error::InvalidInput("Things JSON import must be an array".to_string()))?;
    for (index, item) in items.iter().enumerate() {
        reject_invalid_import_attributes_at(item, &format!("[{index}]"))?;
    }
    Ok(())
}

fn reject_invalid_import_attributes_at(item: &Value, path: &str) -> Result<()> {
    let object = item
        .as_object()
        .ok_or_else(|| Error::InvalidInput(format!("{path}: item must be an object")))?;
    let attributes = object.get("attributes").and_then(Value::as_object);
    if let Some(attributes) = attributes {
        for field in ["creation-date", "completion-date"] {
            let Some(raw) = attributes.get(field) else {
                continue;
            };
            if raw.is_null() {
                continue;
            }
            let Some(raw) = raw.as_str() else {
                return Err(Error::InvalidDate(format!(
                    "{path}.attributes.{field}: expected an ISO 8601 timestamp"
                )));
            };
            let parsed = parse_things_timestamp(raw).ok_or_else(|| {
                Error::InvalidDate(format!(
                    "{path}.attributes.{field}: {raw:?} is not an ISO 8601 timestamp with an offset"
                ))
            })?;
            if parsed > chrono::Utc::now() + chrono::Duration::seconds(60) {
                return Err(Error::InvalidDate(format!(
                    "{path}.attributes.{field}: future timestamps are saved by Things as now"
                )));
            }
        }
        for field in ["when", "deadline"] {
            let Some(raw) = attributes.get(field) else {
                continue;
            };
            if raw.is_null() {
                continue;
            }
            let Some(raw) = raw.as_str() else {
                return Err(Error::InvalidInput(format!(
                    "{path}.attributes.{field}: expected a string"
                )));
            };
            validate_import_when_deadline(field, raw, path)?;
        }
    }
    if let Some(children) = attributes
        .and_then(|attributes| attributes.get("items"))
        .and_then(Value::as_array)
    {
        for (index, child) in children.iter().enumerate() {
            reject_invalid_import_attributes_at(
                child,
                &format!("{path}.attributes.items[{index}]"),
            )?;
        }
    }
    if let Some(children) = attributes
        .and_then(|attributes| attributes.get("checklist-items"))
        .and_then(Value::as_array)
    {
        for (index, child) in children.iter().enumerate() {
            reject_invalid_import_attributes_at(
                child,
                &format!("{path}.attributes.checklist-items[{index}]"),
            )?;
        }
    }
    Ok(())
}

/// Parse the timestamp forms Things accepts for `creation-date` and
/// `completion-date`. This is intentionally more permissive than RFC 3339:
/// Things accepts trailing spaces, a lowercase `z`, compact offsets, and
/// out-of-range time fields that roll into the next day.
fn parse_things_timestamp(raw: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    let raw = raw.trim_end_matches(' ');
    let (date, rest) = raw.split_once('T')?;
    let (time, offset) = split_timestamp_offset(rest)?;
    let mut date_parts = date.split('-');
    let year = i32::try_from(parse_timestamp_part(date_parts.next()?, 4)?).ok()?;
    let month = parse_timestamp_part(date_parts.next()?, 2)?;
    let day = parse_timestamp_part(date_parts.next()?, 2)?;
    if date_parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    let (clock, fraction) = time
        .split_once('.')
        .map_or((time, None), |(clock, fraction)| (clock, Some(fraction)));
    let mut clock_parts = clock.split(':');
    let hour = parse_timestamp_part(clock_parts.next()?, 2)?;
    let minute = parse_timestamp_part(clock_parts.next()?, 2)?;
    let second = parse_timestamp_part(clock_parts.next()?, 2)?;
    if clock_parts.next().is_some() {
        return None;
    }
    let nanos = match fraction {
        Some(fraction) => {
            if fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
                return None;
            }
            let mut nanos = fraction
                .as_bytes()
                .iter()
                .take(9)
                .fold(0_u32, |value, digit| value * 10 + u32::from(digit - b'0'));
            for _ in fraction.len().min(9)..9 {
                nanos *= 10;
            }
            nanos
        }
        None => 0,
    };

    let offset_seconds = parse_timestamp_offset_value(offset)?;
    let date = chrono::NaiveDate::from_ymd_opt(year, month, day)?;
    let base = date.and_hms_opt(0, 0, 0)?.and_utc();
    let seconds = i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second);
    let timestamp = base
        .checked_add_signed(chrono::Duration::seconds(seconds))?
        .checked_add_signed(chrono::Duration::nanoseconds(i64::from(nanos)))?
        .checked_sub_signed(chrono::Duration::seconds(offset_seconds))?;
    Some(timestamp)
}

fn split_timestamp_offset(time: &str) -> Option<(&str, &str)> {
    let offset_index = time.find(['Z', 'z', '+', '-'])?;
    Some((&time[..offset_index], &time[offset_index..]))
}

fn parse_timestamp_part(value: &str, max_digits: usize) -> Option<u32> {
    if value.is_empty()
        || value.len() > max_digits
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    value.parse().ok()
}

fn parse_timestamp_offset_value(offset: &str) -> Option<i64> {
    if matches!(offset, "Z" | "z") {
        return Some(0);
    }
    let sign = match offset.as_bytes().first()? {
        b'+' => 1_i64,
        b'-' => -1_i64,
        _ => return None,
    };
    let offset = &offset[1..];
    let (hours, minutes, seconds) = if let Some((hours, rest)) = offset.split_once(':') {
        let (minutes, seconds) = rest
            .split_once(':')
            .map_or((rest, ""), |(minutes, seconds)| (minutes, seconds));
        (hours, minutes, seconds)
    } else if offset.len() == 4 {
        (&offset[..2], &offset[2..], "")
    } else {
        (offset, "", "")
    };
    if hours.is_empty() || hours.len() > 2 || minutes.len() > 2 || seconds.len() > 2 {
        return None;
    }
    let hours = parse_timestamp_part(hours, 2)?;
    let minutes = if minutes.is_empty() {
        0
    } else {
        parse_timestamp_part(minutes, 2)?
    };
    let seconds = if seconds.is_empty() {
        0
    } else {
        parse_timestamp_part(seconds, 2)?
    };
    if minutes > 59 || seconds > 59 {
        return None;
    }
    let total = i64::from(hours) * 3_600 + i64::from(minutes) * 60 + i64::from(seconds);
    (total <= 18 * 3_600).then_some(sign * total)
}

fn validate_import_when_deadline(field: &str, value: &str, path: &str) -> Result<()> {
    let value = value.trim();
    let today = dates::today();
    if field == "deadline" {
        if value.contains('@') {
            return Err(Error::InvalidDate(format!(
                "{path}.attributes.deadline: a deadline has no time; use YYYY-MM-DD"
            )));
        }
        if value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
        {
            dates::parse_date(value, today).map_err(|_| {
                Error::InvalidDate(format!(
                    "{path}.attributes.deadline: {value:?} is not a valid YYYY-MM-DD date"
                ))
            })?;
        }
        return Ok(());
    }
    if let Some((day, time)) = value.split_once('@') {
        if !matches!(
            day.to_ascii_lowercase().as_str(),
            "today"
                | "tomorrow"
                | "evening"
                | "monday"
                | "tuesday"
                | "wednesday"
                | "thursday"
                | "friday"
                | "saturday"
                | "sunday"
        ) && day
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
        {
            dates::parse_date(day, today).map_err(|_| {
                Error::InvalidDate(format!(
                    "{path}.attributes.when: {day:?} is not a valid YYYY-MM-DD date"
                ))
            })?;
        }
        chrono::NaiveTime::parse_from_str(time, "%H:%M")
            .or_else(|_| chrono::NaiveTime::parse_from_str(time, "%-I:%M%p"))
            .map_err(|_| {
                Error::InvalidDate(format!(
                    "{path}.attributes.when: {time:?} is not a valid time of day"
                ))
            })?;
    }
    Ok(())
}

fn todo_changed(
    before: &Todo,
    after: &Todo,
    input: &EditItem,
    when: Option<&str>,
    deadline: Option<&str>,
    warnings: &[String],
) -> bool {
    let title_changed = input
        .title
        .as_ref()
        .is_none_or(|title| after.title == *title);
    let notes_changed = input
        .notes
        .as_ref()
        .is_none_or(|notes| after.notes == *notes)
        && input
            .prepend_notes
            .as_ref()
            .is_none_or(|notes| after.notes.starts_with(notes))
        && input
            .append_notes
            .as_ref()
            .is_none_or(|notes| after.notes.ends_with(notes));
    let status_changed = input.status.is_none_or(|status| after.status == status);
    let when_changed = when.is_none_or(|expected| schedule_matches(after, expected));
    let deadline_changed = if input.clear_deadline {
        after.deadline.is_none()
    } else {
        deadline.is_none_or(|deadline| {
            after
                .deadline
                .is_some_and(|actual| deadline_matches(deadline, actual))
        })
    };
    let tags_changed = input.tags.as_ref().is_none_or(|tags| {
        let expected = tags
            .iter()
            .filter(|tag| {
                !warnings
                    .iter()
                    .any(|warning| warning.contains(&format!("{tag:?}")))
            })
            .collect::<Vec<_>>();
        expected.len() == after.tags.len()
            && expected.iter().all(|tag| {
                after
                    .tags
                    .iter()
                    .any(|value| value.eq_ignore_ascii_case(tag))
            })
    }) && input.add_tags.iter().all(|tag| {
        after
            .tags
            .iter()
            .any(|value| value.eq_ignore_ascii_case(tag))
    });
    let checklist_changed = input.checklist.as_ref().is_none_or(|items| {
        after
            .checklist
            .iter()
            .filter(|item| item.status == ItemStatus::Open)
            .map(|item| item.title.as_str())
            .eq(items.iter().map(String::as_str))
    }) && input
        .append_checklist
        .iter()
        .all(|item| after.checklist.iter().any(|value| &value.title == item))
        && input
            .prepend_checklist
            .iter()
            .all(|item| after.checklist.iter().any(|value| &value.title == item));
    let destination_changed = input.project.as_ref().is_none_or(|project| {
        after
            .project_title
            .as_ref()
            .is_some_and(|value| value.eq_ignore_ascii_case(project))
            || after
                .project_id
                .as_ref()
                .is_some_and(|value| value == project)
    }) && input.area.as_ref().is_none_or(|area| {
        after
            .area_title
            .as_ref()
            .is_some_and(|value| value.eq_ignore_ascii_case(area))
            || after.area_id.as_ref().is_some_and(|value| value == area)
    });
    let heading_changed = input.heading.as_ref().is_none_or(|heading| {
        after
            .heading_title
            .as_ref()
            .is_some_and(|value| value.eq_ignore_ascii_case(heading))
            || after
                .heading_id
                .as_ref()
                .is_some_and(|value| value == heading)
    });
    let checklist_status_changed = input
        .complete_checklist
        .iter()
        .all(|title| checklist_has_status(after, title, ItemStatus::Completed))
        && input
            .cancel_checklist
            .iter()
            .all(|title| checklist_has_status(after, title, ItemStatus::Canceled))
        && input
            .reopen_checklist
            .iter()
            .all(|title| checklist_has_status(after, title, ItemStatus::Open));
    let any_explicit = input.title.is_some()
        || input.notes.is_some()
        || input.prepend_notes.is_some()
        || input.append_notes.is_some()
        || input.when.is_some()
        || input.clear_when
        || input.deadline.is_some()
        || input.clear_deadline
        || input.reminder.is_some()
        || input.clear_reminder
        || input.tags.is_some()
        || !input.add_tags.is_empty()
        || input.checklist.is_some()
        || !input.append_checklist.is_empty()
        || !input.prepend_checklist.is_empty()
        || !input.complete_checklist.is_empty()
        || !input.cancel_checklist.is_empty()
        || !input.reopen_checklist.is_empty()
        || input.project.is_some()
        || input.area.is_some()
        || input.heading.is_some()
        || input.status.is_some();
    any_explicit
        && title_changed
        && notes_changed
        && status_changed
        && when_changed
        && deadline_changed
        && tags_changed
        && checklist_changed
        && destination_changed
        && heading_changed
        && checklist_status_changed
        && (before.title != after.title
            || before.notes != after.notes
            || before.status != after.status
            || before.start_bucket != after.start_bucket
            || before.start_date != after.start_date
            || before.reminder != after.reminder
            || before.deadline != after.deadline
            || before.tags != after.tags
            || before.checklist.len() != after.checklist.len()
            || before.checklist != after.checklist
            || before.project_id != after.project_id
            || before.area_id != after.area_id
            || before.heading_id != after.heading_id)
}

fn project_changed(
    before: &Project,
    after: &Project,
    input: &EditItem,
    when: Option<&str>,
    deadline: Option<&str>,
    warnings: &[String],
) -> bool {
    let title_changed = input
        .title
        .as_ref()
        .is_none_or(|title| after.title == *title);
    let notes_changed = input
        .notes
        .as_ref()
        .is_none_or(|notes| after.notes == *notes)
        && input
            .prepend_notes
            .as_ref()
            .is_none_or(|notes| after.notes.starts_with(notes))
        && input
            .append_notes
            .as_ref()
            .is_none_or(|notes| after.notes.ends_with(notes));
    let status_changed = input.status.is_none_or(|status| after.status == status);
    let when_changed = when.is_none_or(|expected| schedule_matches_project(after, expected));
    let deadline_changed = if input.clear_deadline {
        after.deadline.is_none()
    } else {
        deadline.is_none_or(|deadline| {
            after
                .deadline
                .is_some_and(|actual| deadline_matches(deadline, actual))
        })
    };
    let tags_changed = input.tags.as_ref().is_none_or(|tags| {
        let expected = tags
            .iter()
            .filter(|tag| {
                !warnings
                    .iter()
                    .any(|warning| warning.contains(&format!("{tag:?}")))
            })
            .collect::<Vec<_>>();
        expected.len() == after.tags.len()
            && expected.iter().all(|tag| {
                after
                    .tags
                    .iter()
                    .any(|value| value.eq_ignore_ascii_case(tag))
            })
    }) && input.add_tags.iter().all(|tag| {
        after
            .tags
            .iter()
            .any(|value| value.eq_ignore_ascii_case(tag))
    });
    let any_explicit = input.title.is_some()
        || input.notes.is_some()
        || input.prepend_notes.is_some()
        || input.append_notes.is_some()
        || input.when.is_some()
        || input.clear_when
        || input.deadline.is_some()
        || input.clear_deadline
        || input.reminder.is_some()
        || input.clear_reminder
        || input.tags.is_some()
        || !input.add_tags.is_empty()
        || input.area.is_some()
        || input.status.is_some();
    any_explicit
        && title_changed
        && notes_changed
        && status_changed
        && when_changed
        && deadline_changed
        && tags_changed
        && (before.title != after.title
            || before.notes != after.notes
            || before.status != after.status
            || before.start_bucket != after.start_bucket
            || before.start_date != after.start_date
            || before.reminder != after.reminder
            || before.deadline != after.deadline
            || before.tags != after.tags
            || before.area_id != after.area_id)
}

fn deadline_matches(raw: &str, actual: chrono::NaiveDate) -> bool {
    dates::parse_date(raw, dates::today()).is_ok_and(|date| date == actual)
}

fn schedule_matches(todo: &Todo, expected: &str) -> bool {
    if expected.eq_ignore_ascii_case("someday") {
        return todo.start_bucket == crate::models::StartBucket::Someday;
    }
    if expected.eq_ignore_ascii_case("anytime") {
        return todo.start_bucket != crate::models::StartBucket::Someday
            && todo.start_date.is_none();
    }
    let Ok((date, reminder, evening)) = dates::parse_when_time(expected, dates::today()) else {
        return false;
    };
    if evening {
        return todo.evening
            && reminder.is_none_or(|reminder| todo.reminder.as_deref() == Some(reminder.as_str()));
    }
    let date_matches = date.map_or(todo.start_date.is_none(), |date| {
        todo.start_date == Some(date)
    });
    date_matches
        && reminder.is_none_or(|reminder| todo.reminder.as_deref() == Some(reminder.as_str()))
}

fn schedule_matches_project(project: &Project, expected: &str) -> bool {
    if expected.eq_ignore_ascii_case("someday") {
        return project.start_bucket == crate::models::StartBucket::Someday;
    }
    if expected.eq_ignore_ascii_case("anytime") {
        return project.start_bucket != crate::models::StartBucket::Someday
            && project.start_date.is_none();
    }
    let Ok((date, reminder, evening)) = dates::parse_when_time(expected, dates::today()) else {
        return false;
    };
    if evening {
        return project.evening
            && reminder
                .is_none_or(|reminder| project.reminder.as_deref() == Some(reminder.as_str()));
    }
    let date_matches = date.map_or(project.start_date.is_none(), |date| {
        project.start_date == Some(date)
    });
    date_matches
        && reminder.is_none_or(|reminder| project.reminder.as_deref() == Some(reminder.as_str()))
}

fn checklist_has_status(todo: &Todo, title: &str, status: ItemStatus) -> bool {
    todo.checklist
        .iter()
        .any(|item| item.title == title && item.status == status)
}

fn todo_matches_request(
    todo: &Todo,
    when: Option<&str>,
    deadline: Option<&str>,
    tags: &[String],
    warnings: &[String],
) -> bool {
    when.is_none_or(|expected| schedule_matches(todo, expected))
        && deadline.is_none_or(|expected| {
            todo.deadline
                .is_some_and(|actual| deadline_matches(expected, actual))
        })
        && tags_match(&todo.tags, tags, warnings)
}

fn project_matches_request(
    project: &Project,
    when: Option<&str>,
    deadline: Option<&str>,
    tags: &[String],
    warnings: &[String],
) -> bool {
    when.is_none_or(|expected| schedule_matches_project(project, expected))
        && deadline.is_none_or(|expected| {
            project
                .deadline
                .is_some_and(|actual| deadline_matches(expected, actual))
        })
        && tags_match(&project.tags, tags, warnings)
}

fn tags_match(actual: &[String], requested: &[String], warnings: &[String]) -> bool {
    requested
        .iter()
        .filter(|tag| {
            !warnings
                .iter()
                .any(|warning| warning.contains(&format!("{tag:?}")))
        })
        .all(|tag| actual.iter().any(|value| value.eq_ignore_ascii_case(tag)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn write_options_default_to_verified() {
        let options = WriteOptions::default();
        assert!(options.verify);
        assert!(!options.dry_run);
    }

    #[test]
    fn evening_schedule_matching_requires_the_evening_flag() {
        let mut todo = Todo {
            id: "todo-1".to_string(),
            title: "Task".to_string(),
            notes: String::new(),
            notes_truncated: false,
            status: ItemStatus::Open,
            start_bucket: crate::models::StartBucket::Anytime,
            start_bucket_code: 1,
            evening: false,
            trashed: false,
            repeating: false,
            template_id: None,
            project_id: None,
            project_title: None,
            project_trashed: false,
            area_id: None,
            area_title: None,
            heading_id: None,
            heading_title: None,
            start_date: None,
            deadline: None,
            reminder: Some("10:30".to_string()),
            stop_date: None,
            created_at: None,
            modified_at: None,
            index: 0,
            today_index: 0,
            today_index_reference_date: None,
            tags: Vec::new(),
            checklist: Vec::new(),
        };
        assert!(!schedule_matches(&todo, "evening@10:30"));
        todo.evening = true;
        assert!(schedule_matches(&todo, "evening@10:30"));
        assert!(!schedule_matches(&todo, "evening@11:30"));
    }

    #[test]
    fn import_created_items_extracts_top_level_and_nested_todos_and_projects() {
        let payload = vec![
            json!({
                "type": "to-do",
                "attributes": {"title": "Top todo"}
            }),
            json!({
                "type": "project",
                "attributes": {
                    "title": "Launch",
                    "items": [
                        {"type": "heading", "attributes": {"title": "Phase 1"}},
                        {"type": "to-do", "attributes": {"title": "Nested todo"}}
                    ]
                }
            }),
        ];

        let items = import_created_items(&payload).expect("extract");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].path, "[0]");
        assert_eq!(items[0].title, "Top todo");
        assert_eq!(items[1].path, "[1]");
        assert_eq!(items[1].title, "Launch");
        assert_eq!(items[2].path, "[1].attributes.items[1]");
        assert_eq!(items[2].title, "Nested todo");
    }

    #[test]
    fn import_created_items_skips_headings_checklists_and_updates() {
        let payload = vec![json!({
            "type": "project",
            "attributes": {
                "title": "Launch",
                "items": [
                    {"type": "heading", "attributes": {"title": "Phase 1"}},
                    {"type": "to-do", "attributes": {
                        "title": "Task",
                        "checklist-items": [
                            {"type": "checklist-item", "attributes": {"title": "Step"}}
                        ]
                    }},
                    {"type": "to-do", "operation": "update", "id": "todo-1", "attributes": {}}
                ]
            }
        })];

        let items = import_created_items(&payload).expect("extract");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "Launch");
        assert_eq!(items[1].title, "Task");
    }

    #[test]
    fn import_nested_todos_inherit_created_project_destination() {
        let payload = vec![json!({
            "type": "project",
            "attributes": {
                "title": "Launch",
                "items": [
                    {"type": "to-do", "attributes": {"title": "Book venue", "list": "Ignored"}},
                    {"type": "heading", "attributes": {"title": "Phase 1"}},
                    {"type": "to-do", "attributes": {"title": "Send invites", "heading": "Ignored"}}
                ]
            }
        })];

        let items = import_created_items(&payload).expect("extract");
        assert_eq!(items.len(), 3);
        assert_eq!(items[1].title, "Book venue");
        assert_eq!(items[1].destination.project.as_deref(), Some("Launch"));
        assert_eq!(items[1].destination.heading, None);
        assert_eq!(items[2].title, "Send invites");
        assert_eq!(items[2].destination.project.as_deref(), Some("Launch"));
        assert_eq!(items[2].destination.heading, None);
    }

    #[test]
    fn import_nested_todos_in_updated_project_use_project_id() {
        let payload = vec![json!({
            "type": "project",
            "operation": "update",
            "id": "project-1",
            "attributes": {
                "items": [
                    {"type": "to-do", "attributes": {"title": "Book venue"}}
                ]
            }
        })];

        let items = import_created_items(&payload).expect("extract");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].destination.project.as_deref(), Some("project-1"));
    }

    #[test]
    fn import_updates_include_nested_update_items() {
        let payload = vec![json!({
            "type": "project",
            "attributes": {
                "title": "Launch",
                "items": [
                    {
                        "type": "to-do",
                        "operation": "update",
                        "id": "todo-1",
                        "attributes": {"completed": true}
                    },
                    {
                        "type": "to-do",
                        "attributes": {
                            "title": "Task",
                            "checklist-items": [
                                {
                                    "type": "checklist-item",
                                    "operation": "update",
                                    "id": "check-1",
                                    "attributes": {"canceled": true}
                                }
                            ]
                        }
                    }
                ]
            }
        })];

        let mut updates = Vec::new();
        collect_import_updates(&payload, &mut updates).expect("collect");
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].path, "[0].attributes.items[0]");
        assert_eq!(updates[0].id, "todo-1");
        assert_eq!(updates[0].wanted_status, Some(ItemStatus::Completed));
        assert_eq!(
            updates[1].path,
            "[0].attributes.items[1].attributes.checklist-items[0]"
        );
        assert_eq!(updates[1].id, "check-1");
        assert_eq!(updates[1].wanted_status, Some(ItemStatus::Canceled));
    }

    #[test]
    fn import_updates_reject_missing_or_blank_ids() {
        for payload in [
            json!([{
                "type": "to-do",
                "operation": "update",
                "attributes": {"completed": true}
            }]),
            json!([{
                "type": "to-do",
                "operation": "update",
                "id": "   ",
                "attributes": {"completed": true}
            }]),
        ] {
            let items = payload.as_array().expect("array");
            let mut updates = Vec::new();
            let error = collect_import_updates(items, &mut updates).unwrap_err();
            assert_eq!(error.code(), crate::error::ErrorCode::InvalidInput);
            assert!(error.to_string().contains("update requires an id"));
        }
    }

    #[test]
    fn unverified_updated_results_keeps_update_metadata() {
        let updates = vec![
            ImportUpdateItem {
                path: "[0]".to_string(),
                item_type: ItemType::Todo,
                id: "todo-1".to_string(),
                title: "Write report".to_string(),
                parent_id: None,
                wanted_status: Some(ItemStatus::Completed),
                attributes_have_restricted_repeat_fields: false,
                has_non_status_fields: false,
                destination: None,
                has_list_id: false,
                has_area_id: false,
                has_heading_id: false,
            },
            ImportUpdateItem {
                path: "[1]".to_string(),
                item_type: ItemType::Project,
                id: "project-1".to_string(),
                title: "Launch".to_string(),
                parent_id: None,
                wanted_status: None,
                attributes_have_restricted_repeat_fields: false,
                has_non_status_fields: true,
                destination: None,
                has_list_id: false,
                has_area_id: false,
                has_heading_id: false,
            },
        ];

        for reason in ["dry-run", "no-verify"] {
            let results = unverified_updated_results(&updates, reason);
            assert_eq!(results.len(), 2);
            assert_eq!(results[0].path, "[0]");
            assert_eq!(results[0].id, "todo-1");
            assert_eq!(results[0].title, "Write report");
            assert_eq!(results[0].wanted_status, Some(ItemStatus::Completed));
            assert_eq!(results[0].reason.as_deref(), Some(reason));
            assert_eq!(results[1].path, "[1]");
            assert_eq!(results[1].wanted_status, None);
            assert_eq!(results[1].reason.as_deref(), Some(reason));
            assert!(results.iter().all(|item| !item.confirmed));
        }
    }

    #[test]
    fn unverified_import_results_keeps_expected_items_in_payload_order() {
        let payload = vec![json!({
            "type": "project",
            "attributes": {
                "title": "Launch",
                "items": [{"type": "to-do", "attributes": {"title": "Nested"}}]
            }
        })];
        let results = unverified_import_results(&payload, "no-verify");
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].path, "[0]");
        assert_eq!(results[1].path, "[0].attributes.items[0]");
        assert!(results.iter().all(|item| !item.confirmed));
        assert!(
            results
                .iter()
                .all(|item| item.reason.as_deref() == Some("no-verify"))
        );
    }

    #[allow(clippy::cast_possible_truncation)] // Test timestamps are small whole seconds.
    fn imported_item(id: &str, item_type: ItemType, title: &str, created_at: f64) -> Item {
        let created = chrono::DateTime::from_timestamp(created_at as i64, 0);
        match item_type {
            ItemType::Todo => Item::Todo(Todo {
                id: id.to_string(),
                title: title.to_string(),
                notes: String::new(),
                notes_truncated: false,
                status: ItemStatus::Open,
                start_bucket: crate::models::StartBucket::Anytime,
                start_bucket_code: 1,
                evening: false,
                trashed: false,
                repeating: false,
                template_id: None,
                project_id: None,
                project_title: None,
                project_trashed: false,
                area_id: None,
                area_title: None,
                heading_id: None,
                heading_title: None,
                start_date: None,
                deadline: None,
                reminder: None,
                stop_date: None,
                created_at: created,
                modified_at: created,
                index: 0,
                today_index: 0,
                today_index_reference_date: None,
                tags: Vec::new(),
                checklist: Vec::new(),
            }),
            ItemType::Project => Item::Project(Project {
                id: id.to_string(),
                title: title.to_string(),
                notes: String::new(),
                notes_truncated: false,
                status: ItemStatus::Open,
                start_bucket: crate::models::StartBucket::Anytime,
                start_bucket_code: 1,
                evening: false,
                trashed: false,
                repeating: false,
                area_id: None,
                area_title: None,
                start_date: None,
                deadline: None,
                reminder: None,
                stop_date: None,
                created_at: created,
                modified_at: created,
                index: 0,
                today_index: 0,
                today_index_reference_date: None,
                tags: Vec::new(),
                task_count: 0,
                open_task_count: 0,
                checklist_count: 0,
                open_checklist_count: 0,
            }),
            _ => panic!("unsupported test item type"),
        }
    }

    fn expected_import(path: &str, item_type: ItemType, title: &str) -> ImportCreatedItem {
        ImportCreatedItem {
            path: path.to_string(),
            item_type,
            title: title.to_string(),
            destination: ImportDestination::default(),
        }
    }

    fn expected_import_in(
        path: &str,
        item_type: ItemType,
        title: &str,
        destination: ImportDestination,
    ) -> ImportCreatedItem {
        ImportCreatedItem {
            path: path.to_string(),
            item_type,
            title: title.to_string(),
            destination,
        }
    }

    fn imported_todo_in(id: &str, title: &str, created_at: f64, project: Option<&str>) -> Item {
        let mut item = imported_item(id, ItemType::Todo, title, created_at);
        let Item::Todo(todo) = &mut item else {
            unreachable!();
        };
        todo.project_id = project.map(str::to_string);
        todo.project_title = project.map(str::to_string);
        item
    }

    fn available_import(
        items: Vec<Item>,
    ) -> std::collections::HashMap<(ItemType, String), Vec<Item>> {
        let mut available: std::collections::HashMap<(ItemType, String), Vec<Item>> =
            std::collections::HashMap::new();
        for item in items {
            available
                .entry((item.item_type(), item.title().to_string()))
                .or_default()
                .push(item);
        }
        for candidates in available.values_mut() {
            candidates.sort_by(|left, right| {
                left.created_at()
                    .cmp(&right.created_at())
                    .then_with(|| left.id().cmp(right.id()))
            });
        }
        available
    }

    #[test]
    fn import_pairs_same_title_rows_in_creation_order() {
        let expected = vec![
            expected_import("[0]", ItemType::Todo, "Buy oat milk"),
            expected_import("[1]", ItemType::Todo, "Buy oat milk"),
        ];
        let available = available_import(vec![
            imported_item("new-2", ItemType::Todo, "Buy oat milk", 1_760_000_002.0),
            imported_item("new-1", ItemType::Todo, "Buy oat milk", 1_760_000_001.0),
        ]);
        let results = pair_import_created_results(&expected, available);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].id.as_deref(), Some("new-1"));
        assert_eq!(results[1].id.as_deref(), Some("new-2"));
        assert!(results.iter().all(|item| item.confirmed));
    }

    #[test]
    fn import_reports_missing_items_with_available_candidates() {
        let expected = vec![
            expected_import("[0]", ItemType::Todo, "Buy oat milk"),
            expected_import("[1]", ItemType::Todo, "Buy oat milk"),
        ];
        let available = available_import(vec![imported_item(
            "new-1",
            ItemType::Todo,
            "Buy oat milk",
            1_760_000_001.0,
        )]);
        let results = pair_import_created_results(&expected, available);
        assert!(results[0].confirmed);
        assert_eq!(results[0].id.as_deref(), Some("new-1"));
        assert!(!results[1].confirmed);
        assert_eq!(results[1].reason.as_deref(), Some("not-found"));
        assert_eq!(results[1].candidates.len(), 1);
    }

    #[test]
    fn import_does_not_guess_when_creation_times_are_equal() {
        let expected = vec![
            expected_import("[0]", ItemType::Todo, "Buy oat milk"),
            expected_import("[1]", ItemType::Todo, "Buy oat milk"),
        ];
        let available = available_import(vec![
            imported_item("new-1", ItemType::Todo, "Buy oat milk", 1_760_000_001.0),
            imported_item("new-2", ItemType::Todo, "Buy oat milk", 1_760_000_001.0),
        ]);
        let results = pair_import_created_results(&expected, available);
        assert!(results.iter().all(|item| !item.confirmed));
        assert!(
            results
                .iter()
                .all(|item| item.reason.as_deref() == Some("ambiguous"))
        );
    }

    #[test]
    fn import_reports_extra_same_title_rows_as_ambiguous() {
        let expected = vec![expected_import("[0]", ItemType::Todo, "Buy oat milk")];
        let available = available_import(vec![
            imported_item("new-1", ItemType::Todo, "Buy oat milk", 1_760_000_001.0),
            imported_item("new-2", ItemType::Todo, "Buy oat milk", 1_760_000_002.0),
        ]);
        let results = pair_import_created_results(&expected, available);
        assert_eq!(results.len(), 1);
        assert!(!results[0].confirmed);
        assert_eq!(results[0].reason.as_deref(), Some("ambiguous"));
        assert_eq!(results[0].candidates.len(), 2);
    }

    #[test]
    fn import_pairs_projects_independently_of_todos() {
        let expected = vec![
            expected_import("[0]", ItemType::Todo, "Launch"),
            expected_import("[1]", ItemType::Project, "Launch"),
        ];
        let available = available_import(vec![
            imported_item("todo-launch", ItemType::Todo, "Launch", 1_760_000_001.0),
            imported_item(
                "project-launch",
                ItemType::Project,
                "Launch",
                1_760_000_001.0,
            ),
        ]);
        let results = pair_import_created_results(&expected, available);
        assert_eq!(results[0].id.as_deref(), Some("todo-launch"));
        assert_eq!(results[1].id.as_deref(), Some("project-launch"));
    }

    #[test]
    fn import_pairs_same_title_items_by_destination_without_dropping_candidates() {
        let expected = vec![
            expected_import_in(
                "[0]",
                ItemType::Todo,
                "Weekly review",
                ImportDestination {
                    project: Some("project-a".to_string()),
                    ..ImportDestination::default()
                },
            ),
            expected_import_in(
                "[1]",
                ItemType::Todo,
                "Weekly review",
                ImportDestination {
                    project: Some("project-b".to_string()),
                    ..ImportDestination::default()
                },
            ),
        ];
        let available = available_import(vec![
            imported_todo_in("new-a", "Weekly review", 1_760_000_001.0, Some("project-a")),
            imported_todo_in("new-b", "Weekly review", 1_760_000_002.0, Some("project-b")),
        ]);
        let results = pair_import_created_results(&expected, available);
        assert_eq!(results[0].id.as_deref(), Some("new-a"));
        assert_eq!(results[1].id.as_deref(), Some("new-b"));
        assert!(results.iter().all(|item| item.confirmed));
    }

    #[test]
    fn import_marks_shared_candidates_across_destinations_as_ambiguous() {
        let expected = vec![
            expected_import_in(
                "[0]",
                ItemType::Todo,
                "Weekly review",
                ImportDestination::default(),
            ),
            expected_import_in(
                "[1]",
                ItemType::Todo,
                "Weekly review",
                ImportDestination {
                    project: Some("project-a".to_string()),
                    ..ImportDestination::default()
                },
            ),
        ];
        let available = available_import(vec![imported_todo_in(
            "new-a",
            "Weekly review",
            1_760_000_001.0,
            Some("project-a"),
        )]);
        let results = pair_import_created_results(&expected, available);
        assert!(results.iter().all(|item| !item.confirmed));
        assert!(
            results
                .iter()
                .all(|item| item.reason.as_deref() == Some("ambiguous"))
        );
    }

    #[test]
    fn import_status_update_maps_two_way_fields() {
        let attributes = |value: serde_json::Value| value.as_object().expect("object").clone();
        assert_eq!(
            import_status_update(&attributes(json!({"completed": true}))),
            Some(ItemStatus::Completed)
        );
        assert_eq!(
            import_status_update(&attributes(json!({"completed": false}))),
            Some(ItemStatus::Open)
        );
        assert_eq!(
            import_status_update(&attributes(json!({"canceled": true, "completed": true}))),
            Some(ItemStatus::Canceled)
        );
        assert_eq!(
            import_status_update(&attributes(json!({"canceled": false}))),
            Some(ItemStatus::Open)
        );
        assert_eq!(
            import_status_update(&attributes(json!({"canceled": false, "completed": false}))),
            Some(ItemStatus::Open)
        );
        assert_eq!(
            import_status_update(&attributes(json!({"canceled": false, "completed": true}))),
            None
        );
    }

    fn import_update_with(
        wanted_status: Option<ItemStatus>,
        has_non_status_fields: bool,
    ) -> ImportUpdateItem {
        ImportUpdateItem {
            path: "[0]".to_string(),
            item_type: ItemType::Todo,
            id: "todo-1".to_string(),
            title: "Write report".to_string(),
            parent_id: None,
            wanted_status,
            attributes_have_restricted_repeat_fields: false,
            has_non_status_fields,
            destination: None,
            has_list_id: false,
            has_area_id: false,
            has_heading_id: false,
        }
    }

    #[test]
    fn import_update_classification_distinguishes_status_from_fields() {
        let status_only = import_update_with(Some(ItemStatus::Completed), false);
        assert_eq!(
            classify_import_update(&status_only, Some(ItemStatus::Completed)),
            (true, None)
        );
        assert_eq!(
            classify_import_update(&status_only, Some(ItemStatus::Open)),
            (false, Some("status-not-applied"))
        );

        let mixed = import_update_with(Some(ItemStatus::Completed), true);
        assert_eq!(
            classify_import_update(&mixed, Some(ItemStatus::Completed)),
            (false, Some("fields-not-verified"))
        );

        let other_fields = import_update_with(None, true);
        assert_eq!(
            classify_import_update(&other_fields, Some(ItemStatus::Open)),
            (false, Some("not-verified"))
        );

        let mut missing = import_update_with(Some(ItemStatus::Completed), false);
        missing.title.clear();
        assert_eq!(
            classify_import_update(&missing, Some(ItemStatus::Completed)),
            (false, Some("target-not-found"))
        );
    }

    #[test]
    fn merge_import_verification_requires_both_sides() {
        let created = Verification {
            verified: true,
            attempts: 2,
            elapsed_ms: 40,
            observed: Some("1/1 created items confirmed".to_string()),
        };
        let status = Verification {
            verified: false,
            attempts: 4,
            elapsed_ms: 120,
            observed: None,
        };
        let merged = merge_import_verification(created, status);
        assert!(!merged.verified);
        assert_eq!(merged.attempts, 4);
        assert_eq!(merged.elapsed_ms, 120);
        assert_eq!(
            merged.observed.as_deref(),
            Some("1/1 created items confirmed")
        );
    }

    #[test]
    fn mixed_import_update_downgrades_top_level_verification() {
        let verified = Verification {
            verified: true,
            attempts: 3,
            elapsed_ms: 80,
            observed: Some("1/1 import status changes confirmed".to_string()),
        };
        let mixed = UpdatedItemResult {
            path: "[0]".to_string(),
            item_type: ItemType::Todo,
            id: "todo-1".to_string(),
            title: "Write report".to_string(),
            confirmed: false,
            wanted_status: Some(ItemStatus::Completed),
            observed_status: Some(ItemStatus::Completed),
            reason: Some("fields-not-verified".to_string()),
        };
        let finalized = finalize_import_update_verification(verified, &[mixed]);
        assert!(!finalized.verified);
        assert_eq!(finalized.observed, None);
        assert_eq!(finalized.attempts, 3);
    }

    #[test]
    fn import_timestamps_accept_things_compatible_forms() {
        let parse = |raw: &str| {
            parse_things_timestamp(raw)
                .map(|timestamp| timestamp.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true))
        };
        assert_eq!(
            parse("2026-10-05T10:30:00Z").as_deref(),
            Some("2026-10-05T10:30:00Z")
        );
        assert_eq!(
            parse("2026-10-05T10:30:00+02:00").as_deref(),
            Some("2026-10-05T08:30:00Z")
        );
        assert_eq!(
            parse("2026-10-05T10:30:00+0200").as_deref(),
            Some("2026-10-05T08:30:00Z")
        );
        assert_eq!(
            parse("2026-10-05T10:30:00+02").as_deref(),
            Some("2026-10-05T08:30:00Z")
        );
        assert_eq!(
            parse("2026-10-05T10:30:00.5+05:30").as_deref(),
            Some("2026-10-05T05:00:00.500Z")
        );
        assert_eq!(
            parse("2026-10-05T9:30:00Z").as_deref(),
            Some("2026-10-05T09:30:00Z")
        );
        assert_eq!(
            parse("2026-10-5T10:30:00Z").as_deref(),
            Some("2026-10-05T10:30:00Z")
        );
        assert_eq!(
            parse("2026-10-05T10:30:00+2").as_deref(),
            Some("2026-10-05T08:30:00Z")
        );
        assert_eq!(
            parse("2026-10-05T10:30:00+02:0").as_deref(),
            Some("2026-10-05T08:30:00Z")
        );
        assert_eq!(
            parse("2026-10-05T25:30:00Z").as_deref(),
            Some("2026-10-06T01:30:00Z")
        );
        assert_eq!(
            parse("2026-10-08T10:00:00+01:00:00").as_deref(),
            Some("2026-10-08T09:00:00Z")
        );
        assert_eq!(
            parse("2026-10-08T10:00:00Z ").as_deref(),
            Some("2026-10-08T10:00:00Z")
        );
        assert_eq!(
            parse("2026-10-08T10:00:00-05:30").as_deref(),
            Some("2026-10-08T15:30:00Z")
        );
        assert_eq!(
            parse("2026-10-05T10:30:00z").as_deref(),
            Some("2026-10-05T10:30:00Z")
        );
        assert_eq!(
            parse("2026-10-01T10:00:00.123456789123Z").as_deref(),
            Some("2026-10-01T10:00:00.123456789Z")
        );
    }

    #[test]
    fn import_timestamps_reject_invalid_or_ambiguous_forms() {
        for raw in [
            "2026-13-01T10:00:00Z",
            "2026-00-01T10:00:00Z",
            "2026-10-00T10:00:00Z",
            "2026-10-32T10:00:00Z",
            "2026-10-01T10:00:00+200",
            "2026-10-01T10:00:00+00200",
            "2026-10-01T10:00:00+19",
            "2026-10-01T10:00:00+02:60",
            "2026-10-01T10:00:00+123:00",
            "20260-10-01T10:00:00Z",
            "2026-010-01T10:00:00Z",
            "2026-10-001T10:00:00Z",
            "2026-10-01T999999999999999999:00:00Z",
            "2026-10-01T10:000:00Z",
            "2026-10-01T10:00:000Z",
            "2026-10-05T10:30:00Z\t",
            "2026-10-05",
            "2026-10-05T10:30:00",
            "2026-10-05T10:30:00.5",
            "2026-10-05T10:30Z",
            "2026-10-05 10:30:00Z",
            "2026-10-05t10:30:00z",
            "2026-10-05t10:30:00Z",
            "2026-10-05T10:30:00+012",
            "2026-10-05T10:30:00ZZ",
            "2026-10-05T100:00:00Z",
            "2026-10-05T10:30:00+100",
            "2026-10-05T10:30:00,5Z",
            "2026-10-05T10:30:00.Z",
            "+2026-10-05T10:30:00Z",
            " 2026-10-05T10:30:00Z ",
            "",
        ] {
            assert!(
                parse_things_timestamp(raw).is_none(),
                "{raw:?} should be rejected"
            );
        }
    }
}
