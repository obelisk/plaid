use aws_sdk_identitystore::Client;
use serde::Deserialize;

use crate::{apis::ApiError, get_aws_sdk_config, AwsAuthentication};

/// Defines configuration for the Iam API
#[derive(Deserialize)]
pub struct IamConfig {
    /// This can either be:
    /// - `IAM`: Uses the IAM role assigned to the instance or environment.
    /// - `ApiKey`: Uses explicit credentials, including an access key ID, secret access key, and region.
    authentication: AwsAuthentication,
    /// The unique identifier of the Identity Store
    identity_store_id: String,
}

/// Represents the Iam API that handles all requests to IAM
pub struct Iam {
    /// The underlying IAM client used to interact with the IAM API.
    client: Client,
    /// The unique identifier of the Identity Store
    identity_store_id: String,
}

impl Iam {
    /// Creates a new instance of `Iam`
    pub async fn new(config: IamConfig) -> Self {
        let sdk_config = get_aws_sdk_config(config.authentication).await;
        let client = aws_sdk_identitystore::Client::new(&sdk_config);

        Self {
            client,
            identity_store_id: config.identity_store_id,
        }
    }

    /// Retrieve a user's ID given their username.
    #[allow(deprecated)] // for the .filters() call
    async fn get_user_id_by_username(&self, user_name: &str) -> Result<String, ApiError> {
        let filter = aws_sdk_identitystore::types::Filter::builder()
            .attribute_path("UserName")
            .attribute_value(user_name)
            .build()
            .map_err(|e| ApiError::IamError(format!("Could not create filter for user: {e}")))?;

        let user_id = self
            .client
            .list_users()
            .identity_store_id(&self.identity_store_id)
            .filters(filter)
            .send()
            .await
            .map_err(|e| ApiError::IamError(format!("Could not list users with filter: {e}")))?
            .users()
            .first()
            .map(|u| u.user_id().to_string())
            .ok_or(ApiError::IamError("Could not get user ID".to_string()))?;
        Ok(user_id)
    }

    /// Retrieve a group's ID given its name.
    #[allow(deprecated)] // for the .filters() call
    async fn get_group_id_by_name(&self, group_name: &str) -> Result<String, ApiError> {
        let filter = aws_sdk_identitystore::types::Filter::builder()
            .attribute_path("DisplayName")
            .attribute_value(group_name)
            .build()
            .map_err(|e| ApiError::IamError(format!("Could not create filter for group: {e}")))?;

        let group_id = self
            .client
            .list_groups()
            .identity_store_id(&self.identity_store_id)
            .filters(filter)
            .send()
            .await
            .map_err(|e| ApiError::IamError(format!("Could not list groups: {e}")))?
            .groups
            .first()
            .map(|g| g.group_id().to_string())
            .ok_or(ApiError::IamError("Could not get group ID".to_string()))?;
        Ok(group_id)
    }

    /// Create a user in an AWS Identity Store.
    pub async fn create_user(&self, user_name: &str, display_name: &str) -> Result<(), ApiError> {
        self.client
            .create_user()
            .identity_store_id(&self.identity_store_id)
            .user_name(user_name)
            .display_name(display_name)
            .send()
            .await
            .map(|_| ()) // if it's OK, we don't care about the output
            .map_err(|e| ApiError::IamError(format!("Could not create user {user_name}: {e}")))
    }

    /// Delete a user from an AWS Identity Store.
    pub async fn delete_user(&self, user_name: &str) -> Result<(), ApiError> {
        let user_id = self.get_user_id_by_username(user_name).await?;
        self.client
            .delete_user()
            .identity_store_id(&self.identity_store_id)
            .user_id(user_id)
            .send()
            .await
            .map(|_| ()) // if it's OK, we don't care about the output
            .map_err(|e| ApiError::IamError(format!("Could not delete user: {e}")))
    }

    /// Add a user to a group
    pub async fn add_user_to_group(
        &self,
        user_name: &str,
        group_name: &str,
    ) -> Result<(), ApiError> {
        let user_id = self.get_user_id_by_username(user_name).await?;
        let member_id = aws_sdk_identitystore::types::MemberId::UserId(user_id.to_string());
        let group_id = self.get_group_id_by_name(group_name).await?;

        self.client
            .create_group_membership()
            .identity_store_id(&self.identity_store_id)
            .group_id(group_id)
            .member_id(member_id)
            .send()
            .await
            .map(|_| ()) // if it's OK, we don't care about the output
            .map_err(|e| {
                ApiError::IamError(format!(
                    "Could not assign user {user_id} to group {group_name}: {e}"
                ))
            })
    }

    /// Remove a user from a group
    pub async fn remove_user_from_group(
        &self,
        user_name: &str,
        group_name: &str,
    ) -> Result<(), ApiError> {
        let user_id = self.get_user_id_by_username(user_name).await?;
        let group_id = self.get_group_id_by_name(group_name).await?;

        // Get all memberships for the given group
        let memberships = self
            .client
            .list_group_memberships()
            .identity_store_id(&self.identity_store_id)
            .group_id(group_id)
            .send()
            .await
            .map_err(|e| {
                ApiError::IamError(format!(
                    "Could not list group memberships for group {group_name}: {e}"
                ))
            })?
            .group_memberships;

        // Find the user's membership in the given group
        if let Some(membership) = memberships.into_iter().find(|m| {
            m.member_id().map_or(false, |id| {
                id.as_user_id().unwrap_or(&String::new()).as_str() == user_id
            })
        }) {
            let membership_id = membership
                .membership_id()
                .ok_or(ApiError::IamError(format!(
                    "Membership ID missing for user {user_name} in group {group_name}"
                )))?;

            // Finally, delete the user's membership for the given group
            self.client
                .delete_group_membership()
                .identity_store_id(&self.identity_store_id)
                .membership_id(membership_id)
                .send()
                .await
                .map(|_| ()) // if it's OK, we don't care about the output
                .map_err(|e| {
                    ApiError::IamError(format!(
                    "Could not delete memberships for user {user_name} in group {group_name}: {e}"
                ))
                })
        } else {
            // The user is not a member of the given group: do nothing
            return Ok(());
        }
    }
}
