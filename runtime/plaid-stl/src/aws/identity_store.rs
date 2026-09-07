use serde::{Deserialize, Serialize};

use crate::PlaidFunctionError;

/// Request payload for creating a user in an AWS Identity Store.
#[derive(Deserialize, Serialize)]
pub struct CreateUserRequest {
    /// The username of the user to create.
    pub user_name: String,
    /// The display name of the user to create.
    pub display_name: String,
}

/// Request payload for deleting a user from an AWS Identity Store.
#[derive(Deserialize, Serialize)]
pub struct DeleteUserRequest {
    /// The username of the user to delete.
    pub user_name: String,
}

/// Request payload for adding a user to a group in an AWS Identity Store.
#[derive(Deserialize, Serialize)]
pub struct AddUserToGroupRequest {
    /// The username of the user to add to the group.
    pub user_name: String,
    /// The display name of the group to add the user to.
    pub group_name: String,
}

/// Request payload for removing a user from a group in an AWS Identity Store.
#[derive(Deserialize, Serialize)]
pub struct RemoveUserFromGroupRequest {
    /// The username of the user to remove from the group.
    pub user_name: String,
    /// The display name of the group to remove the user from.
    pub group_name: String,
}

/// Creates a user in the configured AWS Identity Store.
/// See <https://docs.aws.amazon.com/singlesignon/latest/IdentityStoreAPIReference/API_CreateUser.html> for full documentation.
///
/// # Arguments
///
/// * `user_name` - The username of the user to create.
/// * `display_name` - The display name of the user to create.
pub fn create_user(user_name: &str, display_name: &str) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(aws_identity_store, create_user);
    }

    let request = CreateUserRequest {
        user_name: user_name.to_string(),
        display_name: display_name.to_string(),
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::InternalApiError)?;

    let res = unsafe { aws_identity_store_create_user(request.as_ptr(), request.len()) };

    if res < 0 {
        Err(res.into())
    } else {
        Ok(())
    }
}

/// Deletes a user from the configured AWS Identity Store.
/// See <https://docs.aws.amazon.com/singlesignon/latest/IdentityStoreAPIReference/API_DeleteUser.html> for full documentation.
///
/// # Arguments
///
/// * `user_name` - The username of the user to delete.
pub fn delete_user(user_name: &str) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(aws_identity_store, delete_user);
    }

    let request = DeleteUserRequest {
        user_name: user_name.to_string(),
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::InternalApiError)?;

    let res = unsafe { aws_identity_store_delete_user(request.as_ptr(), request.len()) };

    if res < 0 {
        Err(res.into())
    } else {
        Ok(())
    }
}

/// Adds a user to a group in the configured AWS Identity Store.
/// See <https://docs.aws.amazon.com/singlesignon/latest/IdentityStoreAPIReference/API_CreateGroupMembership.html> for full documentation.
///
/// # Arguments
///
/// * `user_name` - The username of the user to add to the group.
/// * `group_name` - The display name of the group to add the user to.
pub fn add_user_to_group(user_name: &str, group_name: &str) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(aws_identity_store, add_user_to_group);
    }

    let request = AddUserToGroupRequest {
        user_name: user_name.to_string(),
        group_name: group_name.to_string(),
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::InternalApiError)?;

    let res = unsafe { aws_identity_store_add_user_to_group(request.as_ptr(), request.len()) };

    if res < 0 {
        Err(res.into())
    } else {
        Ok(())
    }
}

/// Removes a user from a group in the configured AWS Identity Store.
///
/// This is idempotent: if the user is not a member of the group, the call succeeds without
/// making any change.
/// See <https://docs.aws.amazon.com/singlesignon/latest/IdentityStoreAPIReference/API_DeleteGroupMembership.html> for full documentation.
///
/// # Arguments
///
/// * `user_name` - The username of the user to remove from the group.
/// * `group_name` - The display name of the group to remove the user from.
pub fn remove_user_from_group(
    user_name: &str,
    group_name: &str,
) -> Result<(), PlaidFunctionError> {
    extern "C" {
        new_host_function!(aws_identity_store, remove_user_from_group);
    }

    let request = RemoveUserFromGroupRequest {
        user_name: user_name.to_string(),
        group_name: group_name.to_string(),
    };

    let request =
        serde_json::to_string(&request).map_err(|_| PlaidFunctionError::InternalApiError)?;

    let res = unsafe { aws_identity_store_remove_user_from_group(request.as_ptr(), request.len()) };

    if res < 0 {
        Err(res.into())
    } else {
        Ok(())
    }
}
