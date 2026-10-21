use std::path::PathBuf;
use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
        ToolAnnotations,
    },
    service::RequestContext,
    transport::stdio,
};
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use things_core::{
    Config, CreateTodo, EditItem, ItemQuery, ItemStatus, Service, WriteOptions,
    models::{ItemType, View},
    query::{Filter, SortField, SortOrder},
};

#[derive(Clone)]
pub struct ThingsServer {
    core_config: Config,
    database_path: Arc<PathBuf>,
    database_source: Arc<str>,
    mcp_config: McpConfig,
}

#[derive(Debug, Clone)]
pub struct McpConfig {
    pub read_only: bool,
    pub default_limit: usize,
    pub max_limit: usize,
}

impl McpConfig {
    #[must_use]
    pub fn from_config(config: &Config) -> Self {
        let max_limit = config.mcp.max_page_size.clamp(1, 500);
        Self {
            read_only: config.read_only || config.mcp.read_only,
            default_limit: config.mcp.default_page_size.clamp(1, max_limit),
            max_limit,
        }
    }
}

impl ThingsServer {
    #[must_use]
    pub fn new(service: &Service, config: McpConfig) -> Self {
        Self {
            core_config: service.config().clone(),
            database_path: Arc::new(service.database().path().to_path_buf()),
            database_source: Arc::from(service.selection().source_name()),
            mcp_config: config,
        }
    }

    pub async fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let running = self.serve(stdio()).await?;
        running.waiting().await?;
        Ok(())
    }

    fn service(&self) -> Result<Service, McpError> {
        let mut config = self.core_config.clone();
        config.read_only = self.mcp_config.read_only;
        Service::open(config, Some(self.database_path.as_path())).map_err(|error| map_error(&error))
    }

    /// Merge per-call write flags with config-level defaults.
    fn write_options(&self, input: &WriteInput) -> WriteOptions {
        let mut options: WriteOptions = input.clone().into();
        options.create_missing_tags |= self.core_config.write.create_missing_tags;
        options
    }

    /// Tool names visible in the current read/write mode.
    ///
    /// Exposed for embedding, diagnostics, and integration tests.
    #[must_use]
    pub fn tool_names(&self) -> Vec<String> {
        self.tools()
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect()
    }

    #[must_use]
    pub fn tool_definitions(&self) -> Vec<Tool> {
        self.tools()
    }

    /// Execute a tool and return its JSON value without MCP transport.
    ///
    /// This is useful for embedding the server or testing it without a
    /// `Peer`. The stdio server uses the same dispatcher.
    pub fn call_json(&self, name: &str, arguments: Value) -> Result<Value, McpError> {
        self.dispatch(name, arguments)
    }

    /// Execute a tool and preserve MCP's distinction between protocol and
    /// tool-level errors.
    pub fn call_result(&self, name: &str, arguments: Value) -> Result<CallToolResult, McpError> {
        match self.dispatch(name, arguments) {
            Ok(result) => Ok(CallToolResult::structured(result)),
            Err(error) if is_tool_error(&error) => {
                let value = error
                    .data
                    .unwrap_or_else(|| json!({"message": error.message}));
                Ok(CallToolResult::structured_error(value))
            }
            Err(error) => Err(error),
        }
    }

    fn tools(&self) -> Vec<Tool> {
        let mut tools = vec![
            tool::<ListInput>(
                "list",
                "List Things 3 items. Use a sidebar view (inbox, today, upcoming, anytime, someday, logbook, trash, deadlines, repeating, projects) and optional boolean filters. Returns a paginated envelope with total and has_more.",
                true,
                false,
                false,
            ),
            tool_required::<SearchInput>(
                "search",
                "Search titles and notes across todos and projects with optional type, status, tag, area, and project filters. Returns stable ids for follow-up tools.",
                true,
                false,
                false,
                &["query"],
            ),
            tool::<GetInput>(
                "get",
                "Get one item by full id, id prefix (4+ chars), exact title, or unique title substring. Ambiguous references return candidates.",
                true,
                false,
                false,
            ),
            tool::<ListInput>(
                "list_todos",
                "List todos with the same filters as `list`.",
                true,
                false,
                false,
            ),
            tool::<ListInput>(
                "list_projects",
                "List projects with filters and progress counts.",
                true,
                false,
                false,
            ),
            tool::<EmptyInput>(
                "list_areas",
                "List areas with tags and task counts.",
                true,
                false,
                false,
            ),
            tool::<EmptyInput>(
                "list_tags",
                "List tags with task and area usage counts.",
                true,
                false,
                false,
            ),
            tool::<HeadingsInput>(
                "list_headings",
                "List headings inside a project.",
                true,
                false,
                false,
            ),
            tool::<EmptyInput>(
                "stats",
                "Return database counts and health metadata.",
                true,
                false,
                false,
            ),
            tool::<EmptyInput>(
                "get_agent_brief",
                "Return concise AI instructions describing this server, its guarantees, and Things limitations.",
                true,
                false,
                false,
            ),
        ];
        if !self.mcp_config.read_only {
            tools.extend([
                tool_required::<CreateTodoInput>(
                    "add_todo",
                    "Create a to-do via the Things URL scheme and verify it against the database. Dates accept YYYY-MM-DD, today, tomorrow, evening, anytime, or someday. reminder is HH:MM and requires a concrete date, today, tomorrow, or evening; it defaults to today. Unknown tags are ignored unless create_missing_tags is true.",
                    false,
                    false,
                    false,
                    &["title"],
                ),
                tool_required::<CreateProjectInput>(
                    "add_project",
                    "Create a project via the Things URL scheme and verify it. reminder is HH:MM and requires a concrete date, today, tomorrow, or evening; it defaults to today.",
                    false,
                    false,
                    false,
                    &["title"],
                ),
                tool_required::<EditInput>(
                    "edit",
                    "Partially edit a todo or project. Omitted fields stay unchanged. reminder is HH:MM and preserves the item's existing scheduled date. Repeating items reject when, reminder, deadline, and status changes.",
                    false,
                    false,
                    false,
                    &["target"],
                ),
                tool_required::<StatusInput>(
                    "set_status",
                    "Set a todo or project to open, completed, or canceled. This is the supported way to reopen a completed or canceled item.",
                    false,
                    false,
                    false,
                    &["target", "status"],
                ),
                tool_required::<DeleteInput>(
                    "delete",
                    "Move a todo or project to Things Trash through AppleScript. Requires options.yes=true. An item that is already in the Trash cannot be deleted again; use empty_trash instead.",
                    false,
                    false,
                    true,
                    &["target"],
                ),
                tool::<WriteInput>(
                    "empty_trash",
                    "Permanently delete every item in Things Trash. Requires options.yes=true and cannot be undone.",
                    false,
                    false,
                    true,
                ),
                tool_required::<TagCreateInput>(
                    "add_tag",
                    "Create a tag in Things. Tag creation is not available through the URL scheme.",
                    false,
                    false,
                    false,
                    &["name"],
                ),
                tool_required::<TagRenameInput>(
                    "rename_tag",
                    "Rename a tag by id or unique title.",
                    false,
                    false,
                    false,
                    &["target", "new_name"],
                ),
                tool_required::<TagDeleteInput>(
                    "delete_tag",
                    "Delete a tag from Things. Requires options.yes=true.",
                    false,
                    false,
                    true,
                    &["target"],
                ),
                tool_required::<AreaCreateInput>(
                    "add_area",
                    "Create an area in Things. Area creation is not available through the URL scheme.",
                    false,
                    false,
                    false,
                    &["name"],
                ),
                tool_required::<AreaRenameInput>(
                    "rename_area",
                    "Rename an area by id or unique title.",
                    false,
                    false,
                    false,
                    &["target", "new_name"],
                ),
                tool_required::<AreaDeleteInput>(
                    "delete_area",
                    "Delete an area from Things. Requires options.yes=true; its contents are not deleted.",
                    false,
                    false,
                    true,
                    &["target"],
                ),
                tool_required::<ImportInput>(
                    "import",
                    "Send a Things JSON URL-scheme payload in bulk. Returns `created` verdicts for top-level and nested to-dos/projects plus `updated` verdicts for `operation: update` items. For creates, `confirmed` proves an item with that exact title and type appeared after the pre-write snapshot; it does not field-verify every attribute. For updates, `confirmed` covers the requested status only. Search before retrying any `not-found`, `ambiguous`, or unverified item.",
                    false,
                    false,
                    false,
                    &["data"],
                ),
                tool::<OpenInput>(
                    "open",
                    "Reveal an item or built-in Things view in the app.",
                    false,
                    false,
                    false,
                ),
                tool_required::<RepeatInput>(
                    "open_repeat_editor",
                    "Open an item in Things so the user can create or edit its repeat rule. Direct recurrence writes are intentionally unsupported.",
                    false,
                    false,
                    false,
                    &["target"],
                ),
            ]);
        }
        tools
    }

    fn dispatch(&self, name: &str, arguments: Value) -> Result<Value, McpError> {
        let service = self.service()?;
        let result = match name {
            "list" | "list_todos" | "list_projects" => {
                let input: ListInput = decode(arguments)?;
                let mut query =
                    input.into_query(self.mcp_config.default_limit, self.mcp_config.max_limit);
                if name == "list_projects" {
                    query.filter.item_types = vec![ItemType::Project];
                } else if name == "list_todos" {
                    query.filter.item_types = vec![ItemType::Todo];
                }
                to_value(service.list(&query).map_err(|error| map_error(&error))?)?
            }
            "search" => {
                let input: SearchInput = decode(arguments)?;
                let text = input.query.clone();
                let query =
                    input.into_query(self.mcp_config.default_limit, self.mcp_config.max_limit);
                to_value(
                    service
                        .search(&text, &query)
                        .map_err(|error| map_error(&error))?,
                )?
            }
            "get" => {
                let input: GetInput = decode(arguments)?;
                to_value(
                    service
                        .get(&input.target)
                        .map_err(|error| map_error(&error))?,
                )?
            }
            "list_areas" => to_value(service.areas().map_err(|error| map_error(&error))?)?,
            "list_tags" => to_value(service.tags().map_err(|error| map_error(&error))?)?,
            "list_headings" => {
                let input: HeadingsInput = decode(arguments)?;
                to_value(
                    service
                        .headings(&input.project)
                        .map_err(|error| map_error(&error))?,
                )?
            }
            "stats" => to_value(json!({
                "database": service.database().path(),
                "database_source": self.database_source.as_ref(),
                "schema_version": service.database().schema_version().map_err(|error| map_error(&error))?,
                "counts": service.counts().map_err(|error| map_error(&error))?,
                "read_only": self.mcp_config.read_only,
            }))?,
            "get_agent_brief" => to_value(agent_brief())?,
            "add_todo" if !self.mcp_config.read_only => {
                let input: CreateTodoInput = decode(arguments)?;
                let options = self.write_options(&input.options);
                to_value(
                    service
                        .create_todo(input.into(), &options)
                        .map_err(|error| map_error(&error))?,
                )?
            }
            "add_project" if !self.mcp_config.read_only => {
                let input: CreateProjectInput = decode(arguments)?;
                let options = self.write_options(&input.options);
                to_value(
                    service
                        .create_project(input.into(), &options)
                        .map_err(|error| map_error(&error))?,
                )?
            }
            "edit" if !self.mcp_config.read_only => {
                let input: EditInput = decode(arguments)?;
                let options = self.write_options(&input.options);
                to_value(
                    service
                        .edit(&input.into(), &options)
                        .map_err(|error| map_error(&error))?,
                )?
            }
            "set_status" if !self.mcp_config.read_only => {
                let input: StatusInput = decode(arguments)?;
                let status = match input.status {
                    StatusValue::Open => ItemStatus::Open,
                    StatusValue::Completed => ItemStatus::Completed,
                    StatusValue::Canceled => ItemStatus::Canceled,
                };
                let options = self.write_options(&input.options);
                to_value(
                    service
                        .set_status(&input.target, status, &options)
                        .map_err(|error| map_error(&error))?,
                )?
            }
            "delete" if !self.mcp_config.read_only => {
                let input: DeleteInput = decode(arguments)?;
                let options = self.write_options(&input.options);
                to_value(
                    service
                        .delete(&input.target, &options)
                        .map_err(|error| map_error(&error))?,
                )?
            }
            "empty_trash" if !self.mcp_config.read_only => {
                let input: WriteInput = decode(arguments)?;
                let options = self.write_options(&input);
                to_value(
                    service
                        .empty_trash(&options)
                        .map_err(|error| map_error(&error))?,
                )?
            }
            "add_tag" if !self.mcp_config.read_only => {
                let input: TagCreateInput = decode(arguments)?;
                let options = self.write_options(&input.options);
                to_value(
                    service
                        .add_tag(&input.name, &options)
                        .map_err(|error| map_error(&error))?,
                )?
            }
            "rename_tag" if !self.mcp_config.read_only => {
                let input: TagRenameInput = decode(arguments)?;
                let options = self.write_options(&input.options);
                to_value(
                    service
                        .rename_tag(&input.target, &input.new_name, &options)
                        .map_err(|error| map_error(&error))?,
                )?
            }
            "delete_tag" if !self.mcp_config.read_only => {
                let input: TagDeleteInput = decode(arguments)?;
                let options = self.write_options(&input.options);
                to_value(
                    service
                        .delete_tag(&input.target, &options)
                        .map_err(|error| map_error(&error))?,
                )?
            }
            "add_area" if !self.mcp_config.read_only => {
                let input: AreaCreateInput = decode(arguments)?;
                let options = self.write_options(&input.options);
                to_value(
                    service
                        .add_area(&input.name, &options)
                        .map_err(|error| map_error(&error))?,
                )?
            }
            "rename_area" if !self.mcp_config.read_only => {
                let input: AreaRenameInput = decode(arguments)?;
                let options = self.write_options(&input.options);
                to_value(
                    service
                        .rename_area(&input.target, &input.new_name, &options)
                        .map_err(|error| map_error(&error))?,
                )?
            }
            "delete_area" if !self.mcp_config.read_only => {
                let input: AreaDeleteInput = decode(arguments)?;
                let options = self.write_options(&input.options);
                to_value(
                    service
                        .delete_area(&input.target, &options)
                        .map_err(|error| map_error(&error))?,
                )?
            }
            "import" if !self.mcp_config.read_only => {
                let input: ImportInput = decode(arguments)?;
                let options = self.write_options(&input.options);
                to_value(
                    service
                        .import(&input.data, &options)
                        .map_err(|error| map_error(&error))?,
                )?
            }
            "open" if !self.mcp_config.read_only => {
                let input: OpenInput = decode(arguments)?;
                to_value(json!({
                    "url": service.reveal(input.target.as_deref(), input.view.as_deref(), input.background).map_err(|error| map_error(&error))?
                }))?
            }
            "open_repeat_editor" if !self.mcp_config.read_only => {
                let input: RepeatInput = decode(arguments)?;
                to_value(json!({
                    "url": service.repeat_editor(&input.target).map_err(|error| map_error(&error))?
                }))?
            }
            _ => {
                return Err(McpError::invalid_params(
                    format!("unknown or disabled tool {name:?}"),
                    None,
                ));
            }
        };
        Ok(result)
    }
}

impl ServerHandler for ThingsServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("things", env!("CARGO_PKG_VERSION")))
            .with_instructions(agent_brief().to_string())
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + '_ {
        std::future::ready(Ok(ListToolsResult {
            tools: self.tools(),
            ..Default::default()
        }))
    }

    fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<CallToolResponse, McpError>> + '_ {
        let arguments = request
            .arguments
            .map_or(Value::Object(serde_json::Map::default()), Value::Object);
        std::future::ready((|| {
            let mut result = self.call_result(&request.name, arguments)?;
            if let Some(value) = &result.structured_content {
                let text = serde_json::to_string_pretty(value)
                    .map_err(|error| McpError::internal_error(error.to_string(), None))?;
                result.content = vec![ContentBlock::text(text)];
            }
            Ok(result.into())
        })())
    }
}

fn tool<T: JsonSchema>(
    name: &str,
    description: &str,
    read_only: bool,
    open_world: bool,
    destructive: bool,
) -> Tool {
    tool_from_schema(
        name,
        description,
        schema_object::<T>(),
        read_only,
        open_world,
        destructive,
    )
}

fn tool_required<T: JsonSchema>(
    name: &str,
    description: &str,
    read_only: bool,
    open_world: bool,
    destructive: bool,
    required_fields: &[&str],
) -> Tool {
    let mut schema = (*schema_object::<T>()).clone();
    for field in required_fields {
        require_field(&mut schema, field);
    }
    tool_from_schema(
        name,
        description,
        Arc::new(schema),
        read_only,
        open_world,
        destructive,
    )
}

fn tool_from_schema(
    name: &str,
    description: &str,
    input_schema: Arc<serde_json::Map<String, Value>>,
    read_only: bool,
    open_world: bool,
    destructive: bool,
) -> Tool {
    let mut tool = Tool::new(name.to_string(), description.to_string(), input_schema);
    tool.annotations = Some(
        ToolAnnotations::new()
            .read_only(read_only)
            .destructive(destructive)
            .open_world(open_world),
    );
    tool
}

fn schema_object<T: JsonSchema>() -> Arc<serde_json::Map<String, Value>> {
    let value = serde_json::to_value(schema_for!(T)).unwrap_or_else(|_| json!({"type": "object"}));
    Arc::new(value.as_object().cloned().unwrap_or_default())
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T, McpError> {
    serde_json::from_value(value).map_err(|error| McpError::invalid_params(error.to_string(), None))
}

fn to_value<T: Serialize>(value: T) -> Result<Value, McpError> {
    serde_json::to_value(value).map_err(|error| McpError::internal_error(error.to_string(), None))
}

fn map_error(error: &things_core::Error) -> McpError {
    let data = json!({
        "code": error.code().as_str(),
        "message": error.to_string(),
        "candidates": error.candidates(),
        "candidates_truncated": error.candidates_truncated(),
    });
    match error.code() {
        things_core::error::ErrorCode::InvalidInput
        | things_core::error::ErrorCode::InvalidDate
        | things_core::error::ErrorCode::Conflict => {
            McpError::invalid_params(error.to_string(), Some(data))
        }
        things_core::error::ErrorCode::NotFound | things_core::error::ErrorCode::Ambiguous => {
            McpError::resource_not_found(error.to_string(), Some(data))
        }
        things_core::error::ErrorCode::AuthRequired
        | things_core::error::ErrorCode::Permission
        | things_core::error::ErrorCode::Unsupported => {
            McpError::invalid_request(error.to_string(), Some(data))
        }
        _ => McpError::internal_error(error.to_string(), Some(data)),
    }
}

fn is_tool_error(error: &McpError) -> bool {
    if error.code == rmcp::model::ErrorCode::METHOD_NOT_FOUND {
        return false;
    }
    if error.code != rmcp::model::ErrorCode::INVALID_PARAMS {
        return true;
    }
    // Argument decoding failures have no core error code. Service validation
    // failures carry one and belong in the tool result so agents can recover.
    error
        .data
        .as_ref()
        .and_then(|value| value.get("code"))
        .is_some()
}

fn clamp_limit(limit: Option<usize>, default_limit: usize, max_limit: usize) -> usize {
    limit.unwrap_or(default_limit).clamp(1, max_limit)
}

fn require_field(schema: &mut serde_json::Map<String, Value>, field: &str) {
    let required = schema
        .entry("required")
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Value::Array(fields) = required
        && !fields.iter().any(|value| value == field)
    {
        fields.push(Value::String(field.to_string()));
    }
}

fn agent_brief() -> Value {
    json!({
        "identity": "things",
        "purpose": "Read and modify the user's local Things 3 data through a stable, verified interface.",
        "rules": [
            "Reads are SQLite read-only queries against the live Things database.",
            "Writes go through the official Things URL scheme or AppleScript, then are verified against the database.",
            "Prefer UUIDs returned by list/search/get. A 4+ character UUID prefix, exact title, or unique substring also resolves.",
            "Repeating templates share titles with generated instances. A title lookup prefers the generated instance; address the template by UUID. Templates are omitted from ordinary lists and belong to view=repeating, except that explicit trashed listings and logbook report stored rows literally.",
            "Dates use YYYY-MM-DD; when also accepts today, tomorrow, evening, anytime, and someday. reminder is HH:MM and is encoded in the URL as when=date@time.",
            "Unknown tags are ignored by Things unless create_missing_tags is true.",
            "Reminders require a concrete date, today, tomorrow, or evening; anytime and someday cannot carry one. Editing a reminder preserves the item's existing scheduled date, and inbox, anytime, or someday items are rejected.",
            "Setting a reminder on an item that already has one replaces it. Clearing a reminder alone is unsupported because the URL scheme cannot express it without also changing the schedule.",
            "Things does not allow when, reminder, deadline, or status changes on repeating items.",
            "Direct database writes and direct recurrence-rule writes are unsupported by design.",
            "Deleting moves an item to Things Trash and requires options.yes=true.",
            "empty_trash permanently deletes everything in Things Trash and requires options.yes=true.",
            "Deleting an item that is already in Things Trash returns a conflict error, because Things cannot address trashed items individually. Only empty_trash removes them.",
            "Areas support list/create/rename/delete; headings are read-only because Things has no scripting command for them."
        ],
        "workflow": [
            "List or search with a small limit.",
            "List and search return bounded fields; use get on the chosen id when full notes or checklist matter.",
            "Use edit for partial changes and set_status to reopen or close.",
            "Inspect verification.verified; if false, the write was sent but not confirmed.",
            "For import, inspect both created and updated arrays. Mixed-field updates report fields-not-verified even if their status landed; only status-only updates can be fully confirmed.",
            "Checklist-item updates support completed/canceled only and are read back from TMChecklistItem. Update destinations are reference-checked, so valid moves are accepted. To-dos use list/list-id and heading/heading-id; projects use area/area-id. Unknown ids, empty ids, wrong-type destination fields, and mismatched nested parents fail before sending.",
            "Search by title before retrying not-found, ambiguous, or unverified entries; no-verify, not-verified, and fields-not-verified are not proof of failure."
        ]
    })
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EmptyInput {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ListInput {
    pub view: Option<View>,
    pub status: Vec<StatusValue>,
    pub types: Vec<TypeValue>,
    pub project: Option<String>,
    pub area: Option<String>,
    pub heading: Option<String>,
    pub tag: Option<String>,
    pub tags_all: Vec<String>,
    pub tags_any: Vec<String>,
    pub tags_none: Vec<String>,
    pub text: Option<String>,
    pub title_regex: Option<String>,
    pub notes_regex: Option<String>,
    /// Creation date lower bound (inclusive).
    pub created_from: Option<chrono::NaiveDate>,
    /// Creation date upper bound (inclusive).
    pub created_to: Option<chrono::NaiveDate>,
    /// Modification date lower bound (inclusive).
    pub modified_from: Option<chrono::NaiveDate>,
    /// Modification date upper bound (inclusive).
    pub modified_to: Option<chrono::NaiveDate>,
    /// Matches the completion/cancellation timestamp (Logbook).
    pub completed_after: Option<chrono::NaiveDate>,
    /// Upper bound for the completion/cancellation timestamp.
    pub completed_before: Option<chrono::NaiveDate>,
    pub deadline_from: Option<chrono::NaiveDate>,
    pub deadline_to: Option<chrono::NaiveDate>,
    pub start_from: Option<chrono::NaiveDate>,
    pub start_to: Option<chrono::NaiveDate>,
    pub overdue: bool,
    pub has_deadline: Option<bool>,
    pub has_reminder: Option<bool>,
    pub has_tags: Option<bool>,
    pub has_checklist: Option<bool>,
    pub trashed: Option<bool>,
    pub repeating: Option<bool>,
    /// Include open to-dos left in completed, canceled, or trashed projects.
    pub include_closed_project_tasks: bool,
    pub sort: Option<SortValue>,
    pub desc: bool,
    pub limit: Option<usize>,
    pub offset: usize,
    pub include_notes: bool,
    pub include_checklist: bool,
}

impl Default for ListInput {
    fn default() -> Self {
        Self {
            view: None,
            status: Vec::new(),
            types: Vec::new(),
            project: None,
            area: None,
            heading: None,
            tag: None,
            tags_all: Vec::new(),
            tags_any: Vec::new(),
            tags_none: Vec::new(),
            text: None,
            title_regex: None,
            notes_regex: None,
            created_from: None,
            created_to: None,
            modified_from: None,
            modified_to: None,
            completed_after: None,
            completed_before: None,
            deadline_from: None,
            deadline_to: None,
            start_from: None,
            start_to: None,
            overdue: false,
            has_deadline: None,
            has_reminder: None,
            has_tags: None,
            has_checklist: None,
            trashed: None,
            repeating: None,
            include_closed_project_tasks: false,
            sort: None,
            desc: false,
            limit: None,
            offset: 0,
            include_notes: false,
            include_checklist: true,
        }
    }
}

impl ListInput {
    fn into_query(self, default_limit: usize, max_limit: usize) -> ItemQuery {
        let mut filter = self.view.map(Filter::for_view).unwrap_or_default();
        if !self.status.is_empty() {
            filter.status = self.status.into_iter().map(ItemStatus::from).collect();
        }
        if !self.types.is_empty() {
            filter.item_types = self.types.into_iter().map(ItemType::from).collect();
        }
        filter.project = self.project;
        filter.area = self.area;
        filter.heading = self.heading;
        filter.text = self.text;
        filter.title_regex = self.title_regex;
        filter.notes_regex = self.notes_regex;
        filter.created_from = self.created_from;
        filter.created_to = self.created_to;
        filter.modified_from = self.modified_from;
        filter.modified_to = self.modified_to;
        filter.stop_from = self.completed_after;
        filter.stop_to = self.completed_before;
        filter.deadline_from = self.deadline_from;
        filter.deadline_to = self.deadline_to;
        filter.start_from = self.start_from;
        filter.start_to = self.start_to;
        filter.overdue = self.overdue;
        filter.has_deadline = self.has_deadline;
        filter.has_reminder = self.has_reminder;
        filter.has_tags = self.has_tags;
        filter.has_checklist = self.has_checklist;
        filter.trashed = self.trashed;
        filter.repeating = self.repeating;
        filter.include_closed_project_tasks |= self.include_closed_project_tasks;
        filter.tags_all = self.tags_all;
        filter.tags_any = self.tags_any;
        filter.tags_none = self.tags_none;
        if let Some(tag) = self.tag {
            filter.tags_all.push(tag);
        }
        ItemQuery {
            view: self.view,
            filter,
            sort: self.sort.map_or(SortField::Index, SortField::from),
            order: if self.desc {
                SortOrder::Desc
            } else {
                SortOrder::Asc
            },
            limit: Some(clamp_limit(self.limit, default_limit, max_limit)),
            offset: self.offset,
            include_notes: self.include_notes,
            include_checklist: self.include_checklist,
        }
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SearchInput {
    pub query: String,
    /// Optional view scope, such as `today` or `logbook`.
    pub view: Option<View>,
    /// Keep only these statuses; empty means any status.
    pub status: Vec<StatusValue>,
    /// Keep only these types; empty means todos and projects.
    pub types: Vec<TypeValue>,
    pub area: Option<String>,
    pub project: Option<String>,
    pub heading: Option<String>,
    /// Require one tag.
    pub tag: Option<String>,
    pub tags_all: Vec<String>,
    pub tags_any: Vec<String>,
    pub tags_none: Vec<String>,
    pub text: Option<String>,
    pub title_regex: Option<String>,
    pub notes_regex: Option<String>,
    /// Creation date lower bound (inclusive).
    pub created_from: Option<chrono::NaiveDate>,
    /// Creation date upper bound (inclusive).
    pub created_to: Option<chrono::NaiveDate>,
    /// Modification date lower bound (inclusive).
    pub modified_from: Option<chrono::NaiveDate>,
    /// Modification date upper bound (inclusive).
    pub modified_to: Option<chrono::NaiveDate>,
    /// Matches the completion/cancellation timestamp (Logbook).
    pub completed_after: Option<chrono::NaiveDate>,
    /// Upper bound for the completion/cancellation timestamp.
    pub completed_before: Option<chrono::NaiveDate>,
    pub deadline_from: Option<chrono::NaiveDate>,
    pub deadline_to: Option<chrono::NaiveDate>,
    pub start_from: Option<chrono::NaiveDate>,
    pub start_to: Option<chrono::NaiveDate>,
    pub overdue: bool,
    pub has_deadline: Option<bool>,
    pub has_reminder: Option<bool>,
    pub has_tags: Option<bool>,
    pub has_checklist: Option<bool>,
    pub trashed: Option<bool>,
    pub repeating: Option<bool>,
    /// Include open to-dos left in completed, canceled, or trashed projects.
    pub include_closed_project_tasks: bool,
    pub sort: Option<SortValue>,
    pub desc: bool,
    pub limit: Option<usize>,
    pub offset: usize,
    pub include_notes: bool,
    pub include_checklist: bool,
}

impl SearchInput {
    fn into_query(self, default_limit: usize, max_limit: usize) -> ItemQuery {
        let limit = Some(clamp_limit(self.limit, default_limit, max_limit));
        ListInput {
            view: self.view,
            status: self.status,
            types: self.types,
            project: self.project,
            area: self.area,
            heading: self.heading,
            tag: self.tag,
            tags_all: self.tags_all,
            tags_any: self.tags_any,
            tags_none: self.tags_none,
            text: self.text,
            title_regex: self.title_regex,
            notes_regex: self.notes_regex,
            created_from: self.created_from,
            created_to: self.created_to,
            modified_from: self.modified_from,
            modified_to: self.modified_to,
            completed_after: self.completed_after,
            completed_before: self.completed_before,
            deadline_from: self.deadline_from,
            deadline_to: self.deadline_to,
            start_from: self.start_from,
            start_to: self.start_to,
            overdue: self.overdue,
            has_deadline: self.has_deadline,
            has_reminder: self.has_reminder,
            has_tags: self.has_tags,
            has_checklist: self.has_checklist,
            trashed: self.trashed,
            repeating: self.repeating,
            include_closed_project_tasks: self.include_closed_project_tasks,
            sort: self.sort,
            desc: self.desc,
            limit,
            offset: self.offset,
            include_notes: self.include_notes,
            include_checklist: self.include_checklist,
        }
        .into_query(default_limit, max_limit)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetInput {
    pub target: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HeadingsInput {
    pub project: String,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StatusValue {
    #[default]
    Open,
    Completed,
    Canceled,
}

impl From<StatusValue> for ItemStatus {
    fn from(value: StatusValue) -> Self {
        match value {
            StatusValue::Open => Self::Open,
            StatusValue::Completed => Self::Completed,
            StatusValue::Canceled => Self::Canceled,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TypeValue {
    Todo,
    Project,
    Heading,
}

impl From<TypeValue> for ItemType {
    fn from(value: TypeValue) -> Self {
        match value {
            TypeValue::Todo => Self::Todo,
            TypeValue::Project => Self::Project,
            TypeValue::Heading => Self::Heading,
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SortValue {
    Index,
    Title,
    Created,
    Modified,
    StartDate,
    Deadline,
    StopDate,
}

impl From<SortValue> for SortField {
    fn from(value: SortValue) -> Self {
        match value {
            SortValue::Index => Self::Index,
            SortValue::Title => Self::Title,
            SortValue::Created => Self::Created,
            SortValue::Modified => Self::Modified,
            SortValue::StartDate => Self::StartDate,
            SortValue::Deadline => Self::Deadline,
            SortValue::StopDate => Self::StopDate,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct WriteInput {
    pub dry_run: bool,
    pub verify: Option<bool>,
    pub foreground: bool,
    pub create_missing_tags: bool,
    pub strict_tags: bool,
    pub yes: bool,
}

impl From<WriteInput> for WriteOptions {
    fn from(value: WriteInput) -> Self {
        Self {
            dry_run: value.dry_run,
            verify: value.verify.unwrap_or(true),
            foreground: value.foreground,
            create_missing_tags: value.create_missing_tags,
            strict_tags: value.strict_tags,
            yes: value.yes,
        }
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CreateTodoInput {
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
    pub options: WriteInput,
}

impl From<CreateTodoInput> for CreateTodo {
    fn from(value: CreateTodoInput) -> Self {
        Self {
            title: value.title,
            notes: value.notes,
            when: value.when,
            deadline: value.deadline,
            reminder: value.reminder,
            tags: value.tags,
            checklist: value.checklist,
            project: value.project,
            area: value.area,
            heading: value.heading,
            reveal: value.reveal,
        }
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct CreateProjectInput {
    pub title: String,
    pub notes: Option<String>,
    pub when: Option<String>,
    pub deadline: Option<String>,
    pub reminder: Option<String>,
    pub tags: Vec<String>,
    pub area: Option<String>,
    pub todos: Vec<String>,
    pub reveal: bool,
    pub options: WriteInput,
}

impl From<CreateProjectInput> for things_core::service::CreateProject {
    fn from(value: CreateProjectInput) -> Self {
        Self {
            title: value.title,
            notes: value.notes,
            when: value.when,
            deadline: value.deadline,
            reminder: value.reminder,
            tags: value.tags,
            area: value.area,
            todos: value.todos,
            reveal: value.reveal,
        }
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct EditInput {
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
    pub prepend_checklist: Vec<String>,
    pub append_checklist: Vec<String>,
    pub complete_checklist: Vec<String>,
    pub cancel_checklist: Vec<String>,
    pub reopen_checklist: Vec<String>,
    pub project: Option<String>,
    pub area: Option<String>,
    pub heading: Option<String>,
    pub status: Option<StatusValue>,
    pub duplicate: bool,
    pub reveal: bool,
    pub options: WriteInput,
}

impl From<EditInput> for EditItem {
    fn from(value: EditInput) -> Self {
        Self {
            target: value.target,
            title: value.title,
            notes: value.notes,
            prepend_notes: value.prepend_notes,
            append_notes: value.append_notes,
            when: value.when,
            clear_when: value.clear_when,
            deadline: value.deadline,
            clear_deadline: value.clear_deadline,
            reminder: value.reminder,
            clear_reminder: value.clear_reminder,
            tags: value.tags,
            add_tags: value.add_tags,
            checklist: value.checklist,
            append_checklist: value.append_checklist,
            prepend_checklist: value.prepend_checklist,
            complete_checklist: value.complete_checklist,
            cancel_checklist: value.cancel_checklist,
            reopen_checklist: value.reopen_checklist,
            project: value.project,
            area: value.area,
            heading: value.heading,
            status: value.status.map(ItemStatus::from),
            duplicate: value.duplicate,
            reveal: value.reveal,
        }
    }
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct StatusInput {
    pub target: String,
    pub status: StatusValue,
    pub options: WriteInput,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct DeleteInput {
    pub target: String,
    pub options: WriteInput,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct TagCreateInput {
    pub name: String,
    pub options: WriteInput,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct TagRenameInput {
    pub target: String,
    pub new_name: String,
    pub options: WriteInput,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct TagDeleteInput {
    pub target: String,
    pub options: WriteInput,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AreaCreateInput {
    pub name: String,
    pub options: WriteInput,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AreaRenameInput {
    pub target: String,
    pub new_name: String,
    pub options: WriteInput,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct AreaDeleteInput {
    pub target: String,
    pub options: WriteInput,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ImportInput {
    pub data: String,
    pub options: WriteInput,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct OpenInput {
    pub target: Option<String>,
    pub view: Option<String>,
    pub background: bool,
}

impl Default for OpenInput {
    fn default() -> Self {
        Self {
            target: None,
            view: None,
            background: true,
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepeatInput {
    pub target: String,
}
