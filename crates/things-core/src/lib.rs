//! Shared Things 3 domain, database, URL-scheme, and automation layer.
//!
//! The database is opened read-only. Mutations are sent to the Things app and
//! then verified against the database; this crate never writes to the live
//! Things `SQLite` file.

pub mod automation;
pub mod config;
pub mod database;
pub mod dates;
pub mod error;
pub mod models;
pub mod query;
pub mod service;
pub mod url_scheme;
pub mod verify;

pub use config::{Config, DatabaseSelection};
pub use database::Database;
pub use error::{Error, Result};
pub use models::{
    Area, ChecklistItem, Heading, Item, ItemStatus, ItemType, Project, StartBucket, Tag, Todo, View,
};
pub use query::{Filter, ItemQuery, SortField, SortOrder};
pub use service::{CreateTodo, EditItem, Service, WriteOptions, WriteResult};
