use std::{collections::HashMap, fmt::Display};

use crate::{
    github::{CodeSearchCriteria, FileSearchResult, FileSearchResultItem, RepoFilter},
    PlaidFunctionError,
};

/// Search for code in GitHub.
/// If additional selection criteria are given, these are used to decide whether
/// results are selected or discarded.
///
/// **Arguments:**
/// - `filename`: The name of the files to search, e.g., "README"
/// - `extension`: The extension of the files to search, e.g., "yml"
/// - `path`: The path under which files are searched, e.g., "src"
/// - `search_criteria`: An optional `CodeSearchCriteria` object with additional search criteria
pub fn search_code(
    filename: Option<impl Display>,
    extension: Option<impl Display>,
    path: Option<impl Display>,
    search_criteria: Option<&CodeSearchCriteria>,
) -> Result<Vec<FileSearchResultItem>, PlaidFunctionError> {
    extern "C" {
        new_host_function_with_error_buffer!(github, search_code);
    }

    let mut params: HashMap<&str, String> = HashMap::new();
    if let Some(filename) = filename {
        params.insert("filename", filename.to_string());
    }
    if let Some(extension) = extension {
        params.insert("extension", extension.to_string());
    }
    if let Some(path) = path {
        params.insert("path", path.to_string());
    }

    // If we are given selection criteria, then we divide them between
    //
    // * Those that can be baked directly into the GitHub search query, thus making the overall search more
    // efficient (because less results are returned). These are passed to the API.
    //
    // * Those that have to be (or are better) evaluated module-side. These are not passed to the API and
    // are processed later here.

    if let Some(criteria) = search_criteria {
        if let Some(org) = &criteria.only_from_org {
            // Search only inside an organization
            params.insert("org", org.clone());

            if let Some(RepoFilter::OnlyFromRepos { repos }) = &criteria.repo_filter {
                if repos.len() == 1 {
                    // Special case: search only in a repository
                    params.insert("repo", repos[0].clone());
                }
            }
        }
    }

    let mut search_results = Vec::<FileSearchResultItem>::new();
    let mut page = 0;

    // Use a larger page size to make less requests and reduce chances of hitting the rate limit
    let per_page = 100;
    params.insert("per_page", per_page.to_string());

    const RETURN_BUFFER_SIZE: usize = 1 * 1024 * 1024; // 1 MiB

    loop {
        page += 1;
        params.insert("page", page.to_string());

        let request = serde_json::to_string(&params).unwrap(); // safe unwrap

        let mut return_buffer = vec![0; RETURN_BUFFER_SIZE];

        let res = unsafe {
            github_search_code(
                request.as_bytes().as_ptr(),
                request.as_bytes().len(),
                return_buffer.as_mut_ptr(),
                RETURN_BUFFER_SIZE,
            )
        };

        if res < 0 {
            return Err(res.into());
        }

        return_buffer.truncate(res as usize);
        // This should be safe because unless the Plaid runtime is expressly trying
        // to mess with us, this came from a String in the API module.
        let this_page = String::from_utf8(return_buffer).unwrap();

        let file_search_result = serde_json::from_str::<FileSearchResult>(&this_page)
            .map_err(|_| PlaidFunctionError::InternalApiError)?;

        if file_search_result.items.is_empty() {
            break; // we are past the last page
        }

        // Number of items we got on this page
        let received_page_size = file_search_result.items.len();

        search_results.extend(file_search_result.items);

        // If we did not fill this page, we know there won't be a next one.
        // So we can stop here and save one API call.
        if received_page_size < per_page {
            break;
        }
    }

    // Now that all the search results have been collected, apply the module-side selection criteria.
    if let Some(search_criteria) = search_criteria {
        Ok(filter_search_results(search_results, search_criteria))
    } else {
        // No criteria have been passed
        Ok(search_results)
    }
}

/// Filter results returned by GitHub search API by applying a set of search criteria
pub fn filter_search_results(
    raw_results: Vec<FileSearchResultItem>,
    search_criteria: &CodeSearchCriteria,
) -> Vec<FileSearchResultItem> {
    let mut filtered_results = Vec::<FileSearchResultItem>::new();
    let regex_dot_folder = regex::Regex::new(r"\/\.").unwrap(); // Right now, no way around recompiling this regex

    // Go through all the results and try to discard them by applying the criteria.
    // If the result makes it to the end, then add it to the filtered results.
    for result in raw_results {
        // Discard files in . folders
        if search_criteria.discard_results_in_dot_folders {
            if regex_dot_folder.is_match(&result.html_url) {
                continue;
            }
        }
        // Select / discard files based on the repo name. This _could_ be done in the query, but
        // there is a limit on how many AND / OR / NOT operators can be used. So we keep it here.
        if let Some(RepoFilter::NotFromRepos { repos }) = &search_criteria.repo_filter {
            if repos
                .iter()
                .find(|v| **v == result.repository.name)
                .is_some()
            {
                continue;
            }
        }
        if let Some(RepoFilter::OnlyFromRepos { repos }) = &search_criteria.repo_filter {
            if repos
                .iter()
                .find(|v| **v == result.repository.name)
                .is_none()
            {
                continue;
            }
        }
        // Discard files based on the repo's visibility
        if search_criteria.discard_results_in_private_repos && result.repository.private {
            continue;
        }
        // Discard files based on a substring in the path
        if let Some(sub_paths) = &search_criteria.discard_substrings {
            let mut discarded = false;
            for subp in sub_paths {
                if result.html_url.contains(subp) {
                    discarded = true;
                    break; // inner loop
                }
            }
            if discarded {
                continue;
            }
        }
        // Discard files based on explicit list
        if let Some(discard_explicit) = &search_criteria.discard_specific_files {
            // build the string we will search for
            let search = format!("{}/{}", result.repository.full_name, result.path);

            if discard_explicit.iter().find(|v| **v == search).is_some() {
                continue;
            }
        }

        // If we are here, we have not discarded the result
        filtered_results.push(result);
    }
    filtered_results
}
