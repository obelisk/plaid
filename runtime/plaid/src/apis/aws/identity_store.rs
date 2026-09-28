use std::sync::Arc;

use aws_sdk_identitystore::Client;
use plaid_stl::aws::identity_store::{
    AddUserToGroupRequest, CreateUserRequest, DeleteUserRequest, RemoveUserFromGroupRequest,
};
use serde::Deserialize;

use crate::{apis::ApiError, get_aws_sdk_config, loader::PlaidModule, AwsAuthentication};

/// Defines configuration for the Identity Store API
#[derive(Deserialize)]
pub struct IdentityStoreConfig {
    /// This can either be:
    /// - `IAM`: Uses the IAM role assigned to the instance or environment.
    /// - `ApiKey`: Uses explicit credentials, including an access key ID, secret access key, and region.
    authentication: AwsAuthentication,
    /// The unique identifier of the Identity Store
    identity_store_id: String,
}

/// Represents the Identity Store API that handles all requests to the AWS Identity Store
pub struct IdentityStore {
    /// The underlying client used to interact with the AWS Identity Store API.
    client: Client,
    /// The unique identifier of the Identity Store
    identity_store_id: String,
}

impl IdentityStore {
    /// Creates a new instance of `IdentityStore`
    pub async fn new(config: IdentityStoreConfig) -> Self {
        let sdk_config = get_aws_sdk_config(&config.authentication).await;
        let client = aws_sdk_identitystore::Client::new(&sdk_config);

        Self {
            client,
            identity_store_id: config.identity_store_id,
        }
    }

    /// Retrieve a user's ID given their username.
    async fn get_user_id_by_username(&self, user_name: &str) -> Result<String, ApiError> {
        // Resolve the user directly via GetUserId. This replaces the deprecated
        // ListUsers `.filters()` call and, because it returns a single result,
        // sidesteps pagination entirely.
        let unique_attribute = aws_sdk_identitystore::types::UniqueAttribute::builder()
            .attribute_path("UserName")
            .attribute_value(user_name.into())
            .build()
            .map_err(|_| {
                ApiError::IdentityStoreError("Could not build user identifier".to_string())
            })?;
        let alternate_identifier =
            aws_sdk_identitystore::types::AlternateIdentifier::UniqueAttribute(unique_attribute);

        let user_id = self
            .client
            .get_user_id()
            .identity_store_id(&self.identity_store_id)
            .alternate_identifier(alternate_identifier)
            .send()
            .await
            .map_err(|_| ApiError::IdentityStoreError("Could not resolve user ID".to_string()))?
            .user_id()
            .to_string();
        Ok(user_id)
    }

    /// Retrieve a group's ID given its name.
    async fn get_group_id_by_name(&self, group_name: &str) -> Result<String, ApiError> {
        // Resolve the group directly via GetGroupId. This replaces the deprecated
        // ListGroups `.filters()` call and, because it returns a single result,
        // sidesteps pagination entirely.
        let unique_attribute = aws_sdk_identitystore::types::UniqueAttribute::builder()
            .attribute_path("DisplayName")
            .attribute_value(group_name.into())
            .build()
            .map_err(|_| {
                ApiError::IdentityStoreError("Could not build group identifier".to_string())
            })?;
        let alternate_identifier =
            aws_sdk_identitystore::types::AlternateIdentifier::UniqueAttribute(unique_attribute);

        let group_id = self
            .client
            .get_group_id()
            .identity_store_id(&self.identity_store_id)
            .alternate_identifier(alternate_identifier)
            .send()
            .await
            .map_err(|_| ApiError::IdentityStoreError("Could not resolve group ID".to_string()))?
            .group_id()
            .to_string();
        Ok(group_id)
    }

    /// Create a user in an AWS Identity Store.
    pub async fn create_user(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request =
            serde_json::from_str::<CreateUserRequest>(params).map_err(|_| ApiError::BadRequest)?;

        info!(
            "{module} attempting to create user [{}] in the Identity Store",
            request.user_name
        );

        self.client
            .create_user()
            .identity_store_id(&self.identity_store_id)
            .user_name(&request.user_name)
            .display_name(&request.display_name)
            .send()
            .await
            .map_err(|_| {
                ApiError::IdentityStoreError(format!("Could not create user {}", request.user_name))
            })?;

        Ok(0)
    }

    /// Delete a user from an AWS Identity Store.
    pub async fn delete_user(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request =
            serde_json::from_str::<DeleteUserRequest>(params).map_err(|_| ApiError::BadRequest)?;
        let user_id = self.get_user_id_by_username(&request.user_name).await?;

        info!(
            "{module} attempting to delete user [{}] from the Identity Store",
            request.user_name
        );

        self.client
            .delete_user()
            .identity_store_id(&self.identity_store_id)
            .user_id(user_id)
            .send()
            .await
            .map_err(|_| {
                ApiError::IdentityStoreError(format!("Could not delete user {}", request.user_name))
            })?;

        Ok(0)
    }

    /// Add a user to a group
    pub async fn add_user_to_group(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request = serde_json::from_str::<AddUserToGroupRequest>(params)
            .map_err(|_| ApiError::BadRequest)?;
        let user_id = self.get_user_id_by_username(&request.user_name).await?;
        let member_id = aws_sdk_identitystore::types::MemberId::UserId(user_id);
        let group_id = self.get_group_id_by_name(&request.group_name).await?;

        info!(
            "{module} attempting to add user [{}] to group [{}] in the Identity Store",
            request.user_name, request.group_name
        );

        self.client
            .create_group_membership()
            .identity_store_id(&self.identity_store_id)
            .group_id(group_id)
            .member_id(member_id)
            .send()
            .await
            .map_err(|_| {
                ApiError::IdentityStoreError(format!(
                    "Could not assign user {} to group {}",
                    request.user_name, request.group_name
                ))
            })?;

        Ok(0)
    }

    /// Remove a user from a group
    pub async fn remove_user_from_group(
        &self,
        params: &str,
        module: Arc<PlaidModule>,
    ) -> Result<u32, ApiError> {
        let request = serde_json::from_str::<RemoveUserFromGroupRequest>(params)
            .map_err(|_| ApiError::BadRequest)?;
        let user_id = self.get_user_id_by_username(&request.user_name).await?;
        let group_id = self.get_group_id_by_name(&request.group_name).await?;

        info!(
            "{module} attempting to remove user [{}] from group [{}] in the Identity Store",
            request.user_name, request.group_name
        );

        // Page through all memberships of the group, looking for the target user.
        // A group can have more members than fit in a single response, so we must
        // follow the pagination token rather than inspecting only the first page.
        let mut memberships = self
            .client
            .list_group_memberships()
            .identity_store_id(&self.identity_store_id)
            .group_id(group_id)
            .into_paginator()
            .items()
            .send();

        let mut membership_id = None;
        while let Some(membership) = memberships.next().await {
            let membership = membership.map_err(|_| {
                ApiError::IdentityStoreError(format!(
                    "Could not list group memberships for group {}",
                    request.group_name
                ))
            })?;

            let is_target_user = membership
                .member_id()
                .and_then(|id| id.as_user_id().ok())
                .is_some_and(|id| *id == user_id);

            if is_target_user {
                membership_id = membership.membership_id().map(|id| id.to_string());
                break;
            }
        }

        // If the user isn't a member of the group, there is nothing to remove.
        let Some(membership_id) = membership_id else {
            debug!(
                "User {} is not a member of group {}; nothing to remove",
                request.user_name, request.group_name
            );
            return Ok(0);
        };

        // Delete the user's membership for the given group
        self.client
            .delete_group_membership()
            .identity_store_id(&self.identity_store_id)
            .membership_id(membership_id)
            .send()
            .await
            .map_err(|_| {
                ApiError::IdentityStoreError(format!(
                    "Could not remove user {} from group {}",
                    request.user_name, request.group_name
                ))
            })?;

        Ok(0)
    }
}
