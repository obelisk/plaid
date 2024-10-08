use std::collections::HashMap;

use super::Github;
use crate::apis::{github::GitHubError, ApiError};

impl Github {
    /// Search for files with given name across a GH organization and return the result.
    /// See https://docs.github.com/en/rest/search/search?apiVersion=2022-11-28#search-code for more details
    pub async fn search_file_in_org_code(
        &self,
        params: &str,
        module: &str,
    ) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        let org = self.validate_org(request.get("org").ok_or(ApiError::BadRequest)?)?;
        let filename =
            self.validate_filename(request.get("filename").ok_or(ApiError::BadRequest)?)?;
        // See if we were told to look only in a particular repository
        let repo = match request.get("repo") {
            Some(r) => Some(self.validate_repository_name(r)?),
            None => None,
        };
        let per_page: u8 = request
            .get("per_page")
            .unwrap_or(&"30")
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

        match repo {
            Some(r) => info!(
                "Finding all files called [{filename}] in repository [{org}/{r}] on behalf of [{module}]"
            ),
            None => info!(
                "Finding all files called [{filename}] in organization [{org}] on behalf of [{module}]"
            )
        }

        // Build the search query
        let query = match repo {
            Some(r) => {
                urlencoding::encode(&format!("{} in:path repo:{}/{}", filename, org, r)).to_string()
            }
            None => urlencoding::encode(&format!("{} in:path org:{}", filename, org)).to_string(),
        };

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
