//! # Todo API Example
//!
//! A complete CRUD API backed by plaid's persistent storage. Demonstrates how
//! to build a stateful "mini web service" inside a single plaid rule.
//!
//! ## Key concepts
//! - `plaid::storage::insert(key, value)` — create or update a record
//! - `plaid::storage::get(key)` — read a record
//! - `plaid::storage::delete(key)` — remove a record
//! - `plaid::storage::list_keys(prefix)` — list all keys with a given prefix
//! - Using a key prefix (`todo:`) to namespace your data
//!
//! ## Config required
//! ```toml
//! # webhooks.toml
//! [webhooks."local".webhooks."todos"]
//! log_type = "todo_api"
//! headers = ["Content-Type"]
//! logbacks_allowed = { Limited = 0 }
//! ```
//!
//! ## Try it
//! ```sh
//! # Create a todo:
//! curl -s -X POST http://localhost:8080/webhook/todos \
//!   -H "Content-Type: application/json" \
//!   -d '{"action": "create", "id": "1", "title": "Buy milk", "done": false}'
//!
//! # List all todos:
//! curl -s -X POST http://localhost:8080/webhook/todos \
//!   -H "Content-Type: application/json" \
//!   -d '{"action": "list"}'
//!
//! # Get a specific todo:
//! curl -s -X POST http://localhost:8080/webhook/todos \
//!   -H "Content-Type: application/json" \
//!   -d '{"action": "get", "id": "1"}'
//!
//! # Update a todo:
//! curl -s -X POST http://localhost:8080/webhook/todos \
//!   -H "Content-Type: application/json" \
//!   -d '{"action": "update", "id": "1", "title": "Buy oat milk", "done": true}'
//!
//! # Delete a todo:
//! curl -s -X POST http://localhost:8080/webhook/todos \
//!   -H "Content-Type: application/json" \
//!   -d '{"action": "delete", "id": "1"}'
//! ```

use plaid_stl::{entrypoint_with_source_and_response, messages::LogSource, plaid};
use serde::{Deserialize, Serialize};

entrypoint_with_source_and_response!();

/// All todo keys are prefixed with this string so they don't collide
/// with keys from other rules sharing the same storage.
const KEY_PREFIX: &str = "todo:";

#[derive(Deserialize)]
struct Request {
    action: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    done: Option<bool>,
}

#[derive(Serialize, Deserialize)]
struct Todo {
    id: String,
    title: String,
    done: bool,
}

#[derive(Serialize)]
struct ListResponse {
    count: usize,
    todos: Vec<Todo>,
}

fn storage_key(id: &str) -> String {
    format!("{KEY_PREFIX}{id}")
}

fn main(data: String, _source: LogSource) -> Result<Option<String>, i32> {
    let req: Request = match serde_json::from_str(&data) {
        Ok(r) => r,
        Err(e) => return Ok(Some(format!("{{\"error\": \"invalid JSON: {e}\"}}"))),
    };

    let result = match req.action.as_str() {
        "create" | "update" => {
            let id = match &req.id {
                Some(id) => id.clone(),
                None => return Ok(Some("{\"error\": \"missing id\"}".to_string())),
            };
            let todo = Todo {
                id: id.clone(),
                title: req.title.unwrap_or_else(|| "untitled".to_string()),
                done: req.done.unwrap_or(false),
            };
            let json = serde_json::to_vec(&todo).unwrap();
            let _ = plaid::storage::insert(&storage_key(&id), &json);
            plaid::print_debug_string(&format!("[todo-api] {} todo '{}'", req.action, id));
            serde_json::to_string_pretty(&todo).unwrap()
        }

        "get" => {
            let id = match &req.id {
                Some(id) => id.clone(),
                None => return Ok(Some("{\"error\": \"missing id\"}".to_string())),
            };
            match plaid::storage::get(&storage_key(&id)) {
                Ok(bytes) => {
                    // Return the raw stored JSON.
                    String::from_utf8(bytes).unwrap_or_else(|_| "{\"error\": \"corrupt data\"}".to_string())
                }
                Err(_) => format!("{{\"error\": \"todo '{id}' not found\"}}"),
            }
        }

        "delete" => {
            let id = match &req.id {
                Some(id) => id.clone(),
                None => return Ok(Some("{\"error\": \"missing id\"}".to_string())),
            };
            let _ = plaid::storage::delete(&storage_key(&id));
            plaid::print_debug_string(&format!("[todo-api] deleted todo '{id}'"));
            format!("{{\"deleted\": \"{id}\"}}")
        }

        "list" => {
            // list_keys returns all keys matching the given prefix.
            let keys = plaid::storage::list_keys(Some(KEY_PREFIX)).unwrap_or_default();
            let mut todos = Vec::new();
            for key in &keys {
                if let Ok(bytes) = plaid::storage::get(key) {
                    if let Ok(todo) = serde_json::from_slice::<Todo>(&bytes) {
                        todos.push(todo);
                    }
                }
            }
            let response = ListResponse {
                count: todos.len(),
                todos,
            };
            serde_json::to_string_pretty(&response).unwrap()
        }

        other => format!("{{\"error\": \"unknown action: {other}\"}}"),
    };

    Ok(Some(result))
}
