use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ItemType {
    Todo,
    Project,
    Heading,
    Area,
    Tag,
    ChecklistItem,
}

impl ItemType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Project => "project",
            Self::Heading => "heading",
            Self::Area => "area",
            Self::Tag => "tag",
            Self::ChecklistItem => "checklist_item",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Open,
    Completed,
    Canceled,
}

impl ItemStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Completed => "completed",
            Self::Canceled => "canceled",
        }
    }

    #[must_use]
    pub const fn from_code(code: i64) -> Self {
        match code {
            3 => Self::Completed,
            2 => Self::Canceled,
            _ => Self::Open,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StartBucket {
    Inbox,
    Anytime,
    Someday,
}

impl StartBucket {
    #[must_use]
    pub const fn from_code(code: i64) -> Self {
        match code {
            0 => Self::Inbox,
            2 => Self::Someday,
            _ => Self::Anytime,
        }
    }
}

/// A built-in Things sidebar view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum View {
    Inbox,
    Today,
    Upcoming,
    Anytime,
    Someday,
    Logbook,
    Trash,
    Deadlines,
    Repeating,
    Projects,
}

impl View {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Today => "today",
            Self::Upcoming => "upcoming",
            Self::Anytime => "anytime",
            Self::Someday => "someday",
            Self::Logbook => "logbook",
            Self::Trash => "trash",
            Self::Deadlines => "deadlines",
            Self::Repeating => "repeating",
            Self::Projects => "projects",
        }
    }
}

impl std::str::FromStr for View {
    type Err = crate::error::Error;

    fn from_str(value: &str) -> crate::error::Result<Self> {
        match value.to_ascii_lowercase().as_str() {
            "inbox" => Ok(Self::Inbox),
            "today" => Ok(Self::Today),
            "upcoming" => Ok(Self::Upcoming),
            "anytime" => Ok(Self::Anytime),
            "someday" => Ok(Self::Someday),
            "logbook" => Ok(Self::Logbook),
            "trash" => Ok(Self::Trash),
            "deadlines" => Ok(Self::Deadlines),
            "repeating" => Ok(Self::Repeating),
            "projects" => Ok(Self::Projects),
            other => Err(crate::error::Error::InvalidInput(format!(
                "unknown view {other:?}; expected inbox, today, upcoming, anytime, someday, logbook, trash, deadlines, repeating, or projects"
            ))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Todo {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub notes: String,
    /// True when `notes` is a list/search preview rather than the full text.
    #[serde(default)]
    pub notes_truncated: bool,
    pub status: ItemStatus,
    pub start_bucket: StartBucket,
    pub start_bucket_code: i64,
    pub evening: bool,
    pub trashed: bool,
    pub repeating: bool,
    pub template_id: Option<String>,
    pub project_id: Option<String>,
    pub project_title: Option<String>,
    pub project_trashed: bool,
    pub area_id: Option<String>,
    pub area_title: Option<String>,
    pub heading_id: Option<String>,
    pub heading_title: Option<String>,
    pub start_date: Option<NaiveDate>,
    pub deadline: Option<NaiveDate>,
    pub reminder: Option<String>,
    pub stop_date: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
    pub modified_at: Option<DateTime<Utc>>,
    pub index: i64,
    pub today_index: i64,
    pub today_index_reference_date: Option<NaiveDate>,
    pub tags: Vec<String>,
    pub checklist: Vec<ChecklistItem>,
}

impl Todo {
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.status == ItemStatus::Open
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Project {
    pub id: String,
    pub title: String,
    pub notes: String,
    /// True when `notes` is a list/search preview rather than the full text.
    pub notes_truncated: bool,
    pub status: ItemStatus,
    pub start_bucket: StartBucket,
    pub start_bucket_code: i64,
    pub evening: bool,
    pub trashed: bool,
    pub repeating: bool,
    pub area_id: Option<String>,
    pub area_title: Option<String>,
    pub start_date: Option<NaiveDate>,
    pub deadline: Option<NaiveDate>,
    pub reminder: Option<String>,
    pub stop_date: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
    pub modified_at: Option<DateTime<Utc>>,
    pub index: i64,
    pub today_index: i64,
    pub today_index_reference_date: Option<NaiveDate>,
    pub tags: Vec<String>,
    pub task_count: i64,
    pub open_task_count: i64,
    pub checklist_count: i64,
    pub open_checklist_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Heading {
    pub id: String,
    pub title: String,
    pub status: ItemStatus,
    pub trashed: bool,
    pub project_id: Option<String>,
    pub project_title: Option<String>,
    pub index: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Area {
    pub id: String,
    pub title: String,
    pub visible: bool,
    pub index: i64,
    pub tags: Vec<String>,
    pub project_count: i64,
    pub open_task_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Tag {
    pub id: String,
    pub title: String,
    pub shortcut: Option<String>,
    pub parent_id: Option<String>,
    pub index: i64,
    pub task_count: i64,
    pub area_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ChecklistItem {
    pub id: String,
    pub title: String,
    pub status: ItemStatus,
    pub stop_date: Option<DateTime<Utc>>,
    pub index: i64,
}

/// Unified item returned by `get`, search, and cross-type listings.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Item {
    Todo(Todo),
    Project(Project),
    Heading(Heading),
    Area(Area),
    Tag(Tag),
}

impl Item {
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Todo(value) => &value.id,
            Self::Project(value) => &value.id,
            Self::Heading(value) => &value.id,
            Self::Area(value) => &value.id,
            Self::Tag(value) => &value.id,
        }
    }

    #[must_use]
    pub fn title(&self) -> &str {
        match self {
            Self::Todo(value) => &value.title,
            Self::Project(value) => &value.title,
            Self::Heading(value) => &value.title,
            Self::Area(value) => &value.title,
            Self::Tag(value) => &value.title,
        }
    }

    #[must_use]
    pub fn item_type(&self) -> ItemType {
        match self {
            Self::Todo(_) => ItemType::Todo,
            Self::Project(_) => ItemType::Project,
            Self::Heading(_) => ItemType::Heading,
            Self::Area(_) => ItemType::Area,
            Self::Tag(_) => ItemType::Tag,
        }
    }

    #[must_use]
    pub fn status(&self) -> Option<ItemStatus> {
        match self {
            Self::Todo(value) => Some(value.status),
            Self::Project(value) => Some(value.status),
            Self::Heading(value) => Some(value.status),
            Self::Area(_) | Self::Tag(_) => None,
        }
    }

    #[must_use]
    pub fn project_title(&self) -> Option<&str> {
        match self {
            Self::Todo(value) => value.project_title.as_deref(),
            Self::Project(_) | Self::Heading(_) | Self::Area(_) | Self::Tag(_) => None,
        }
    }

    #[must_use]
    pub fn index(&self) -> i64 {
        match self {
            Self::Todo(value) => value.index,
            Self::Project(value) => value.index,
            Self::Heading(value) => value.index,
            Self::Area(value) => value.index,
            Self::Tag(value) => value.index,
        }
    }

    /// Whether the item sits in the Things "This Evening" section.
    #[must_use]
    pub fn evening(&self) -> bool {
        match self {
            Self::Todo(value) => value.evening,
            Self::Project(value) => value.evening,
            Self::Heading(_) | Self::Area(_) | Self::Tag(_) => false,
        }
    }

    /// Things' manual Today position (`todayIndex`). Only meaningful for tasks.
    #[must_use]
    pub fn today_index(&self) -> i64 {
        match self {
            Self::Todo(value) => value.today_index,
            Self::Project(value) => value.today_index,
            Self::Heading(_) | Self::Area(_) | Self::Tag(_) => 0,
        }
    }

    /// The day an item was last added to Today; newer days sort first.
    #[must_use]
    pub fn today_index_reference_date(&self) -> Option<NaiveDate> {
        match self {
            Self::Todo(value) => value.today_index_reference_date,
            Self::Project(value) => value.today_index_reference_date,
            Self::Heading(_) | Self::Area(_) | Self::Tag(_) => None,
        }
    }

    #[must_use]
    pub fn created_at(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::Todo(value) => value.created_at,
            Self::Project(value) => value.created_at,
            Self::Heading(_) | Self::Area(_) | Self::Tag(_) => None,
        }
    }

    #[must_use]
    pub fn modified_at(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::Todo(value) => value.modified_at,
            Self::Project(value) => value.modified_at,
            Self::Heading(_) | Self::Area(_) | Self::Tag(_) => None,
        }
    }

    #[must_use]
    pub fn start_date(&self) -> Option<NaiveDate> {
        match self {
            Self::Todo(value) => value.start_date,
            Self::Project(value) => value.start_date,
            Self::Heading(_) | Self::Area(_) | Self::Tag(_) => None,
        }
    }

    #[must_use]
    pub fn deadline(&self) -> Option<NaiveDate> {
        match self {
            Self::Todo(value) => value.deadline,
            Self::Project(value) => value.deadline,
            Self::Heading(_) | Self::Area(_) | Self::Tag(_) => None,
        }
    }

    #[must_use]
    pub fn stop_date(&self) -> Option<DateTime<Utc>> {
        match self {
            Self::Todo(value) => value.stop_date,
            Self::Project(value) => value.stop_date,
            Self::Heading(_) | Self::Area(_) | Self::Tag(_) => None,
        }
    }
}

pub(crate) fn timestamp(raw: Option<f64>) -> Option<DateTime<Utc>> {
    raw.and_then(crate::dates::decode_timestamp)
}
