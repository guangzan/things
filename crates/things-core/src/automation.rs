use std::process::Command;

use crate::error::{Error, Result};

/// Escape a string for an `AppleScript` string literal.
#[must_use]
pub fn applescript_string(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

/// Open a Things URL in the background. The URL is passed as an argv element,
/// never through a shell.
pub fn open_url(url: &str, foreground: bool) -> Result<()> {
    let mut command = Command::new("open");
    if !foreground {
        command.arg("-g");
    }
    command.arg(url);
    run(command, "opening Things URL")
}

/// Execute a generated `AppleScript` program.
pub fn run_applescript(script: &str, context: &str) -> Result<()> {
    let mut command = Command::new("osascript");
    command.args(["-e", script]);
    run(command, context)
}

/// Run an `AppleScript` program and return stdout.
pub fn run_applescript_output(script: &str, context: &str) -> Result<String> {
    let output = Command::new("osascript")
        .args(["-e", script])
        .output()
        .map_err(|error| Error::AppUnavailable(format!("{context}: {error}")))?;
    if !output.status.success() {
        return Err(Error::AppUnavailable(format!(
            "{context}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn run(mut command: Command, context: &str) -> Result<()> {
    let output = command
        .output()
        .map_err(|error| Error::AppUnavailable(format!("{context}: {error}")))?;
    if !output.status.success() {
        return Err(Error::AppUnavailable(format!(
            "{context}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

/// Wrap one statement body in the standard `tell`/timeout envelope.
fn applescript_program(body: &str, timeout_seconds: u32) -> String {
    format!(
        "tell application \"Things3\"\nwith timeout of {timeout_seconds} seconds\n{body}\nend timeout\nend tell"
    )
}

/// Create a tag through Things' `AppleScript` dictionary and return its id.
///
/// The id lets callers describe the tag even when post-write verification is
/// disabled and the database has not caught up yet.
pub fn create_tag(name: &str) -> Result<String> {
    run_applescript_output(&create_tag_script(name), &format!("creating tag {name:?}"))
}

fn create_tag_script(name: &str) -> String {
    applescript_program(
        &format!(
            "set _new to (make new tag with properties {{name:{}}})\nreturn id of _new",
            applescript_string(name)
        ),
        30,
    )
}

/// Rename a tag by id.
pub fn rename_tag(id: &str, name: &str) -> Result<()> {
    run_applescript(
        &rename_tag_script(id, name),
        &format!("renaming tag {id:?}"),
    )
}

fn rename_tag_script(id: &str, name: &str) -> String {
    applescript_program(
        &format!(
            "set name of tag id {} to {}",
            applescript_string(id),
            applescript_string(name)
        ),
        30,
    )
}

/// Delete a tag by id. Things moves it to its Trash.
pub fn delete_tag(id: &str) -> Result<()> {
    run_applescript(&delete_tag_script(id), &format!("deleting tag {id:?}"))
}

fn delete_tag_script(id: &str) -> String {
    applescript_program(&format!("delete tag id {}", applescript_string(id)), 30)
}

/// Delete a to-do or project by id, including items already in the Logbook.
pub fn delete_item(id: &str, item_type: &str) -> Result<()> {
    let class = match item_type {
        "project" => "project",
        "todo" | "to-do" | "task" => "to do",
        other => {
            return Err(Error::Unsupported(format!(
                "AppleScript delete is not supported for {other}"
            )));
        }
    };
    run_applescript(
        &delete_item_script(id, class),
        &format!("deleting {item_type} {id:?}"),
    )
}

fn delete_item_script(id: &str, class: &str) -> String {
    if class == "to do" {
        // `to do id` resolves an item whatever its list, including the
        // Logbook and the Trash, which a collection query
        // (`every to do whose id = ...`) does not. The query also failed
        // silently when it found nothing, so a delete of an item Things no
        // longer showed reported success without deleting anything.
        applescript_program(
            &format!(
                "try\nset _target to to do id {}\non error\nerror \"to-do not found\"\nend try\ndelete _target",
                applescript_string(id)
            ),
            60,
        )
    } else {
        applescript_program(&format!("delete project id {}", applescript_string(id)), 60)
    }
}

/// Create an area and return its id.
pub fn create_area(name: &str) -> Result<String> {
    run_applescript_output(
        &create_area_script(name),
        &format!("creating area {name:?}"),
    )
}

fn create_area_script(name: &str) -> String {
    applescript_program(
        &format!(
            "set _new to (make new area with properties {{name:{}}})\nreturn id of _new",
            applescript_string(name)
        ),
        30,
    )
}

/// Rename an area by id.
pub fn rename_area(id: &str, name: &str) -> Result<()> {
    run_applescript(
        &rename_area_script(id, name),
        &format!("renaming area {id:?}"),
    )
}

fn rename_area_script(id: &str, name: &str) -> String {
    applescript_program(
        &format!(
            "set name of area id {} to {}",
            applescript_string(id),
            applescript_string(name)
        ),
        30,
    )
}

/// Delete an area by id. Things moves it to its Trash.
pub fn delete_area(id: &str) -> Result<()> {
    run_applescript(&delete_area_script(id), &format!("deleting area {id:?}"))
}

fn delete_area_script(id: &str) -> String {
    applescript_program(&format!("delete area id {}", applescript_string(id)), 30)
}

/// Permanently empty the Things Trash.
pub fn empty_trash() -> Result<()> {
    run_applescript(&empty_trash_script(), "emptying Things Trash")
}

fn empty_trash_script() -> String {
    applescript_program("empty trash", 60)
}

/// Set an item's status through `AppleScript`. This is the only reliable route
/// for reopening a completed or canceled item.
pub fn set_status(id: &str, item_type: &str, status: &str) -> Result<()> {
    let script = status_script(id, item_type, status)?;
    run_applescript(&script, &format!("setting {item_type} {id:?} to {status}"))
}

fn status_script(id: &str, item_type: &str, status: &str) -> Result<String> {
    let class = match item_type {
        "project" => "project",
        "todo" | "to-do" | "task" => "to do",
        other => {
            return Err(Error::Unsupported(format!(
                "status changes are not supported for {other}"
            )));
        }
    };
    let status = match status {
        "open" => "open",
        "completed" => "completed",
        "canceled" | "cancelled" => "canceled",
        other => {
            return Err(Error::InvalidInput(format!(
                "unknown status {other:?}; expected open, completed, or canceled"
            )));
        }
    };
    let script = if class == "to do" {
        // `to do id` resolves an item whatever its list: open, in the
        // Logbook, or in the Trash. A collection query
        // (`every to do whose id = ...`) does not: it only walks the to-dos
        // Things still shows, so it found nothing for a completed item and
        // reopening one failed with "to-do not found".
        applescript_program(
            &format!(
                "try\nset _target to to do id {}\non error\nerror \"to-do not found\"\nend try\nset status of _target to {status}",
                applescript_string(id)
            ),
            30,
        )
    } else {
        applescript_program(
            &format!(
                "set status of project id {} to {status}",
                applescript_string(id)
            ),
            30,
        )
    };
    Ok(script)
}

/// Reveal an item or built-in list in Things.
pub fn show_in_things(id: &str, background: bool) -> Result<()> {
    open_url(&crate::url_scheme::show(Some(id), None, None)?, !background)
}

/// Open the official Things repeat editor. Repeating rules are intentionally
/// not written through `SQLite`.
pub fn open_repeat_editor(id: &str) -> Result<()> {
    show_in_things(id, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_applescript_strings() {
        assert_eq!(applescript_string("a\"b\\c\n"), "\"a\\\"b\\\\c\\n\"");
    }

    #[test]
    fn status_script_reaches_logbook_todos() {
        let script = status_script("todo-1", "todo", "open").unwrap();
        // A collection query misses the Logbook, so the lookup must be by id.
        assert!(script.contains("to do id \"todo-1\""));
        assert!(script.contains("error \"to-do not found\""));
        assert!(script.contains("set status of _target to open"));
        assert!(!script.contains("every to do"));
    }

    #[test]
    fn status_script_rejects_unknown_status() {
        assert!(status_script("todo-1", "to do", "done").is_err());
    }

    #[test]
    fn delete_script_looks_items_up_by_id() {
        // A collection query finds neither the Logbook nor the Trash, so it
        // could not delete a completed to-do and reported success without
        // deleting anything. The lookup must be by id, and a miss must be an
        // error rather than a silent no-op.
        let script = delete_item_script("todo-1", "to do");
        assert!(script.contains("to do id \"todo-1\""));
        assert!(script.contains("error \"to-do not found\""));
        assert!(script.contains("delete _target"));
        assert!(!script.contains("every to do"));
    }

    /// Compile every generated program with `osacompile`. This never runs the
    /// scripts against Things; it only proves they are syntactically valid
    /// `AppleScript` and resolve against the installed Things dictionary.
    #[test]
    fn generated_applescript_compiles() {
        if !cfg!(target_os = "macos") {
            eprintln!("skipping AppleScript compile test: not macOS");
            return;
        }
        let dictionary =
            std::path::Path::new("/Applications/Things3.app/Contents/Resources/Things.sdef");
        if !dictionary.is_file() {
            eprintln!("skipping AppleScript compile test: Things 3 is not installed");
            return;
        }
        let Ok(directory) = tempfile::tempdir() else {
            eprintln!("skipping AppleScript compile test: no temp directory");
            return;
        };

        let mut programs = vec![
            ("create_tag", create_tag_script("Deep work")),
            ("rename_tag", rename_tag_script("tag-1", "Renamed")),
            ("delete_tag", delete_tag_script("tag-1")),
            ("delete_item_todo", delete_item_script("todo-1", "to do")),
            (
                "delete_item_project",
                delete_item_script("project-1", "project"),
            ),
            ("create_area", create_area_script("Work")),
            ("rename_area", rename_area_script("area-1", "Home")),
            ("delete_area", delete_area_script("area-1")),
            ("empty_trash", empty_trash_script()),
        ];
        for (name, item_type, status) in [
            ("status_todo_open", "todo", "open"),
            ("status_todo_completed", "todo", "completed"),
            ("status_project_canceled", "project", "canceled"),
        ] {
            programs.push((
                name,
                status_script("id-1", item_type, status).expect("status script"),
            ));
        }
        // Quotes, backslashes, and newlines must survive escaping.
        programs.push(("create_tag_escaped", create_tag_script("a\"b\\c\nd")));

        for (name, program) in programs {
            let output_path = directory.path().join(format!("{name}.scpt"));
            let output = Command::new("osacompile")
                .arg("-e")
                .arg(&program)
                .arg("-o")
                .arg(&output_path)
                .output();
            let Ok(output) = output else {
                eprintln!("skipping AppleScript compile test: osacompile is unavailable");
                return;
            };
            assert!(
                output.status.success(),
                "{name} failed to compile: {}\n{program}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
    }
}
