use super::Github;
use crate::apis::{github::GitHubError, ApiError};
use serde::{Deserialize, Serialize};

use std::collections::HashMap;

impl Github {
    // List all seats in the Copilot subscription for an organization
    // See https://docs.github.com/en/rest/copilot/copilot-user-management?apiVersion=2022-11-28#list-all-copilot-seat-assignments-for-an-organization for more details
    pub async fn list_seats_in_org_copilot(&self, params: &str, module: &str) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let organization = self.validate_org(request.get("org").ok_or(ApiError::BadRequest)?)?;
        let per_page: u8 = request.get("per_page").unwrap_or(&"50")
            .parse::<u8>().map_err(|_| ApiError::BadRequest)?;
        let page: u16 = request.get("page").unwrap_or(&"1")
            .parse::<u16>().map_err(|_| ApiError::BadRequest)?;

        info!("List seats in Copilot subscription for org {organization}");

        let address = format!("/orgs/{organization}/copilot/billing/seats?page={page}&per_page={per_page}");

        match self.make_generic_get_request(address, &module).await {
            Ok((status, Ok(body))) => {
                if status == 200 {
                    Ok(body)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Ok((_, Err(e))) => Err(e),
            Err(e) => Err(e),
        }
    }

    // Add users to the Copilot subscription for an organization
    // See https://docs.github.com/en/rest/copilot/copilot-user-management?apiVersion=2022-11-28#add-users-to-the-copilot-subscription-for-an-organization for more details
    pub async fn add_users_to_org_copilot(&self, params: &str, module: &str) -> Result<u32, ApiError> {
        #[derive(Deserialize, Serialize)]
        struct Request {
            org: String,
            selected_usernames: Vec<String>,
        }
        let request: Request =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let organization = self.validate_org(&request.org)?;
        for username in &request.selected_usernames {
            self.validate_username(&username)?;
        }

        info!("Adding users {:?} to Copilot subscription for org {organization} as module {module}", request.selected_usernames);

        let address = format!("/orgs/{organization}/copilot/billing/selected_users");

        match self.make_generic_post_request(address, &request, &module).await {
            Ok((status, Ok(_))) => {
                if status == 201 {
                    Ok(0)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Ok((_, Err(e))) => Err(e),
            Err(e) => Err(e),
        }
    }

    // Remove users from the Copilot subscription for an organization
    // See https://docs.github.com/en/rest/copilot/copilot-user-management?apiVersion=2022-11-28#remove-users-from-the-copilot-subscription-for-an-organization for more details
    pub async fn remove_users_from_org_copilot(&self, params: &str, module: &str) -> Result<u32, ApiError> {
        #[derive(Deserialize, Serialize)]
        struct Request {
            org: String,
            selected_usernames: Vec<String>,
        }
        let request: Request =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let organization = self.validate_org(&request.org)?;
        for username in request.selected_usernames.iter() {
            self.validate_username(&username)?;
        }

        info!("Remove users {:?} from Copilot subscription for org {organization}", request.selected_usernames);

        let address = format!("/orgs/{organization}/copilot/billing/selected_users");

        match self.make_generic_delete_request(address, Some(&request), &module).await {
            Ok((status, Ok(_))) => {
                if status == 200 {
                    Ok(0)
                } else {
                    Err(ApiError::GitHubError(GitHubError::UnexpectedStatusCode(
                        status,
                    )))
                }
            }
            Ok((_, Err(e))) => Err(e),
            Err(e) => Err(e),
        }
    }
}
