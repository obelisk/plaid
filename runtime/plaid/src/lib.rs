use std::{collections::HashMap, sync::Arc};

use loader::PlaidModule;

#[macro_use]
extern crate log;

pub mod apis;
pub mod config;
pub mod data;
pub mod executor;
pub mod functions;
pub mod loader;
pub mod logging;
pub mod performance;
pub mod storage;

type ModulesByName = HashMap<String, Arc<PlaidModule>>;
