use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::models::{ItemStatus, ItemType, StartBucket, View};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    Index,
    Title,
    Created,
    Modified,
    StartDate,
    Deadline,
    StopDate,
}

impl SortField {
    #[must_use]
    pub const fn sql_column(self) -> &'static str {
        match self {
            Self::Index => "t.\"index\"",
            Self::Title => "t.title COLLATE NOCASE",
            Self::Created => "t.creationDate",
            Self::Modified => "t.userModificationDate",
            Self::StartDate => "t.startDate",
            Self::Deadline => "t.deadline",
            Self::StopDate => "t.stopDate",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    Asc,
    Desc,
}

impl SortOrder {
    #[must_use]
    pub const fn sql(self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }
}

/// Declarative filters shared by CLI and MCP.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Filter {
    pub status: Vec<ItemStatus>,
    pub item_types: Vec<ItemType>,
    pub project: Option<String>,
    pub area: Option<String>,
    pub heading: Option<String>,
    pub tags_all: Vec<String>,
    pub tags_any: Vec<String>,
    pub tags_none: Vec<String>,
    pub text: Option<String>,
    pub title_regex: Option<String>,
    pub notes_regex: Option<String>,
    pub created_from: Option<NaiveDate>,
    pub created_to: Option<NaiveDate>,
    pub modified_from: Option<NaiveDate>,
    pub modified_to: Option<NaiveDate>,
    pub stop_from: Option<NaiveDate>,
    pub stop_to: Option<NaiveDate>,
    pub start_from: Option<NaiveDate>,
    pub start_to: Option<NaiveDate>,
    pub deadline_from: Option<NaiveDate>,
    pub deadline_to: Option<NaiveDate>,
    pub overdue: bool,
    pub has_deadline: Option<bool>,
    pub has_reminder: Option<bool>,
    pub has_tags: Option<bool>,
    pub has_checklist: Option<bool>,
    pub trashed: Option<bool>,
    pub repeating: Option<bool>,
    /// Include open to-dos left behind in completed, canceled, or trashed
    /// projects. Things hides these from its lists, including Trash.
    pub include_closed_project_tasks: bool,
    pub start_buckets: Vec<StartBucket>,
}

impl Filter {
    #[must_use]
    pub fn for_view(view: View) -> Self {
        match view {
            View::Inbox => Self {
                status: vec![ItemStatus::Open],
                item_types: vec![ItemType::Todo],
                start_buckets: vec![StartBucket::Inbox],
                trashed: Some(false),
                ..Self::default()
            },
            View::Anytime => Self {
                status: vec![ItemStatus::Open],
                item_types: vec![ItemType::Todo],
                start_buckets: vec![StartBucket::Anytime],
                trashed: Some(false),
                ..Self::default()
            },
            View::Someday => Self {
                status: vec![ItemStatus::Open],
                item_types: vec![ItemType::Todo],
                start_buckets: vec![StartBucket::Someday],
                trashed: Some(false),
                ..Self::default()
            },
            View::Logbook => Self {
                status: vec![ItemStatus::Completed, ItemStatus::Canceled],
                trashed: Some(false),
                ..Self::default()
            },
            View::Trash => Self {
                trashed: Some(true),
                ..Self::default()
            },
            View::Deadlines => Self {
                status: vec![ItemStatus::Open],
                item_types: vec![ItemType::Todo, ItemType::Project],
                has_deadline: Some(true),
                trashed: Some(false),
                ..Self::default()
            },
            View::Repeating => Self {
                repeating: Some(true),
                item_types: vec![ItemType::Todo, ItemType::Project],
                trashed: Some(false),
                ..Self::default()
            },
            View::Projects => Self {
                item_types: vec![ItemType::Project],
                trashed: Some(false),
                ..Self::default()
            },
            // Today and Upcoming are date-scoped and get extra SQL predicates.
            View::Today | View::Upcoming => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ItemQuery {
    pub view: Option<View>,
    pub filter: Filter,
    pub sort: SortField,
    pub order: SortOrder,
    pub limit: Option<usize>,
    pub offset: usize,
    /// Include full notes. When false, list results carry at most
    /// [`NOTE_PREVIEW_CHARS`] characters; `get` always returns full text.
    pub include_notes: bool,
    pub include_checklist: bool,
}

/// Maximum note characters carried by a list item when full notes are off.
pub const NOTE_PREVIEW_CHARS: usize = 200;

impl Default for ItemQuery {
    fn default() -> Self {
        Self {
            view: None,
            filter: Filter::default(),
            sort: SortField::Index,
            order: SortOrder::Asc,
            limit: None,
            offset: 0,
            include_notes: false,
            include_checklist: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
    pub has_more: bool,
}

impl<T> Page<T> {
    #[must_use]
    pub fn new(items: Vec<T>, total: usize, limit: usize, offset: usize) -> Self {
        Self {
            has_more: offset.saturating_add(items.len()) < total,
            items,
            total,
            limit,
            offset,
        }
    }
}
