use crate::{apis::ApiError, get_aws_sdk_config, loader::PlaidModule, AwsAuthentication};
use aws_sdk_ecr::{
    types::{ImageDetail, Repository},
    Client,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Display, sync::Arc};

/// A request to list repositories
#[derive(Deserialize)]
struct ListRepositoriesRequest {
    /// Registry ID where the repositories reside. If not specified, defaults to the caller's registry.
    registry_id: Option<String>,
    /// Maximum number of repositories to return
    max_results: Option<i32>,
    /// Token for pagination
    next_token: Option<String>,
}

/// A request to list images in a repository
#[derive(Deserialize)]
struct ListImagesRequest {
    /// Registry ID where the repository resides. If not specified, defaults to the caller's registry.
    registry_id: Option<String>,
    /// Name of the repository
    repository_name: String,
    /// Maximum number of images to return
    max_results: Option<i32>,
    /// Token for pagination
    next_token: Option<String>,
}

/// A request to describe images with detailed metadata
#[derive(Deserialize)]
struct DescribeImagesRequest {
    /// Registry ID where the repository resides. If not specified, defaults to the caller's registry.
    registry_id: Option<String>,
    /// Name of the repository
    repository_name: String,
    /// List of image IDs to describe. If not specified, describes all images.
    image_ids: Option<Vec<ImageId>>,
    /// Maximum number of images to return
    max_results: Option<i32>,
    /// Token for pagination
    next_token: Option<String>,
}

/// Represents an image ID consisting of tag and/or digest
#[derive(Deserialize)]
struct ImageId {
    /// Image tag
    image_tag: Option<String>,
    /// Image digest
    image_digest: Option<String>,
}

/// Response containing repository information
#[derive(Serialize)]
struct ListRepositoriesResponse {
    /// List of repositories
    repositories: Vec<RepositoryInfo>,
    /// Token for next page of results
    next_token: Option<String>,
}

/// Simplified repository information
#[derive(Serialize)]
struct RepositoryInfo {
    /// Registry ID
    registry_id: Option<String>,
    /// Repository name
    repository_name: Option<String>,
    /// Repository ARN
    repository_arn: Option<String>,
    /// Repository URI
    repository_uri: Option<String>,
    /// Creation date
    created_at: Option<String>,
}

/// Response containing image information
#[derive(Serialize)]
struct ListImagesResponse {
    /// List of images
    images: Vec<ImageInfo>,
    /// Token for next page of results
    next_token: Option<String>,
}

/// Simplified image information
#[derive(Serialize)]
struct ImageInfo {
    /// Registry ID
    registry_id: Option<String>,
    /// Repository name
    repository_name: Option<String>,
    /// Image digest
    image_digest: Option<String>,
    /// Image tags
    image_tags: Option<Vec<String>>,
    /// Image size in bytes
    image_size_in_bytes: Option<i64>,
    /// Image push date
    image_pushed_at: Option<String>,
}

/// Defines configuration for the ECR API
#[derive(Deserialize)]
pub struct EcrConfig {
    /// Specifies the authentication method for accessing the ECR API.
    ///
    /// This can either be:
    /// - `IAM`: Uses the IAM role assigned to the instance or environment.
    /// - `ApiKey`: Uses explicit credentials, including an access key ID, secret access key, and region.
    authentication: AwsAuthentication,
    /// Configured repositories - maps repository names or ARNs to a list of rules that are allowed to access them
    repository_configuration: HashMap<String, Vec<String>>,
}

/// Represents the ECR API that handles all requests to ECR
pub struct Ecr {
    /// The underlying ECR client used to interact with the ECR API.
    client: Client,
    /// A collection of repository names/ARNs and the rules that are allowed to interact with them
    repository_configuration: HashMap<String, Vec<String>>,
}

impl Ecr {
    /// Creates a new instance of `Ecr`
    pub async fn new(config: EcrConfig) -> Self {
        let sdk_config = get_aws_sdk_config(config.authentication).await;
        let client = aws_sdk_ecr::Client::new(&sdk_config);

        Self {
            client,
            repository_configuration: config.repository_configuration,
        }
    }

    /// List ECR repositories that the module has access to
    pub async fn list_repositories(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        // Parse the request parameters
        let request: ListRepositoriesRequest =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Make the ECR API call
        let mut describe_repos = self.client.describe_repositories();

        if let Some(registry_id) = &request.registry_id {
            describe_repos = describe_repos.registry_id(registry_id);
        }
        if let Some(max_results) = request.max_results {
            describe_repos = describe_repos.max_results(max_results);
        }
        if let Some(next_token) = &request.next_token {
            describe_repos = describe_repos.next_token(next_token);
        }

        let output = describe_repos
            .send()
            .await
            .map_err(|e| ApiError::AwsEcrError(format!("Failed to list repositories: {:?}", e)))?;

        // Filter repositories based on access control
        let mut filtered_repositories = Vec::new();
        
        if let Some(repositories) = output.repositories {
            for repo in repositories {
                if let Some(repo_name) = &repo.repository_name {
                    if self.check_repository_access(&module, repo_name)? {
                        filtered_repositories.push(RepositoryInfo::from_repository(repo));
                    }
                }
            }
        }

        let response = ListRepositoriesResponse {
            repositories: filtered_repositories,
            next_token: output.next_token,
        };

        serde_json::to_string(&response).map_err(|_| ApiError::BadRequest)
    }

    /// List images in a specific ECR repository
    pub async fn list_images(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        // Parse the request parameters
        let request: ListImagesRequest =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Check repository access
        self.check_repository_access(&module, &request.repository_name)?;

        // Make the ECR API call
        let mut list_images = self.client.list_images();

        if let Some(registry_id) = &request.registry_id {
            list_images = list_images.registry_id(registry_id);
        }
        list_images = list_images.repository_name(&request.repository_name);
        
        if let Some(max_results) = request.max_results {
            list_images = list_images.max_results(max_results);
        }
        if let Some(next_token) = &request.next_token {
            list_images = list_images.next_token(next_token);
        }

        let output = list_images
            .send()
            .await
            .map_err(|e| ApiError::AwsEcrError(format!("Failed to list images: {:?}", e)))?;

        // Convert to our response format
        let mut images = Vec::new();
        
        if let Some(image_ids) = output.image_ids {
            for image_id in image_ids {
                images.push(ImageInfo {
                    registry_id: request.registry_id.clone(),
                    repository_name: Some(request.repository_name.clone()),
                    image_digest: image_id.image_digest,
                    image_tags: image_id.image_tag.map(|tag| vec![tag]),
                    image_size_in_bytes: None,
                    image_pushed_at: None,
                });
            }
        }

        let response = ListImagesResponse {
            images,
            next_token: output.next_token,
        };

        serde_json::to_string(&response).map_err(|_| ApiError::BadRequest)
    }

    /// Describe images with detailed metadata
    pub async fn describe_images(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<String, ApiError> {
        // Parse the request parameters
        let request: DescribeImagesRequest =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;

        // Check repository access
        self.check_repository_access(&module, &request.repository_name)?;

        // Make the ECR API call
        let mut describe_images = self.client.describe_images();

        if let Some(registry_id) = &request.registry_id {
            describe_images = describe_images.registry_id(registry_id);
        }
        describe_images = describe_images.repository_name(&request.repository_name);
        
        if let Some(image_ids) = &request.image_ids {
            for image_id in image_ids {
                let mut ecr_image_id = aws_sdk_ecr::types::ImageIdentifier::builder();
                if let Some(tag) = &image_id.image_tag {
                    ecr_image_id = ecr_image_id.image_tag(tag);
                }
                if let Some(digest) = &image_id.image_digest {
                    ecr_image_id = ecr_image_id.image_digest(digest);
                }
                describe_images = describe_images.image_ids(ecr_image_id.build());
            }
        }
        
        if let Some(max_results) = request.max_results {
            describe_images = describe_images.max_results(max_results);
        }
        if let Some(next_token) = &request.next_token {
            describe_images = describe_images.next_token(next_token);
        }

        let output = describe_images
            .send()
            .await
            .map_err(|e| ApiError::AwsEcrError(format!("Failed to describe images: {:?}", e)))?;

        // Convert to our response format
        let mut images = Vec::new();
        
        if let Some(image_details) = output.image_details {
            for detail in image_details {
                images.push(ImageInfo::from_image_detail(detail, &request.repository_name));
            }
        }

        let response = ListImagesResponse {
            images,
            next_token: output.next_token,
        };

        serde_json::to_string(&response).map_err(|_| ApiError::BadRequest)
    }

    /// Check if a module has access to a specific repository
    fn check_repository_access<T: Display>(
        &self,
        module: &T,
        repository_name: &str,
    ) -> Result<bool, ApiError> {
        // Check if there's a specific configuration for this repository
        if let Some(allowed_rules) = self.repository_configuration.get(repository_name) {
            if allowed_rules.contains(&module.to_string()) {
                return Ok(true);
            }
        }

        // Check if there's a wildcard configuration that allows access to all repositories
        if let Some(allowed_rules) = self.repository_configuration.get("*") {
            if allowed_rules.contains(&module.to_string()) {
                return Ok(true);
            }
        }

        error!(
            "{module} tried to access ECR repository which it's not allowed to: {repository_name}"
        );
        Err(ApiError::BadRequest)
    }
}

impl RepositoryInfo {
    /// Convert from AWS SDK Repository to our simplified format
    fn from_repository(repo: Repository) -> Self {
        Self {
            registry_id: repo.registry_id,
            repository_name: repo.repository_name,
            repository_arn: repo.repository_arn,
            repository_uri: repo.repository_uri,
            created_at: repo.created_at.map(|dt| dt.to_string()),
        }
    }
}

impl ImageInfo {
    /// Convert from AWS SDK ImageDetail to our simplified format
    fn from_image_detail(detail: ImageDetail, repository_name: &str) -> Self {
        Self {
            registry_id: detail.registry_id,
            repository_name: Some(repository_name.to_string()),
            image_digest: detail.image_digest,
            image_tags: detail.image_tags,
            image_size_in_bytes: detail.image_size_in_bytes,
            image_pushed_at: detail.image_pushed_at.map(|dt| dt.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_check_repository_access_allowed() {
        let mut repo_config = HashMap::new();
        repo_config.insert("test-repo".to_string(), vec!["test-module".to_string()]);
        
        let ecr = Ecr {
            client: aws_sdk_ecr::Client::from_conf(aws_sdk_ecr::Config::new(&aws_config::SdkConfig::builder().build())),
            repository_configuration: repo_config,
        };

        assert!(ecr.check_repository_access(&"test-module", "test-repo").is_ok());
    }

    #[test]
    fn test_check_repository_access_denied() {
        let mut repo_config = HashMap::new();
        repo_config.insert("test-repo".to_string(), vec!["allowed-module".to_string()]);
        
        let ecr = Ecr {
            client: aws_sdk_ecr::Client::from_conf(aws_sdk_ecr::Config::new(&aws_config::SdkConfig::builder().build())),
            repository_configuration: repo_config,
        };

        assert!(ecr.check_repository_access(&"denied-module", "test-repo").is_err());
    }

    #[test]
    fn test_check_repository_access_wildcard() {
        let mut repo_config = HashMap::new();
        repo_config.insert("*".to_string(), vec!["test-module".to_string()]);
        
        let ecr = Ecr {
            client: aws_sdk_ecr::Client::from_conf(aws_sdk_ecr::Config::new(&aws_config::SdkConfig::builder().build())),
            repository_configuration: repo_config,
        };

        assert!(ecr.check_repository_access(&"test-module", "any-repo").is_ok());
    }

    #[test]
    fn test_list_repositories_request_deserialize() {
        let json = r#"{
            "registry_id": "123456789012",
            "max_results": 10,
            "next_token": "token123"
        }"#;

        let request: ListRepositoriesRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.registry_id, Some("123456789012".to_string()));
        assert_eq!(request.max_results, Some(10));
        assert_eq!(request.next_token, Some("token123".to_string()));
    }

    #[test]
    fn test_list_images_request_deserialize() {
        let json = r#"{
            "repository_name": "my-repo",
            "max_results": 5
        }"#;

        let request: ListImagesRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.repository_name, "my-repo");
        assert_eq!(request.max_results, Some(5));
        assert_eq!(request.registry_id, None);
    }
}