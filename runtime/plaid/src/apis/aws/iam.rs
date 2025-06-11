use aws_sdk_iam::Client;
use serde::Deserialize;

use crate::{apis::ApiError, get_aws_sdk_config, AwsAuthentication};

/// Defines configuration for the Iam API
#[derive(Deserialize)]
pub struct IamConfig {
    /// This can either be:
    /// - `IAM`: Uses the IAM role assigned to the instance or environment.
    /// - `ApiKey`: Uses explicit credentials, including an access key ID, secret access key, and region.
    authentication: AwsAuthentication,
}

/// Represents the Iam API that handles all requests to IAM
pub struct Iam {
    /// The underlying IAM client used to interact with the IAM API.
    client: Client,
}

impl Iam {
    /// Creates a new instance of `Iam`
    pub async fn new(config: IamConfig) -> Self {
        let sdk_config = get_aws_sdk_config(config.authentication).await;
        let client = aws_sdk_iam::Client::new(&sdk_config);

        Self { client }
    }

    /// Delete an IAM user from an AWS account.
    /// When deleting a user programmatically, AWS requires that we first delete all attached resources,
    /// so this function takes care of that as well.
    /// For more info, see
    /// https://docs.rs/aws-sdk-iam/latest/aws_sdk_iam/struct.Client.html#method.delete_user
    /// https://docs.rs/aws-sdk-iam/latest/aws_sdk_iam/operation/delete_user/builders/struct.DeleteUserFluentBuilder.html
    pub async fn delete_iam_user(&self, username: &str) -> Result<(), ApiError> {
        // Cleanup steps
        try_cleanup("login profile", || {
            delete_login_profile(&self.client, username)
        })
        .await;

        try_cleanup("access keys", || delete_access_keys(&self.client, username)).await;

        try_cleanup("inline policies", || {
            delete_inline_policies(&self.client, username)
        })
        .await;

        try_cleanup("managed policies", || {
            detach_managed_policies(&self.client, username)
        })
        .await;

        try_cleanup("remove user from groups", || {
            remove_user_from_groups(&self.client, username)
        })
        .await;

        try_cleanup("signing certificates", || {
            delete_signing_certificates(&self.client, username)
        })
        .await;

        try_cleanup("MFA devices", || {
            deactivate_and_delete_mfa_devices(&self.client, username)
        })
        .await;

        try_cleanup("SSH public keys", || {
            delete_ssh_public_keys(&self.client, username)
        })
        .await;

        try_cleanup("service-specific credentials", || {
            delete_service_specific_credentials(&self.client, username)
        })
        .await;

        // Finally, delete the user
        match self.client.delete_user().user_name(username).send().await {
            Ok(_) => Ok(()),
            Err(e) => Err(ApiError::IamError(format!(
                "Could not delete IAM user: {e}"
            ))),
        }
    }
}

/// Perform a cleanup step and log if something goes wrong.
/// The exception is when the error string contains "NoSuchEntity", because it means we were trying
/// to delete a resource that does not exist. This is not a real error, so we ignore it and continue.
async fn try_cleanup<F, Fut>(desc: &str, cleanup_fn: F)
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<(), ApiError>>,
{
    match cleanup_fn().await {
        Ok(_) => (),
        Err(ApiError::IamError(e)) => {
            if e.contains("NoSuchEntity") {
                // Do nothing: it means we were trying to remove a resource that does not exist
            } else {
                // This is a real error: log but do not stop. In any case, if we cannot delete the IAM user at the end, we will return an Err
                warn!("Something went wrong while deleting an IAM user from AWS. The cleanup step for \"{desc}\" failed with this error: {e}")
            }
        }
        _ => unreachable!(), // we only return ApiError::IamError
    }
}

/// Remove console login profile
async fn delete_login_profile(client: &Client, username: &str) -> Result<(), ApiError> {
    client
        .delete_login_profile()
        .user_name(username)
        .send()
        .await
        .map_err(|e| ApiError::IamError(format!("Could not delete login profile: {e}")))?;
    Ok(())
}

/// Remove access keys
async fn delete_access_keys(client: &Client, username: &str) -> Result<(), ApiError> {
    let resp = client
        .list_access_keys()
        .user_name(username)
        .send()
        .await
        .map_err(|e| ApiError::IamError(format!("Could not list access keys: {e}")))?;
    for key in resp.access_key_metadata() {
        if let Some(id) = key.access_key_id() {
            client
                .delete_access_key()
                .user_name(username)
                .access_key_id(id)
                .send()
                .await
                .map_err(|e| ApiError::IamError(format!("Could not delete access keys: {e}")))?;
        }
    }
    Ok(())
}

/// Delete inline user policies
async fn delete_inline_policies(client: &Client, username: &str) -> Result<(), ApiError> {
    let resp = client
        .list_user_policies()
        .user_name(username)
        .send()
        .await
        .map_err(|e| ApiError::IamError(format!("Could not list user policies: {e}")))?;
    for name in resp.policy_names() {
        client
            .delete_user_policy()
            .user_name(username)
            .policy_name(name)
            .send()
            .await
            .map_err(|e| ApiError::IamError(format!("Could not delete user policies: {e}")))?;
    }
    Ok(())
}

/// Detach managed policies
async fn detach_managed_policies(client: &Client, username: &str) -> Result<(), ApiError> {
    let resp = client
        .list_attached_user_policies()
        .user_name(username)
        .send()
        .await
        .map_err(|e| ApiError::IamError(format!("Could not list attached user policies: {e}")))?;
    for policy in resp.attached_policies() {
        if let Some(arn) = policy.policy_arn() {
            client
                .detach_user_policy()
                .user_name(username)
                .policy_arn(arn)
                .send()
                .await
                .map_err(|e| {
                    ApiError::IamError(format!("Could not delete attached user policies: {e}"))
                })?;
        }
    }
    Ok(())
}

/// Remove from IAM groups
async fn remove_user_from_groups(client: &Client, username: &str) -> Result<(), ApiError> {
    let resp = client
        .list_groups_for_user()
        .user_name(username)
        .send()
        .await
        .map_err(|e| ApiError::IamError(format!("Could not list user groups: {e}")))?;
    for group in resp.groups() {
        client
            .remove_user_from_group()
            .user_name(username)
            .group_name(group.group_name())
            .send()
            .await
            .map_err(|e| ApiError::IamError(format!("Could not remove user from group: {e}")))?;
    }
    Ok(())
}

/// Delete signing certificates
async fn delete_signing_certificates(client: &Client, username: &str) -> Result<(), ApiError> {
    let resp = client
        .list_signing_certificates()
        .user_name(username)
        .send()
        .await
        .map_err(|e| ApiError::IamError(format!("Could not list signing certificates: {e}")))?;
    for cert in resp.certificates() {
        client
            .delete_signing_certificate()
            .user_name(username)
            .certificate_id(cert.certificate_id())
            .send()
            .await
            .map_err(|e| {
                ApiError::IamError(format!("Could not delete signing certificates: {e}"))
            })?;
    }
    Ok(())
}

/// Deactivate and delete MFA devices
async fn deactivate_and_delete_mfa_devices(
    client: &Client,
    username: &str,
) -> Result<(), ApiError> {
    let resp = client
        .list_mfa_devices()
        .user_name(username)
        .send()
        .await
        .map_err(|e| ApiError::IamError(format!("Could not list MFA devices: {e}")))?;
    for device in resp.mfa_devices() {
        client
            .deactivate_mfa_device()
            .user_name(username)
            .serial_number(device.serial_number())
            .send()
            .await
            .map_err(|e| {
                ApiError::IamError(format!("Could not delete deactivate MFA device: {e}"))
            })?;
    }

    let resp = client
        .list_virtual_mfa_devices()
        .send()
        .await
        .map_err(|e| ApiError::IamError(format!("Could not list virtual MFA devices: {e}")))?;
    for vdevice in resp.virtual_mfa_devices() {
        if let Some(user) = vdevice.user() {
            if user.user_name() == username {
                client
                    .delete_virtual_mfa_device()
                    .serial_number(vdevice.serial_number())
                    .send()
                    .await
                    .map_err(|e| {
                        ApiError::IamError(format!("Could not delete virtual MFA device: {e}"))
                    })?;
            }
        }
    }
    Ok(())
}

/// Delete SSH public keys
async fn delete_ssh_public_keys(client: &Client, username: &str) -> Result<(), ApiError> {
    let resp = client
        .list_ssh_public_keys()
        .user_name(username)
        .send()
        .await
        .map_err(|e| ApiError::IamError(format!("Could not list SSH public keys: {e}")))?;
    for key in resp.ssh_public_keys() {
        client
            .delete_ssh_public_key()
            .user_name(username)
            .ssh_public_key_id(key.ssh_public_key_id())
            .send()
            .await
            .map_err(|e| ApiError::IamError(format!("Could not delete SSH public key: {e}")))?;
    }
    Ok(())
}

/// Delete service-specific credentials
async fn delete_service_specific_credentials(
    client: &Client,
    username: &str,
) -> Result<(), ApiError> {
    let resp = client
        .list_service_specific_credentials()
        .user_name(username)
        .send()
        .await
        .map_err(|e| {
            ApiError::IamError(format!("Could not list service-specific credentials: {e}"))
        })?;
    for cred in resp.service_specific_credentials() {
        client
            .delete_service_specific_credential()
            .user_name(username)
            .service_specific_credential_id(cred.service_specific_credential_id())
            .send()
            .await
            .map_err(|e| {
                ApiError::IamError(format!("Could not delete service-specific credential: {e}"))
            })?;
    }
    Ok(())
}
