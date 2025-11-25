use serde_json::{json, Value};

impl super::CreateIssueRequest {
    pub fn to_payload(&self) -> Value {
        let mut fields = json!({
            "project": { "key": self.project_key },
            "summary": self.summary,
            "description": self.description,
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
    pub fn to_payload(&self) -> Result<Value, String> {
        // Serialize to JSON, if at least one between `fields` and `update` is Some.
        // Otherwise return an error because Jira won't accept it anyway.
        match (&self.fields, &self.update) {
            (Some(f), Some(u)) => Ok(json!({
                "fields": f,
                "update": u
            })),
            (Some(f), None) => Ok(json!({
                "fields": f
            })),
            (None, Some(u)) => Ok(json!({
                "update": u
            })),
            _ => Err("Both fields and update are missing".to_string()),
        }
    }
}
