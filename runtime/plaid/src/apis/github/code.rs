use std::{collections::HashMap, sync::Arc};

use super::Github;
use crate::{
    apis::{github::GitHubError, ApiError},
    loader::PlaidModule,
};

impl Github {
    /// Search for files with given name.
    /// See https://docs.github.com/en/rest/search/search?apiVersion=2022-11-28#search-code for more details
    pub async fn search_code(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        let request: HashMap<&str, &str> =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        /*
        The request can contain the following parameters:
        - filename
        - extension
        - path
        - org
        - repo
        NOTE - All params are optional and we do not do any validation on whether the request as a whole makes sense.
        We simply validate individual parameters if present.
        */

        let filename = match request.get("filename") {
            None => String::new(),
            Some(filename) => format!("filename:{}", self.validate_filename(filename)?),
        };

        let extension = match request.get("extension") {
            None => String::new(),
            Some(extension) => format!("extension:{}", self.validate_extension(extension)?),
        };

        let path = match request.get("path") {
            None => String::new(),
            Some(path) => format!("path:{}", self.validate_path(path)?),
        };

        // See if we were told to search only in a specific organization or repository
        let org = match request.get("org") {
            None => None,
            Some(org) => Some(self.validate_org(org)?),
        };
        let repo = match request.get("repo") {
            None => None,
            Some(repo) => Some(self.validate_repository_name(repo)?),
        };

        // Assemble org and repo depending on what we received.
        // NOTE - If we received both, we need to set repo:{org}/{repo} in the query
        let org_and_repo = match (org, repo) {
            (None, None) => String::new(),
            (Some(org), Some(repo)) => format!("repo:{org}/{repo}"),
            (Some(org), _) => format!("org:{org}"),
            (_, Some(repo)) => format!("repo:{repo}"),
        };

        let per_page: u8 = request
            .get("per_page")
            .unwrap_or(&"100")
            .parse::<u8>()
            .map_err(|_| ApiError::BadRequest)?;
        if per_page > 100 {
            // GitHub supports up to 100 results per page
            return Err(ApiError::BadRequest);
        }

        let page: u16 = request
            .get("page")
            .unwrap_or(&"1")
            .parse::<u16>()
            .map_err(|_| ApiError::BadRequest)?;

        // Construct the query with the piece we have. Multiple spaces, if present, do not cause problems.
        let query = format!("{filename} {extension} {path} {org_and_repo}");

        // Log what we are doing
        info!("Searching code in GH with query [{query}] on behalf of [{module}]");

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
