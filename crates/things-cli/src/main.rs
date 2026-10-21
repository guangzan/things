use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use chrono::NaiveDate;
use clap::{ArgAction, Args, Parser, Subcommand, ValueEnum};
use serde::Serialize;
use serde_json::{Value, json};
use things_core::error::ErrorCode;
use things_core::models::{Area, Heading, Item, ItemStatus, ItemType, Tag, View};
use things_core::query::{Filter, ItemQuery, Page, SortField, SortOrder};
use things_core::service::{CreateProject, EditItem, WriteOptions, WriteResult};
use things_core::{Config, CreateTodo, Database, Error, Result, Service};
use things_mcp::{McpConfig, ThingsServer};

#[derive(Debug, Parser)]
#[command(
    name = "things",
    version,
    about = "AI-first Things 3 CLI with verified writes and an MCP server",
    long_about = "Read the live Things 3 database safely, query it with boolean and regex filters, \
                  and mutate it through the official URL scheme or AppleScript. Every mutation can \
                  be verified against the database."
)]
struct Cli {
    /// Explicit path to Things' main.sqlite.
    #[arg(long, global = true, env = "THINGSDB")]
    db: Option<PathBuf>,

    /// Disable every write tool and command.
    #[arg(long, global = true)]
    read_only: bool,

    /// Emit JSON. Equivalent to --format json.
    #[arg(long, global = true)]
    json: bool,

    /// Output format. `ndjson` emits one item per line for list results.
    #[arg(long, global = true, value_enum)]
    format: Option<OutputMode>,

    /// Build and print the official URL or `AppleScript` action without sending it.
    #[arg(long, global = true)]
    dry_run: bool,

    /// Send the write but skip database verification.
    #[arg(long = "no-verify", global = true)]
    no_verify: bool,

    /// Bring Things to the foreground while applying a write.
    #[arg(long, global = true)]
    foreground: bool,

    /// Confirm destructive or project-wide operations.
    #[arg(long, global = true, short = 'y')]
    yes: bool,

    /// Create tags that do not exist before using them.
    #[arg(long = "create-tags", global = true)]
    create_missing_tags: bool,

    /// Reject unknown tags instead of allowing Things to ignore them.
    #[arg(long, global = true)]
    strict_tags: bool,

    /// Override the post-write verification timeout.
    #[arg(long, global = true, env = "THINGS_VERIFY_TIMEOUT_MS")]
    verify_timeout_ms: Option<u64>,

    /// Override the post-write verification polling interval.
    #[arg(long, global = true, env = "THINGS_VERIFY_POLL_MS")]
    verify_poll_ms: Option<u64>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum OutputMode {
    Text,
    Json,
    Ndjson,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List the Today view.
    Today(ListArgs),
    /// List the Inbox.
    Inbox(ListArgs),
    /// List upcoming items.
    Upcoming(ListArgs),
    /// List Anytime items.
    Anytime(ListArgs),
    /// List Someday items.
    Someday(ListArgs),
    /// List completed and canceled items.
    Logbook(ListArgs),
    /// List items in Things Trash.
    Trash(ListArgs),
    /// List open items with deadlines.
    Deadlines(ListArgs),
    /// List repeating tasks and projects.
    Repeating(ListArgs),
    /// List projects.
    Projects(ListArgs),
    /// List any built-in view with advanced filters.
    List {
        #[arg(long)]
        view: Option<String>,
        #[command(flatten)]
        args: ListArgs,
    },
    /// Search titles and notes.
    Search {
        query: String,
        #[command(flatten)]
        args: ListArgs,
    },
    /// Show one item by id, id prefix, exact title, or unique title substring.
    Show { target: String },
    /// Create a to-do.
    Add(AddArgs),
    /// Partially edit a to-do or project.
    Edit(EditArgs),
    /// Mark an item completed.
    Done { target: String },
    /// Mark an item canceled.
    Cancel { target: String },
    /// Reopen a completed or canceled item.
    Reopen { target: String },
    /// Move an item to Things Trash. An item that is already in the Trash
    /// cannot be deleted again; use `things empty-trash` instead.
    Delete { target: String },
    /// Permanently delete everything in Things Trash.
    EmptyTrash,
    /// Project operations.
    Project {
        #[command(subcommand)]
        command: ProjectCommand,
    },
    /// Tag operations.
    Tag {
        #[command(subcommand)]
        command: TagCommand,
    },
    /// Area operations. Defaults to listing areas.
    #[command(visible_alias = "areas")]
    Area {
        #[command(subcommand)]
        command: Option<AreaCommand>,
    },
    /// List headings inside a project.
    Headings { project: String },
    /// Import a Things JSON payload from a file, or `-` for stdin.
    Import { file: String },
    /// Open an item or built-in view in Things.
    Open {
        target: Option<String>,
        #[arg(long)]
        view: Option<String>,
        /// Keep Things in the background.
        #[arg(long)]
        background: bool,
    },
    /// Open an item so its repeat rule can be edited in Things.
    Repeat { target: String },
    /// Run the MCP server over stdio.
    Mcp,
    /// Check database, token, and app readiness.
    Doctor,
    /// Show database counts and schema metadata.
    Stats,
    /// Print instructions optimized for AI agents.
    AgentBrief,
    /// Show resolved configuration.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ProjectCommand {
    /// Create a project.
    Add(Box<ProjectAddArgs>),
    /// Edit a project.
    Edit(Box<EditArgs>),
}

#[derive(Debug, Subcommand)]
enum TagCommand {
    /// List tags.
    List,
    /// Create a tag.
    Add { name: String },
    /// Rename a tag.
    Rename { target: String, new_name: String },
    /// Delete a tag from Things.
    Delete { target: String },
}

#[derive(Debug, Subcommand)]
enum AreaCommand {
    /// List areas.
    List,
    /// Create an area.
    Add { name: String },
    /// Rename an area.
    Rename { target: String, new_name: String },
    /// Delete an area from Things.
    Delete { target: String },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Print the config file path.
    Path,
    /// Print the effective config.
    Show,
}

#[derive(Debug, Args, Default)]
struct ListArgs {
    /// Filter by status. Repeat or comma-separate: open,completed,canceled.
    #[arg(long, value_delimiter = ',')]
    status: Vec<String>,
    /// Filter by type: todo,project,heading.
    #[arg(long = "type", value_delimiter = ',')]
    types: Vec<String>,
    /// Project id, id prefix, exact title, or unique substring.
    #[arg(long)]
    project: Option<String>,
    /// Area id, id prefix, exact title, or unique substring.
    #[arg(long)]
    area: Option<String>,
    /// Heading id or exact title inside the selected project.
    #[arg(long)]
    heading: Option<String>,
    /// Require one tag.
    #[arg(long)]
    tag: Option<String>,
    /// Require every listed tag.
    #[arg(long = "tags-all", value_delimiter = ',')]
    tags_all: Vec<String>,
    /// Require at least one listed tag.
    #[arg(long = "tags-any", value_delimiter = ',')]
    tags_any: Vec<String>,
    /// Exclude items carrying any listed tag.
    #[arg(long = "tags-none", value_delimiter = ',')]
    tags_none: Vec<String>,
    /// Case-insensitive substring across title and notes.
    #[arg(long)]
    text: Option<String>,
    /// Rust regular expression matched against titles.
    #[arg(long)]
    title_regex: Option<String>,
    /// Rust regular expression matched against notes.
    #[arg(long)]
    notes_regex: Option<String>,
    #[arg(long)]
    created_from: Option<NaiveDate>,
    #[arg(long)]
    created_to: Option<NaiveDate>,
    #[arg(long)]
    modified_from: Option<NaiveDate>,
    #[arg(long)]
    modified_to: Option<NaiveDate>,
    /// Keep items completed or canceled on or after this date.
    #[arg(long)]
    completed_after: Option<NaiveDate>,
    /// Keep items completed or canceled on or before this date.
    #[arg(long)]
    completed_before: Option<NaiveDate>,
    #[arg(long)]
    start_from: Option<NaiveDate>,
    #[arg(long)]
    start_to: Option<NaiveDate>,
    #[arg(long)]
    deadline_from: Option<NaiveDate>,
    #[arg(long)]
    deadline_to: Option<NaiveDate>,
    /// Open items whose deadline is before today.
    #[arg(long)]
    overdue: bool,
    #[arg(long)]
    has_deadline: bool,
    #[arg(long)]
    no_deadline: bool,
    #[arg(long)]
    has_reminder: bool,
    #[arg(long)]
    no_reminder: bool,
    #[arg(long)]
    has_tags: bool,
    #[arg(long)]
    no_tags: bool,
    #[arg(long)]
    has_checklist: bool,
    #[arg(long)]
    no_checklist: bool,
    /// List stored Trash rows literally, including repeating templates.
    #[arg(long)]
    trashed: bool,
    /// Exclude Trash; repeating templates stay out of ordinary listings.
    #[arg(long)]
    not_trashed: bool,
    /// Also list open to-dos left in completed, canceled, or trashed projects.
    #[arg(long)]
    closed_projects: bool,
    #[arg(long)]
    repeating: bool,
    #[arg(long)]
    not_repeating: bool,
    /// Sort field: `index,title,created,modified,start_date,deadline,stop_date`.
    #[arg(long)]
    sort: Option<String>,
    /// Reverse the sort order.
    #[arg(long)]
    desc: bool,
    #[arg(long)]
    limit: Option<usize>,
    #[arg(long, default_value_t = 0)]
    offset: usize,
    /// Include full notes; listings otherwise return a 200-character preview.
    #[arg(long)]
    notes: bool,
    /// Include checklist items in list output.
    #[arg(long)]
    checklist: bool,
}

#[derive(Debug, Args)]
struct AddArgs {
    title: String,
    #[arg(long)]
    notes: Option<String>,
    #[arg(long)]
    when: Option<String>,
    #[arg(long)]
    deadline: Option<String>,
    /// Reminder time in 24-hour HH:MM. Requires a concrete date, today, tomorrow, or evening.
    #[arg(long)]
    reminder: Option<String>,
    #[arg(long, value_delimiter = ',')]
    tags: Vec<String>,
    #[arg(long = "checklist", action = ArgAction::Append)]
    checklist: Vec<String>,
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    area: Option<String>,
    #[arg(long)]
    heading: Option<String>,
    #[arg(long)]
    reveal: bool,
}

#[derive(Debug, Args)]
struct ProjectAddArgs {
    title: String,
    #[arg(long)]
    notes: Option<String>,
    #[arg(long)]
    when: Option<String>,
    #[arg(long)]
    deadline: Option<String>,
    /// Reminder time in 24-hour HH:MM. Requires a concrete date, today, tomorrow, or evening.
    #[arg(long)]
    reminder: Option<String>,
    #[arg(long, value_delimiter = ',')]
    tags: Vec<String>,
    #[arg(long)]
    area: Option<String>,
    #[arg(long = "todo", action = ArgAction::Append)]
    todos: Vec<String>,
    #[arg(long)]
    reveal: bool,
}

#[derive(Debug, Args, Default)]
struct EditArgs {
    target: String,
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    notes: Option<String>,
    #[arg(long)]
    prepend_notes: Option<String>,
    #[arg(long)]
    append_notes: Option<String>,
    #[arg(long)]
    when: Option<String>,
    #[arg(long)]
    clear_when: bool,
    #[arg(long)]
    deadline: Option<String>,
    #[arg(long)]
    clear_deadline: bool,
    /// Reminder time in 24-hour HH:MM. Keeps the item's current scheduled date.
    #[arg(long)]
    reminder: Option<String>,
    /// Remove the item's reminder. The URL scheme cannot do this without
    /// changing the schedule, so the command reports why it is unsupported.
    #[arg(long)]
    clear_reminder: bool,
    /// Replace all tags.
    #[arg(long, value_delimiter = ',')]
    tags: Option<Vec<String>>,
    /// Add tags without removing existing ones.
    #[arg(long = "add-tags", value_delimiter = ',')]
    add_tags: Vec<String>,
    /// Replace the checklist.
    #[arg(long = "checklist", value_delimiter = '\n')]
    checklist: Option<Vec<String>>,
    #[arg(long = "append-checklist", action = ArgAction::Append)]
    append_checklist: Vec<String>,
    #[arg(long = "prepend-checklist", action = ArgAction::Append)]
    prepend_checklist: Vec<String>,
    /// Mark checklist items completed by exact title.
    #[arg(long = "complete-checklist", action = ArgAction::Append)]
    complete_checklist: Vec<String>,
    /// Mark checklist items canceled by exact title.
    #[arg(long = "cancel-checklist", action = ArgAction::Append)]
    cancel_checklist: Vec<String>,
    /// Reopen checklist items by exact title.
    #[arg(long = "reopen-checklist", action = ArgAction::Append)]
    reopen_checklist: Vec<String>,
    #[arg(long)]
    project: Option<String>,
    #[arg(long)]
    area: Option<String>,
    #[arg(long)]
    heading: Option<String>,
    /// completed or canceled; use `things reopen` to reopen.
    #[arg(long)]
    status: Option<String>,
    #[arg(long)]
    reveal: bool,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let wants_json = wants_json_output();
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) {
                let _ = error.print();
                return ExitCode::SUCCESS;
            }
            if wants_json {
                print_error_json(ErrorCode::InvalidInput, &error.to_string(), None, false);
            } else {
                let _ = error.print();
            }
            return ExitCode::from(2);
        }
    };

    match run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let wants_json = wants_json_output();
            if wants_json {
                print_error_json(
                    error.code(),
                    &error.to_string(),
                    error.candidates(),
                    error.candidates_truncated(),
                );
            } else {
                eprintln!("error: {error}");
            }
            ExitCode::from(exit_code(error.code()))
        }
    }
}

async fn run(cli: Cli) -> Result<()> {
    let mut config = Config::load()?;
    if cli.read_only {
        config.read_only = true;
    }
    if let Some(timeout) = cli.verify_timeout_ms {
        config.write.verify_timeout_ms = timeout;
    }
    if let Some(poll) = cli.verify_poll_ms {
        config.write.verify_poll_ms = poll;
    }

    let mode = resolve_mode(&cli, &config);
    let write_options = WriteOptions {
        dry_run: cli.dry_run,
        verify: !cli.no_verify,
        foreground: cli.foreground,
        create_missing_tags: cli.create_missing_tags || config.write.create_missing_tags,
        strict_tags: cli.strict_tags,
        yes: cli.yes,
    };

    match &cli.command {
        Command::Config { command } => run_config(command, &config, cli.db.as_deref(), mode),
        Command::AgentBrief => {
            output(mode, &agent_brief())?;
            Ok(())
        }
        Command::Doctor => run_doctor(&config, cli.db.as_deref(), mode),
        Command::Mcp => {
            let service = Service::open(config.clone(), cli.db.as_deref())?;
            let mcp_config = McpConfig::from_config(&config);
            ThingsServer::new(&service, mcp_config)
                .run()
                .await
                .map_err(|error| Error::Internal(error.to_string()))
        }
        Command::Stats => {
            let service = Service::open(config, cli.db.as_deref())?;
            output(mode, &service.stats()?)
        }
        _ => {
            let service = Service::open(config, cli.db.as_deref())?;
            run_service_command(&cli.command, &service, &write_options, mode)
        }
    }
}

fn run_service_command(
    command: &Command,
    service: &Service,
    options: &WriteOptions,
    mode: OutputMode,
) -> Result<()> {
    match command {
        Command::Today(args) => list_view(service, View::Today, args, mode),
        Command::Inbox(args) => list_view(service, View::Inbox, args, mode),
        Command::Upcoming(args) => list_view(service, View::Upcoming, args, mode),
        Command::Anytime(args) => list_view(service, View::Anytime, args, mode),
        Command::Someday(args) => list_view(service, View::Someday, args, mode),
        Command::Logbook(args) => list_view(service, View::Logbook, args, mode),
        Command::Trash(args) => list_view(service, View::Trash, args, mode),
        Command::Deadlines(args) => list_view(service, View::Deadlines, args, mode),
        Command::Repeating(args) => list_view(service, View::Repeating, args, mode),
        Command::Projects(args) => list_view(service, View::Projects, args, mode),
        Command::List { view, args } => {
            let view = view.as_deref().map(parse_view).transpose()?;
            list_view_optional(service, view, args, mode)
        }
        Command::Search { query, args } => {
            let query_options = build_query(None, args)?;
            let page = service.search(query, &query_options)?;
            output_page(&page, mode)
        }
        Command::Show { target } => {
            let item = service.get(target)?;
            output(mode, &item)
        }
        Command::Add(args) => {
            let result = service.create_todo(
                CreateTodo {
                    title: args.title.clone(),
                    notes: args.notes.clone(),
                    when: args.when.clone(),
                    deadline: args.deadline.clone(),
                    reminder: args.reminder.clone(),
                    tags: args.tags.clone(),
                    checklist: args.checklist.clone(),
                    project: args.project.clone(),
                    area: args.area.clone(),
                    heading: args.heading.clone(),
                    reveal: args.reveal,
                },
                options,
            )?;
            output_write(&result, mode)
        }
        Command::Edit(args) => {
            let result = service.edit(&edit_item(args)?, options)?;
            output_write(&result, mode)
        }
        Command::Done { target } => output_write(
            &service.set_status(target, ItemStatus::Completed, options)?,
            mode,
        ),
        Command::Cancel { target } => output_write(
            &service.set_status(target, ItemStatus::Canceled, options)?,
            mode,
        ),
        Command::Reopen { target } => output_write(
            &service.set_status(target, ItemStatus::Open, options)?,
            mode,
        ),
        Command::Delete { target } => output_write(&service.delete(target, options)?, mode),
        Command::EmptyTrash => output_write(&service.empty_trash(options)?, mode),
        Command::Project { command } => match command {
            ProjectCommand::Add(args) => {
                let result = service.create_project(
                    CreateProject {
                        title: args.title.clone(),
                        notes: args.notes.clone(),
                        when: args.when.clone(),
                        deadline: args.deadline.clone(),
                        reminder: args.reminder.clone(),
                        tags: args.tags.clone(),
                        area: args.area.clone(),
                        todos: args.todos.clone(),
                        reveal: args.reveal,
                    },
                    options,
                )?;
                output_write(&result, mode)
            }
            ProjectCommand::Edit(args) => {
                let result = service.edit(&edit_item(args)?, options)?;
                output_write(&result, mode)
            }
        },
        Command::Tag { command } => match command {
            TagCommand::List => output(mode, &service.tags()?),
            TagCommand::Add { name } => output(mode, &service.add_tag(name, options)?),
            TagCommand::Rename { target, new_name } => {
                output(mode, &service.rename_tag(target, new_name, options)?)
            }
            TagCommand::Delete { target } => output(mode, &service.delete_tag(target, options)?),
        },
        Command::Area { command } => match command {
            None | Some(AreaCommand::List) => output(mode, &service.areas()?),
            Some(AreaCommand::Add { name }) => output(mode, &service.add_area(name, options)?),
            Some(AreaCommand::Rename { target, new_name }) => {
                output(mode, &service.rename_area(target, new_name, options)?)
            }
            Some(AreaCommand::Delete { target }) => {
                output(mode, &service.delete_area(target, options)?)
            }
        },
        Command::Headings { project } => output(mode, &service.headings(project)?),
        Command::Import { file } => {
            let payload = read_import(file)?;
            output_write(&service.import(&payload, options)?, mode)
        }
        Command::Open {
            target,
            view,
            background,
        } => {
            let url = service.reveal(target.as_deref(), view.as_deref(), *background)?;
            output(mode, &json!({"opened": url}))
        }
        Command::Repeat { target } => {
            let url = service.repeat_editor(target)?;
            output(
                mode,
                &json!({
                    "opened": url,
                    "message": "Direct repeat-rule writes are unsupported; edit the rule in Things."
                }),
            )
        }
        Command::Mcp
        | Command::Doctor
        | Command::Stats
        | Command::AgentBrief
        | Command::Config { .. } => Err(Error::Internal(
            "command was dispatched to the wrong handler".to_string(),
        )),
    }
}

fn list_view(service: &Service, view: View, args: &ListArgs, mode: OutputMode) -> Result<()> {
    list_view_optional(service, Some(view), args, mode)
}

fn list_view_optional(
    service: &Service,
    view: Option<View>,
    args: &ListArgs,
    mode: OutputMode,
) -> Result<()> {
    let query = build_query(view, args)?;
    let page = service.list(&query)?;
    output_page(&page, mode)
}

fn build_query(view: Option<View>, args: &ListArgs) -> Result<ItemQuery> {
    let mut filter = view.map(Filter::for_view).unwrap_or_default();
    if !args.status.is_empty() {
        filter.status = args
            .status
            .iter()
            .map(|value| parse_status(value))
            .collect::<Result<Vec<_>>>()?;
    }
    if !args.types.is_empty() {
        filter.item_types = args
            .types
            .iter()
            .map(|value| parse_item_type(value))
            .collect::<Result<Vec<_>>>()?;
    }
    filter.project.clone_from(&args.project);
    filter.area.clone_from(&args.area);
    filter.heading.clone_from(&args.heading);
    filter.text.clone_from(&args.text);
    filter.title_regex.clone_from(&args.title_regex);
    filter.notes_regex.clone_from(&args.notes_regex);
    filter.created_from = args.created_from;
    filter.created_to = args.created_to;
    filter.modified_from = args.modified_from;
    filter.modified_to = args.modified_to;
    filter.stop_from = args.completed_after;
    filter.stop_to = args.completed_before;
    filter.start_from = args.start_from;
    filter.start_to = args.start_to;
    filter.deadline_from = args.deadline_from;
    filter.deadline_to = args.deadline_to;
    filter.overdue = args.overdue;
    filter.has_deadline = bool_pair(args.has_deadline, args.no_deadline, "deadline")?;
    filter.has_reminder = bool_pair(args.has_reminder, args.no_reminder, "reminder")?;
    filter.has_tags = bool_pair(args.has_tags, args.no_tags, "tags")?;
    filter.has_checklist = bool_pair(args.has_checklist, args.no_checklist, "checklist")?;
    filter.trashed = bool_pair(args.trashed, args.not_trashed, "trashed")?;
    filter.include_closed_project_tasks |= args.closed_projects;
    filter.repeating = bool_pair(args.repeating, args.not_repeating, "repeating")?;
    filter.tags_all.clone_from(&args.tags_all);
    filter.tags_any.clone_from(&args.tags_any);
    filter.tags_none.clone_from(&args.tags_none);
    if let Some(tag) = &args.tag {
        filter.tags_all.push(tag.clone());
    }

    Ok(ItemQuery {
        view,
        filter,
        sort: args
            .sort
            .as_deref()
            .map(parse_sort)
            .transpose()?
            .unwrap_or(SortField::Index),
        order: if args.desc {
            SortOrder::Desc
        } else {
            SortOrder::Asc
        },
        limit: args.limit,
        offset: args.offset,
        include_notes: args.notes,
        include_checklist: args.checklist,
    })
}

fn edit_item(args: &EditArgs) -> Result<EditItem> {
    if args.target.trim().is_empty() {
        return Err(Error::InvalidInput("target cannot be empty".to_string()));
    }
    Ok(EditItem {
        target: args.target.clone(),
        title: args.title.clone(),
        notes: args.notes.clone(),
        prepend_notes: args.prepend_notes.clone(),
        append_notes: args.append_notes.clone(),
        when: args.when.clone(),
        clear_when: args.clear_when,
        deadline: args.deadline.clone(),
        clear_deadline: args.clear_deadline,
        reminder: args.reminder.clone(),
        clear_reminder: args.clear_reminder,
        tags: args.tags.clone(),
        add_tags: args.add_tags.clone(),
        checklist: args.checklist.clone(),
        append_checklist: args.append_checklist.clone(),
        prepend_checklist: args.prepend_checklist.clone(),
        complete_checklist: args.complete_checklist.clone(),
        cancel_checklist: args.cancel_checklist.clone(),
        reopen_checklist: args.reopen_checklist.clone(),
        project: args.project.clone(),
        area: args.area.clone(),
        heading: args.heading.clone(),
        status: args.status.as_deref().map(parse_status).transpose()?,
        duplicate: false,
        reveal: args.reveal,
    })
}

fn run_config(
    command: &ConfigCommand,
    config: &Config,
    explicit_database: Option<&Path>,
    mode: OutputMode,
) -> Result<()> {
    match command {
        ConfigCommand::Path => output(mode, &json!({"path": Config::config_path()})),
        ConfigCommand::Show => {
            let selection = config.select_database(explicit_database)?;
            output(
                mode,
                &json!({
                    "config_path": Config::config_path(),
                    "database": selection.path,
                    "database_source": selection.source_name(),
                    "read_only": config.read_only,
                    "output": config.output,
                    "write": config.write,
                    "mcp": config.mcp,
                }),
            )
        }
    }
}

fn run_doctor(config: &Config, explicit_database: Option<&Path>, mode: OutputMode) -> Result<()> {
    let app = Path::new("/Applications/Things3.app").is_dir();
    let selection = config.select_database(explicit_database);
    let mut database_readable = false;
    let mut schema_version = None;
    let mut counts = None;
    let mut token_source = if config.token().is_some() {
        "config_or_environment"
    } else {
        "none"
    };
    let mut database_error = None;
    let database_path = match &selection {
        Ok(selection) => Some(selection.path.clone()),
        Err(_) => None,
    };

    if let Ok(selection) = &selection {
        match Database::open(&selection.path) {
            Ok(database) => {
                database_readable = true;
                schema_version = database.schema_version()?;
                counts = Some(database.counts()?);
                if config.token().is_none() && database.auth_token()?.is_some() {
                    token_source = "database";
                }
            }
            Err(error) => database_error = Some(error.to_string()),
        }
    } else if let Err(error) = &selection {
        database_error = Some(error.to_string());
    }

    let report = json!({
        "ok": app && database_readable,
        "things_app_installed": app,
        "database": database_path,
        "database_source": selection
            .as_ref()
            .ok()
            .map(things_core::DatabaseSelection::source_name),
        "database_readable": database_readable,
        "database_error": database_error,
        "schema_version": schema_version,
        "token_configured": token_source != "none",
        "token_source": token_source,
        "counts": counts,
        "read_only": config.read_only,
        "config_path": Config::config_path(),
        "full_disk_access_note": "If database_readable is false, grant Full Disk Access to the terminal or Codex app.",
        "url_scheme_note": "Enable Things URLs in Things > Settings > General to allow updates."
    });
    output(mode, &report)
}

fn resolve_mode(cli: &Cli, config: &Config) -> OutputMode {
    if cli.json {
        return OutputMode::Json;
    }
    if let Some(mode) = cli.format {
        return mode;
    }
    match config.output.format.to_ascii_lowercase().as_str() {
        "json" => OutputMode::Json,
        "ndjson" => OutputMode::Ndjson,
        _ => OutputMode::Text,
    }
}

fn output<T: Serialize>(mode: OutputMode, value: &T) -> Result<()> {
    match mode {
        OutputMode::Json => {
            let text = serde_json::to_string_pretty(value)
                .map_err(|error| Error::Internal(error.to_string()))?;
            println!("{text}");
        }
        OutputMode::Ndjson => {
            let text =
                serde_json::to_string(value).map_err(|error| Error::Internal(error.to_string()))?;
            println!("{text}");
        }
        OutputMode::Text => print_text_value(value),
    }
    Ok(())
}

fn output_page(page: &Page<Item>, mode: OutputMode) -> Result<()> {
    match mode {
        OutputMode::Json => output(mode, page),
        OutputMode::Ndjson => {
            for item in &page.items {
                let text = serde_json::to_string(item)
                    .map_err(|error| Error::Internal(error.to_string()))?;
                println!("{text}");
            }
            Ok(())
        }
        OutputMode::Text => {
            println!(
                "{} item(s), showing {} from offset {} (has_more={})",
                page.total,
                page.items.len(),
                page.offset,
                page.has_more
            );
            for item in &page.items {
                println!("{}", format_item(item));
            }
            Ok(())
        }
    }
}

fn output_write(result: &WriteResult, mode: OutputMode) -> Result<()> {
    match mode {
        OutputMode::Json | OutputMode::Ndjson => output(mode, result),
        OutputMode::Text => {
            let sent = if result.sent { "sent" } else { "dry-run" };
            let id = result.id.as_deref().unwrap_or("-");
            println!("{} {}: {} [{}]", sent, result.action, result.title, id);
            println!(
                "verification: {} ({} attempt(s), {} ms)",
                result.verification.verified,
                result.verification.attempts,
                result.verification.elapsed_ms
            );
            for warning in &result.warnings {
                println!("warning: {warning}");
            }
            for item in &result.created {
                let status = if item.confirmed {
                    item.id
                        .as_deref()
                        .map_or_else(|| "confirmed".to_string(), |id| format!("confirmed ({id})"))
                } else {
                    format!(
                        "not confirmed ({})",
                        item.reason.as_deref().unwrap_or("unknown")
                    )
                };
                println!(
                    "{} {}: {} [{status}]",
                    item.path,
                    item.item_type.as_str(),
                    item.title
                );
            }
            for item in &result.updated {
                let status = if item.confirmed {
                    match (item.wanted_status, item.observed_status) {
                        (Some(wanted), Some(observed)) => {
                            format!("confirmed ({} -> {})", wanted.as_str(), observed.as_str())
                        }
                        (Some(wanted), None) => format!("confirmed ({})", wanted.as_str()),
                        _ => "confirmed".to_string(),
                    }
                } else {
                    let observed = item
                        .observed_status
                        .map(|status| format!(", observed={}", status.as_str()))
                        .unwrap_or_default();
                    format!(
                        "not confirmed ({}{observed})",
                        item.reason.as_deref().unwrap_or("unknown")
                    )
                };
                println!(
                    "{} {}: {} [{status}]",
                    item.path,
                    item.item_type.as_str(),
                    item.title
                );
            }
            if !result.sent {
                println!("url: {}", result.url);
            }
            Ok(())
        }
    }
}

fn print_text_value<T: Serialize>(value: &T) {
    let Ok(value) = serde_json::to_value(value) else {
        println!("<unserializable>");
        return;
    };
    if let Ok(items) = serde_json::from_value::<Vec<Item>>(value.clone()) {
        for item in &items {
            println!("{}", format_item(item));
        }
        return;
    }
    if let Ok(tags) = serde_json::from_value::<Vec<Tag>>(value.clone()) {
        for tag in &tags {
            println!(
                "{}  {}{}",
                tag.id,
                tag.title,
                tag.shortcut
                    .as_ref()
                    .map(|value| format!("  shortcut={value}"))
                    .unwrap_or_default()
            );
        }
        return;
    }
    if let Ok(areas) = serde_json::from_value::<Vec<Area>>(value.clone()) {
        for area in &areas {
            println!(
                "{}  {}  projects={} open_tasks={} tags={}",
                area.id,
                area.title,
                area.project_count,
                area.open_task_count,
                area.tags.join(",")
            );
        }
        return;
    }
    if let Ok(headings) = serde_json::from_value::<Vec<Heading>>(value.clone()) {
        for heading in &headings {
            println!("{}  {}", heading.id, heading.title);
        }
        return;
    }
    if let Ok(item) = serde_json::from_value::<Item>(value.clone()) {
        println!("{}", format_item(&item));
        return;
    }
    if let Ok(tag) = serde_json::from_value::<Tag>(value.clone()) {
        println!("{}  {}", tag.id, tag.title);
        return;
    }
    match serde_json::to_string_pretty(&value) {
        Ok(text) => println!("{text}"),
        Err(_) => println!("<unserializable>"),
    }
}

fn format_item(item: &Item) -> String {
    let status = item
        .status()
        .map(|value| format!("/{}", value.as_str()))
        .unwrap_or_default();
    let context = match item {
        Item::Todo(todo) => todo
            .project_title
            .as_ref()
            .map(|value| format!("  project={value}"))
            .unwrap_or_default(),
        Item::Project(project) => {
            format!("  open={}/{}", project.open_task_count, project.task_count)
        }
        Item::Heading(heading) => heading
            .project_title
            .as_ref()
            .map(|value| format!("  project={value}"))
            .unwrap_or_default(),
        Item::Area(_) | Item::Tag(_) => String::new(),
    };
    format!(
        "{}  [{}{}] {}{}",
        item.id(),
        item.item_type().as_str(),
        status,
        item.title(),
        context
    )
}

fn read_import(file: &str) -> Result<String> {
    if file == "-" {
        let mut payload = String::new();
        io::stdin()
            .read_to_string(&mut payload)
            .map_err(Error::Io)?;
        return Ok(payload);
    }
    std::fs::read_to_string(file).map_err(Error::Io)
}

fn parse_view(value: &str) -> Result<View> {
    value.parse()
}

fn parse_status(value: &str) -> Result<ItemStatus> {
    match value.to_ascii_lowercase().as_str() {
        "open" | "incomplete" => Ok(ItemStatus::Open),
        "completed" | "complete" | "done" => Ok(ItemStatus::Completed),
        "canceled" | "cancelled" => Ok(ItemStatus::Canceled),
        _ => Err(Error::InvalidInput(format!(
            "unknown status {value:?}; expected open, completed, or canceled"
        ))),
    }
}

fn parse_item_type(value: &str) -> Result<ItemType> {
    match value.to_ascii_lowercase().as_str() {
        "todo" | "to-do" | "task" => Ok(ItemType::Todo),
        "project" => Ok(ItemType::Project),
        "heading" => Ok(ItemType::Heading),
        other => Err(Error::InvalidInput(format!(
            "unknown type {other:?}; expected todo, project, or heading"
        ))),
    }
}

fn parse_sort(value: &str) -> Result<SortField> {
    match value.to_ascii_lowercase().as_str() {
        "index" => Ok(SortField::Index),
        "title" => Ok(SortField::Title),
        "created" | "created_at" => Ok(SortField::Created),
        "modified" | "modified_at" => Ok(SortField::Modified),
        "start" | "start_date" => Ok(SortField::StartDate),
        "deadline" => Ok(SortField::Deadline),
        "stop" | "stop_date" => Ok(SortField::StopDate),
        _ => Err(Error::InvalidInput(format!("unknown sort field {value:?}"))),
    }
}

fn bool_pair(yes: bool, no: bool, field: &str) -> Result<Option<bool>> {
    match (yes, no) {
        (true, true) => Err(Error::Conflict(format!(
            "--{field} and --no-{field} are mutually exclusive"
        ))),
        (true, false) => Ok(Some(true)),
        (false, true) => Ok(Some(false)),
        (false, false) => Ok(None),
    }
}

fn agent_brief() -> Value {
    json!({
        "identity": "things",
        "purpose": "Read and modify the user's local Things 3 data through a stable, verified interface.",
        "principles": [
            "Reads are SQLite read-only queries; direct database writes are never performed.",
            "Writes use the official Things URL scheme or AppleScript and are verified against SQLite.",
            "Use --json or --format ndjson for machine-readable output.",
            "Use stable ids from list/search/get; 4+ character id prefixes are accepted.",
            "Use --dry-run to inspect the generated URL without changing Things.",
            "Destructive commands still require --yes even when --dry-run is set.",
            "Use `things doctor` when database permissions or URL authentication fail.",
            "Repeating-rule writes are intentionally unsupported; use `things repeat <id>`.",
            "Repeating templates are omitted from ordinary lists and belong to `things repeating`. When an instance and template share a title, title lookups prefer the instance; address the template by id. Explicit `--trashed` listings and the logbook report stored rows literally.",
            "`things empty-trash --yes` is the only permanent delete; headings are read-only.",
            "Deleting an item that is already in Things Trash returns a conflict error; only `things empty-trash --yes` removes it.",
            "For import, read both created and updated per-item results. Mixed-field updates report fields-not-verified even if the status landed; only status-only updates can be fully confirmed. Checklist-item updates support status only.",
            "Import update destinations are preflighted as references, so a valid move elsewhere is accepted. To-dos use list/list-id and heading/heading-id; projects use area/area-id. Unknown ids, empty ids, wrong-type destination fields, and mismatched nested parents fail before sending.",
            "Search before retrying not-found, ambiguous, dry-run, no-verify, or fields-not-verified import entries."
        ],
        "suggested_workflow": [
            "things today --json --limit 20",
            "things search \"term\" --json",
            "things show <id> --json",
            "things edit <id> --deadline 2026-10-20 --json",
            "things done <id> --json",
            "things import payload.json --dry-run --json",
            "things import payload.json --json"
        ],
        "mcp": "Run `things mcp`; configure it as a stdio MCP server."
    })
}

fn print_error_json(
    code: ErrorCode,
    message: &str,
    candidates: Option<&[things_core::error::Candidate]>,
    candidates_truncated: bool,
) {
    let value = json!({
        "error": code.as_str(),
        "message": message,
        "candidates": candidates,
        "candidates_truncated": candidates_truncated,
    });
    if let Ok(text) = serde_json::to_string_pretty(&value) {
        eprintln!("{text}");
    }
}

fn wants_json_output() -> bool {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--json" || arg == "--format=json" || arg == "--format=ndjson" {
            return true;
        }
        if arg == "--format" {
            let value = args.next();
            if value
                .as_deref()
                .is_some_and(|value| matches!(value, "json" | "ndjson"))
            {
                return true;
            }
        }
    }
    false
}

fn exit_code(code: ErrorCode) -> u8 {
    match code {
        ErrorCode::NotFound | ErrorCode::Ambiguous => 4,
        ErrorCode::InvalidInput
        | ErrorCode::InvalidDate
        | ErrorCode::Conflict
        | ErrorCode::Unsupported => 2,
        ErrorCode::Permission | ErrorCode::AuthRequired | ErrorCode::AppUnavailable => 3,
        ErrorCode::VerificationFailed => 5,
        ErrorCode::Database | ErrorCode::Internal => 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflicting_presence_filters_are_rejected() {
        assert!(bool_pair(true, true, "deadline").is_err());
    }

    #[test]
    fn parse_aliases_are_stable() {
        assert_eq!(parse_status("done").unwrap(), ItemStatus::Completed);
        assert_eq!(parse_item_type("to-do").unwrap(), ItemType::Todo);
        assert_eq!(parse_sort("start_date").unwrap(), SortField::StartDate);
    }
}
