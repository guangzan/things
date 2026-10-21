use std::thread::sleep;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::database::Database;
use crate::error::{Error, Result};
use crate::models::{Item, ItemStatus};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Verification {
    pub verified: bool,
    pub attempts: u32,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct VerifyOptions {
    pub timeout: Duration,
    pub poll: Duration,
}

impl Default for VerifyOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(8),
            poll: Duration::from_millis(250),
        }
    }
}

pub fn wait_for<F>(options: VerifyOptions, mut check: F) -> Result<Verification>
where
    F: FnMut() -> Result<Option<String>>,
{
    let started = Instant::now();
    let mut attempts = 0;
    loop {
        attempts += 1;
        if let Some(observed) = check()? {
            return Ok(Verification {
                verified: true,
                attempts,
                elapsed_ms: elapsed_ms(started),
                observed: Some(observed),
            });
        }
        if started.elapsed() >= options.timeout {
            return Ok(Verification {
                verified: false,
                attempts,
                elapsed_ms: elapsed_ms(started),
                observed: None,
            });
        }
        sleep(options.poll);
    }
}

pub fn wait_for_status(
    database: &Database,
    id: &str,
    wanted: ItemStatus,
    options: VerifyOptions,
) -> Result<Verification> {
    wait_for(options, || {
        let item = database.get(id)?;
        let status = match &item {
            Item::Todo(value) => value.status,
            Item::Project(value) => value.status,
            Item::Heading(value) => value.status,
            Item::Area(_) | Item::Tag(_) => {
                return Err(Error::Unsupported(
                    "status verification is only available for tasks and projects".to_string(),
                ));
            }
        };
        Ok((status == wanted).then(|| status.as_str().to_string()))
    })
}

pub fn wait_for_title(
    database: &Database,
    id: &str,
    wanted: &str,
    options: VerifyOptions,
) -> Result<Verification> {
    wait_for(options, || {
        let item = database.get(id)?;
        Ok((item.title() == wanted).then(|| item.title().to_string()))
    })
}

pub fn wait_for_tag(
    database: &Database,
    name: &str,
    present: bool,
    options: VerifyOptions,
) -> Result<Verification> {
    wait_for(options, || {
        let found = database
            .list_tags()?
            .into_iter()
            .any(|tag| tag.title.eq_ignore_ascii_case(name));
        Ok((found == present).then(|| {
            if present {
                "present".to_string()
            } else {
                "absent".to_string()
            }
        }))
    })
}

pub fn wait_for_area(
    database: &Database,
    name: &str,
    present: bool,
    options: VerifyOptions,
) -> Result<Verification> {
    wait_for(options, || {
        let found = database
            .list_areas()?
            .into_iter()
            .any(|area| area.title.eq_ignore_ascii_case(name));
        Ok((found == present).then(|| {
            if present {
                "present".to_string()
            } else {
                "absent".to_string()
            }
        }))
    })
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}
