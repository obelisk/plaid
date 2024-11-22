mod groups;
mod users;

use jwt_simple::{
    claims::Claims,
    prelude::{Duration as JwtDuration, RS256KeyPair, RSAKeyPairLike},
};
use reqwest::Client;
use serde::{de, Deserialize, Serialize};

use std::{string::FromUtf8Error, time::Duration};

use super::default_timeout_seconds;

#[derive(Deserialize)]
#[serde(untagged)]
enum Authentication {
    ApiKey {
        token: String,
    },
    OktaApp {
        client_id: String,
        #[serde(deserialize_with = "private_key_deserializer")]
        private_key: RS256KeyPair,
    },
}

fn private_key_deserializer<'de, D>(deserializer: D) -> Result<RS256KeyPair, D::Error>
where
    D: de::Deserializer<'de>,
{
    let pem_key = String::deserialize(deserializer)?;
    Ok(RS256KeyPair::from_pem(&pem_key)
        .map_err(|_| de::Error::custom("Could not deserialize app's private key")))?
}

#[derive(Deserialize)]
pub struct OktaConfig {
    /// The Okta domain to run queries against
    pub domain: String,
    /// How the authentication to Okta is made
    authentication: Authentication,
    /// The number of seconds until an external API request times out.
    /// If no value is provided, the result of `default_timeout_seconds()` will be used.
    #[serde(default = "default_timeout_seconds")]
    api_timeout_seconds: u64,
}

pub struct Okta {
    config: OktaConfig,
    client: Client,
}

#[derive(Debug)]
pub enum OktaError {
    BadData(FromUtf8Error),
    UnexpectedStatusCode(u16),
    AuthenticationFailure,
    BadPrivateKey,
    JwtSignatureFailure,
    BadJsonResponse,
}

/// Which operation we will execute through the API. This is used to determine
/// the OAuth 2.0 scope to include in the request for an access token.
pub enum OktaOperation {
    GetUserInfo,
    RemoveUserFromGroup,
}

impl OktaOperation {
    /// Get the correct Okta OAuth 2.0 scope, depending on which operation needs to be performed.
    pub fn to_okta_scope(&self) -> &str {
        match self {
            // https://developer.okta.com/docs/api/openapi/okta-management/management/tag/User/#tag/User/operation/getUser
            Self::GetUserInfo => "okta.users.read",
            // https://developer.okta.com/docs/api/openapi/okta-management/management/tag/Group/#tag/Group/operation/unassignUserFromGroup
            Self::RemoveUserFromGroup => "okta.groups.manage",
        }
    }
}

impl Okta {
    pub fn new(config: OktaConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.api_timeout_seconds))
            .build()
            .unwrap();

        Self { config, client }
    }

    /// Return an appropriate authorization header, to be used when making a REST call.
    pub async fn get_authorization_header(&self, op: &OktaOperation) -> Result<String, OktaError> {
        match &self.config.authentication {
            Authentication::ApiKey { token } => Ok(format!("SSWS {}", token)),
            Authentication::OktaApp {
                client_id,
                private_key,
            } => {
                let access_token = self.get_access_token(op, client_id, private_key).await?;
                Ok(format!("Bearer {}", access_token))
            }
        }
    }

    /// Construct a JWT signed with Plaid's private key, to be later exchanged for an access token.
    fn get_jwt(&self, client_id: &str, private_key: &RS256KeyPair) -> Result<String, OktaError> {
        // For more details, see https://developer.okta.com/docs/guides/implement-oauth-for-okta-serviceapp/main/#create-and-sign-the-jwt
        let claims = Claims::create(JwtDuration::from_secs(30))
            .with_issuer(client_id)
            .with_audience(format!("https://{}/oauth2/v1/token", self.config.domain))
            .with_subject(client_id);
        private_key
            .sign(claims)
            .map_err(|_| OktaError::JwtSignatureFailure)
    }

    /// Obtain an access token from Okta, in exchange for a properly constructed JWT.
    async fn get_access_token(
        &self,
        op: &OktaOperation,
        client_id: &str,
        private_key: &RS256KeyPair,
    ) -> Result<String, OktaError> {
        // For more details, see https://developer.okta.com/docs/guides/implement-oauth-for-okta-serviceapp/main/#get-an-access-token
        #[derive(Serialize)]
        struct Form<'a> {
            grant_type: &'a str,
            scope: &'a str,
            client_assertion_type: &'a str,
            client_assertion: &'a str,
        }

        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct AccessTokenResponse {
            token_type: String,
            expires_in: u32,
            access_token: String,
            scope: String,
        }

        let form = Form {
            grant_type: "client_credentials",
            scope: op.to_okta_scope(),
            client_assertion_type: "urn:ietf:params:oauth:client-assertion-type:jwt-bearer",
            client_assertion: &self.get_jwt(client_id, private_key)?,
        };
        let req = self
            .client
            .post(format!("https://{}/oauth2/v1/token", self.config.domain))
            .header("Accept", "application/json")
            .form(&form);
        let res = req
            .send()
            .await
            .map_err(|_| OktaError::AuthenticationFailure)?;
        let access_token = res
            .json::<AccessTokenResponse>()
            .await
            .map_err(|_| OktaError::BadJsonResponse)?
            .access_token;
        Ok(access_token)
    }
}
