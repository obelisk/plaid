use std::collections::HashMap;

use super::Github;
use crate::apis::{github::GitHubError, ApiError};

impl Github {
    /// Search for files with given name.
    /// See https://docs.github.com/en/rest/search/search?apiVersion=2022-11-28#search-code for more details
    pub async fn search_for_file(&self, params: &str, module: &str) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let filename =
            self.validate_filename(request.get("filename").ok_or(ApiError::BadRequest)?)?;

        // See if we were told to search only in a specific organization or repository
        let org = match request.get("org") {
            None => None,
            Some(org) => Some(self.validate_org(org)?),
        };
        let repo = match request.get("repo") {
            None => None,
            Some(repo) => Some(self.validate_repository_name(repo)?),
        };

        let per_page: u8 = request
            .get("per_page")
            .unwrap_or(&"100")
            .parse::<u8>()
            .map_err(|_| ApiError::BadRequest)?;
        let page: u16 = request
            .get("page")
            .unwrap_or(&"1")
            .parse::<u16>()
            .map_err(|_| ApiError::BadRequest)?;

        if per_page > 100 {
            // GitHub supports up to 100 results per page
            return Err(ApiError::BadRequest);
        }

        // Log what we are doing
        match org {
            None => info!(
                "Finding all files called [{filename}] on behalf of [{module}]"
            ),
            Some(org) =>  match repo {
                    None => info!(
                        "Finding all files called [{filename}] in organization [{org}] on behalf of [{module}]"
                    ),
                    Some(repo) => info!(
                        "Finding all files called [{filename}] in repository [{org}/{repo}] on behalf of [{module}]"
                    ),
                }
        }

        // Build the search query
        let query = match org {
            None => format!("{filename} in:path"),
            Some(org) => match repo {
                None => format!("{filename} in:path org:{org}"),
                Some(repo) => format!("{filename} in:path repo:{org}/{repo}"),
            },
        };
        let query = urlencoding::encode(&query).to_string();

        // !!! NOTE - This endpoint has a custom rate limitation !!!
        // https://docs.github.com/en/rest/search/search?apiVersion=2022-11-28#rate-limit
        let address = format!("/search/code?q={query}&per_page={per_page}&page={page}");

        match self.make_generic_get_request(address, module).await {
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
}
