use std::fmt::Display;

use serde_json::{json, Value};

/// Takes a string-like description and turns it into the JSON format that Jira expects
fn create_simple_jira_description(description: impl Display) -> Value {
    serde_json::json!({
        "version":1,
        "type":"doc",
        "content":[
            {
                "type":"paragraph",
                "content":[
                    {
                        "type":"text",
                        "text":description.to_string()
                    }
                ]
            }
        ]
    })
}

impl super::CreateIssueRequest {
    pub fn to_payload(&self) -> Value {
        let mut fields = json!({
            "project": { "key": self.project_key },
            "summary": self.summary,
            "description": create_simple_jira_description(&self.description),
        });

        if let Some(name) = &self.issuetype_name {
            fields["issuetype"] = json!({ "name": name });
        }

        // Merge any extra fields
        for (k, v) in &self.other_fields {
            fields[k] = v.clone();
        }

        json!({ "fields": fields })
    }
}

impl super::PostCommentRequest {
    pub fn to_payload(&self) -> Value {
        json!({
          "body": {
            "type": "doc",
            "version": 1,
            "content": [
              {
                "type": "paragraph",
                "content": [
                  {
                    "text": self.comment,
                    "type": "text"
                  }
                ]
              }
            ]
          }
        })
    }
}

impl super::UpdateIssueRequest {
    pub fn to_payload(&self) -> Value {
        // When we are here, we know that at least one between `fields` and `update` is Some
        match (&self.fields, &self.update) {
            (Some(f), Some(u)) => {
                json!({
                    "fields": f,
                    "update": u
                })
            }
            (Some(f), None) => {
                json!({
                    "fields": f
                })
            }
            (None, Some(u)) => {
                json!({
                    "update": u
                })
            }
            _ => unreachable!(
                "Both fields and update are missing: this should not have passed validation!"
            ),
        }
    }
}
