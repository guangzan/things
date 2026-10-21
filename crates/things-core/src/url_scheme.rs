use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{Error, Result};

pub const MAX_TITLE: usize = 4_000;
pub const MAX_NOTES: usize = 10_000;
pub const MAX_CHECKLIST: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Add,
    AddProject,
    Update,
    UpdateProject,
    Json,
    Show,
}

impl Command {
    const fn path(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::AddProject => "add-project",
            Self::Update => "update",
            Self::UpdateProject => "update-project",
            Self::Json => "json",
            Self::Show => "show",
        }
    }
}

/// Build a Things URL. Things expects `%20` for spaces, not `+`.
pub fn build(command: Command, parameters: &BTreeMap<String, String>) -> Result<String> {
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in parameters {
        serializer.append_pair(key, value);
    }
    let query = serializer.finish().replace('+', "%20");
    let text = format!("things:///{}?{query}", command.path());
    let parsed = Url::parse(&text)
        .map_err(|error| Error::Internal(format!("could not construct Things URL: {error}")))?;
    Ok(parsed.to_string().replace('+', "%20"))
}

/// Build and validate a `things:///add` URL.
pub fn add_todo(input: &AddTodo) -> Result<String> {
    validate_common(&input.title, input.notes.as_deref(), &input.tags)?;
    if input.checklist.len() > MAX_CHECKLIST {
        return Err(Error::InvalidInput(format!(
            "checklist has {} items; Things accepts at most {MAX_CHECKLIST}",
            input.checklist.len()
        )));
    }
    if input.list.is_some() && input.list_id.is_some() {
        return Err(Error::Conflict(
            "list and list_id are mutually exclusive".to_string(),
        ));
    }
    if input.heading.is_some() && input.heading_id.is_some() {
        return Err(Error::Conflict(
            "heading and heading_id are mutually exclusive".to_string(),
        ));
    }
    let mut parameters = common_parameters(
        &input.title,
        input.notes.as_deref(),
        input.when.as_deref(),
        input.deadline.as_deref(),
        &input.tags,
    );
    set(
        &mut parameters,
        "checklist-items",
        input.checklist.join("\n"),
    );
    set_opt(&mut parameters, "list", input.list.as_deref());
    set_opt(&mut parameters, "list-id", input.list_id.as_deref());
    set_opt(&mut parameters, "heading", input.heading.as_deref());
    set_opt(&mut parameters, "heading-id", input.heading_id.as_deref());
    if input.reveal {
        set(&mut parameters, "reveal", "true".to_string());
    }
    build(Command::Add, &parameters)
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddTodo {
    pub title: String,
    pub notes: Option<String>,
    pub when: Option<String>,
    pub deadline: Option<String>,
    pub tags: Vec<String>,
    pub checklist: Vec<String>,
    pub list: Option<String>,
    pub list_id: Option<String>,
    pub heading: Option<String>,
    pub heading_id: Option<String>,
    pub reveal: bool,
}

/// Build and validate a `things:///add-project` URL.
pub fn add_project(input: &AddProject) -> Result<String> {
    validate_common(&input.title, input.notes.as_deref(), &input.tags)?;
    if input.area.is_some() && input.area_id.is_some() {
        return Err(Error::Conflict(
            "area and area_id are mutually exclusive".to_string(),
        ));
    }
    let mut parameters = common_parameters(
        &input.title,
        input.notes.as_deref(),
        input.when.as_deref(),
        input.deadline.as_deref(),
        &input.tags,
    );
    set_opt(&mut parameters, "area", input.area.as_deref());
    set_opt(&mut parameters, "area-id", input.area_id.as_deref());
    set(&mut parameters, "to-dos", input.todos.join("\n"));
    if input.reveal {
        set(&mut parameters, "reveal", "true".to_string());
    }
    build(Command::AddProject, &parameters)
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct AddProject {
    pub title: String,
    pub notes: Option<String>,
    pub when: Option<String>,
    pub deadline: Option<String>,
    pub tags: Vec<String>,
    pub area: Option<String>,
    pub area_id: Option<String>,
    pub todos: Vec<String>,
    pub reveal: bool,
}

/// Build and validate a `things:///update` URL. Only `Some` fields are sent,
/// so omitted fields stay untouched.
pub fn update_todo(input: &UpdateTodo, token: &str) -> Result<String> {
    require_token(token)?;
    validate_optional_common(
        input.title.as_deref(),
        input.notes.as_deref(),
        input.tags.as_deref(),
    )?;
    if input.list.is_some() && input.list_id.is_some() {
        return Err(Error::Conflict(
            "list and list_id are mutually exclusive".to_string(),
        ));
    }
    if input.heading.is_some() && input.heading_id.is_some() {
        return Err(Error::Conflict(
            "heading and heading_id are mutually exclusive".to_string(),
        ));
    }
    if input.completed && input.canceled {
        return Err(Error::Conflict(
            "completed and canceled cannot both be true".to_string(),
        ));
    }
    let mut parameters = BTreeMap::new();
    set(&mut parameters, "id", input.id.clone());
    set(&mut parameters, "auth-token", token.to_string());
    set_opt(&mut parameters, "title", input.title.as_deref());
    set_opt(&mut parameters, "notes", input.notes.as_deref());
    set_opt(
        &mut parameters,
        "prepend-notes",
        input.prepend_notes.as_deref(),
    );
    set_opt(
        &mut parameters,
        "append-notes",
        input.append_notes.as_deref(),
    );
    set_present(&mut parameters, "when", input.when.as_deref());
    set_opt(&mut parameters, "deadline", input.deadline.as_deref());
    set_opt(&mut parameters, "tags", input.tags.as_deref());
    set_opt(&mut parameters, "add-tags", input.add_tags.as_deref());
    set_opt(
        &mut parameters,
        "checklist-items",
        input.checklist.as_deref(),
    );
    set_opt(
        &mut parameters,
        "prepend-checklist-items",
        input.prepend_checklist.as_deref(),
    );
    set_opt(
        &mut parameters,
        "append-checklist-items",
        input.append_checklist.as_deref(),
    );
    set_opt(&mut parameters, "list", input.list.as_deref());
    set_opt(&mut parameters, "list-id", input.list_id.as_deref());
    set_opt(&mut parameters, "heading", input.heading.as_deref());
    set_opt(&mut parameters, "heading-id", input.heading_id.as_deref());
    if input.completed {
        set(&mut parameters, "completed", "true".to_string());
    }
    if input.canceled {
        set(&mut parameters, "canceled", "true".to_string());
    }
    if input.duplicate {
        set(&mut parameters, "duplicate", "true".to_string());
    }
    if input.reveal {
        set(&mut parameters, "reveal", "true".to_string());
    }
    build(Command::Update, &parameters)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default)]
pub struct UpdateTodo {
    pub id: String,
    pub title: Option<String>,
    pub notes: Option<String>,
    pub prepend_notes: Option<String>,
    pub append_notes: Option<String>,
    pub when: Option<String>,
    pub deadline: Option<String>,
    pub tags: Option<String>,
    pub add_tags: Option<String>,
    pub checklist: Option<String>,
    pub prepend_checklist: Option<String>,
    pub append_checklist: Option<String>,
    pub list: Option<String>,
    pub list_id: Option<String>,
    pub heading: Option<String>,
    pub heading_id: Option<String>,
    pub completed: bool,
    pub canceled: bool,
    pub duplicate: bool,
    pub reveal: bool,
}

/// Build and validate a `things:///update-project` URL.
pub fn update_project(input: &UpdateProject, token: &str) -> Result<String> {
    require_token(token)?;
    validate_optional_common(
        input.title.as_deref(),
        input.notes.as_deref(),
        input.tags.as_deref(),
    )?;
    if input.area.is_some() && input.area_id.is_some() {
        return Err(Error::Conflict(
            "area and area_id are mutually exclusive".to_string(),
        ));
    }
    if input.completed && input.canceled {
        return Err(Error::Conflict(
            "completed and canceled cannot both be true".to_string(),
        ));
    }
    let mut parameters = BTreeMap::new();
    set(&mut parameters, "id", input.id.clone());
    set(&mut parameters, "auth-token", token.to_string());
    set_opt(&mut parameters, "title", input.title.as_deref());
    set_opt(&mut parameters, "notes", input.notes.as_deref());
    set_opt(
        &mut parameters,
        "prepend-notes",
        input.prepend_notes.as_deref(),
    );
    set_opt(
        &mut parameters,
        "append-notes",
        input.append_notes.as_deref(),
    );
    set_present(&mut parameters, "when", input.when.as_deref());
    set_opt(&mut parameters, "deadline", input.deadline.as_deref());
    set_opt(&mut parameters, "tags", input.tags.as_deref());
    set_opt(&mut parameters, "add-tags", input.add_tags.as_deref());
    set_opt(&mut parameters, "area", input.area.as_deref());
    set_opt(&mut parameters, "area-id", input.area_id.as_deref());
    if input.completed {
        set(&mut parameters, "completed", "true".to_string());
    }
    if input.canceled {
        set(&mut parameters, "canceled", "true".to_string());
    }
    if input.reveal {
        set(&mut parameters, "reveal", "true".to_string());
    }
    build(Command::UpdateProject, &parameters)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default)]
pub struct UpdateProject {
    pub id: String,
    pub title: Option<String>,
    pub notes: Option<String>,
    pub prepend_notes: Option<String>,
    pub append_notes: Option<String>,
    pub when: Option<String>,
    pub deadline: Option<String>,
    pub tags: Option<String>,
    pub add_tags: Option<String>,
    pub area: Option<String>,
    pub area_id: Option<String>,
    pub completed: bool,
    pub canceled: bool,
    pub reveal: bool,
}

/// Build the JSON import URL. The caller supplies already-validated JSON.
pub fn import_json(data: &str, token: Option<&str>, reveal: bool) -> Result<String> {
    let mut parameters = BTreeMap::new();
    set(&mut parameters, "data", data.to_string());
    set_opt(&mut parameters, "auth-token", token);
    if reveal {
        set(&mut parameters, "reveal", "true".to_string());
    }
    build(Command::Json, &parameters)
}

/// Build a `things:///show` URL.
pub fn show(id: Option<&str>, query: Option<&str>, filter: Option<&str>) -> Result<String> {
    if id.is_none() && query.is_none() {
        return Err(Error::InvalidInput(
            "show requires an id or query".to_string(),
        ));
    }
    let mut parameters = BTreeMap::new();
    set_opt(&mut parameters, "id", id);
    set_opt(&mut parameters, "query", query);
    set_opt(&mut parameters, "filter", filter);
    build(Command::Show, &parameters)
}

fn common_parameters(
    title: &str,
    notes: Option<&str>,
    when: Option<&str>,
    deadline: Option<&str>,
    tags: &[String],
) -> BTreeMap<String, String> {
    let mut parameters = BTreeMap::new();
    set(&mut parameters, "title", title.to_string());
    set_opt(&mut parameters, "notes", notes);
    set_present(&mut parameters, "when", when);
    set_opt(&mut parameters, "deadline", deadline);
    if !tags.is_empty() {
        set(&mut parameters, "tags", tags.join(","));
    }
    parameters
}

fn set(parameters: &mut BTreeMap<String, String>, key: &str, value: String) {
    if !value.is_empty() {
        parameters.insert(key.to_string(), value);
    }
}

fn set_present(parameters: &mut BTreeMap<String, String>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        parameters.insert(key.to_string(), value.to_string());
    }
}

fn set_opt(parameters: &mut BTreeMap<String, String>, key: &str, value: Option<&str>) {
    set_present(parameters, key, value);
}

fn require_token(token: &str) -> Result<()> {
    if token.trim().is_empty() {
        Err(Error::AuthRequired)
    } else {
        Ok(())
    }
}

fn validate_common(title: &str, notes: Option<&str>, tags: &[String]) -> Result<()> {
    validate_optional_common(Some(title), notes, Some(tags.join(",").as_str()))?;
    if title.trim().is_empty() {
        return Err(Error::InvalidInput("title cannot be empty".to_string()));
    }
    Ok(())
}

fn validate_optional_common(
    title: Option<&str>,
    notes: Option<&str>,
    tags: Option<&str>,
) -> Result<()> {
    if let Some(title) = title
        && title.chars().count() > MAX_TITLE
    {
        return Err(Error::InvalidInput(format!(
            "title exceeds {MAX_TITLE} characters"
        )));
    }
    if let Some(notes) = notes
        && notes.chars().count() > MAX_NOTES
    {
        return Err(Error::InvalidInput(format!(
            "notes exceed {MAX_NOTES} characters"
        )));
    }
    if let Some(tags) = tags {
        for tag in tags.split(',') {
            let tag = tag.trim();
            if tag.chars().count() > MAX_TITLE {
                return Err(Error::InvalidInput(format!(
                    "tag exceeds {MAX_TITLE} characters"
                )));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_url_encodes_spaces_as_percent_twenty() {
        let input = AddTodo {
            title: "Buy milk".to_string(),
            notes: Some("2% and a+b".to_string()),
            when: Some("today".to_string()),
            deadline: None,
            tags: vec!["errand".to_string()],
            checklist: vec![],
            list: None,
            list_id: None,
            heading: None,
            heading_id: None,
            reveal: false,
        };
        let url = add_todo(&input).unwrap();
        assert!(url.contains("title=Buy%20milk"));
        assert!(!url.contains("title=Buy+milk"));
        assert!(url.contains("2%25%20and%20a%2Bb"));
    }

    #[test]
    fn update_requires_token() {
        let input = UpdateTodo {
            id: "abc".to_string(),
            ..UpdateTodo::default()
        };
        assert!(matches!(update_todo(&input, ""), Err(Error::AuthRequired)));
    }

    #[test]
    fn update_url_includes_id_and_when() {
        let input = UpdateTodo {
            id: "abc".to_string(),
            when: Some("2026-10-19@10:15".to_string()),
            ..UpdateTodo::default()
        };
        let url = update_todo(&input, "token").unwrap();
        assert!(url.contains("id=abc"), "{url}");
        assert!(url.contains("when=2026-10-19%4010%3A15"), "{url}");
    }

    #[test]
    fn conflicting_fields_are_rejected() {
        let input = AddTodo {
            title: "x".to_string(),
            notes: None,
            when: None,
            deadline: None,
            tags: vec![],
            checklist: vec![],
            list: Some("Project".to_string()),
            list_id: Some("id".to_string()),
            heading: None,
            heading_id: None,
            reveal: false,
        };
        assert!(matches!(add_todo(&input), Err(Error::Conflict(_))));
    }
}
