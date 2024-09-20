use crate::apis::ApiError;

use super::{hashes, Npm, NpmError};

use flate2::{write::GzEncoder, Compression};
use plaid_stl::npm::PublishEmptyStubParams;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tar::{Builder, Header};

const REGISTRY_URL: &str = "https://registry.npmjs.org";
const NODE_VERSION: &str = "20.16.0";
const NPM_VERSION: &str = "10.8.3";

#[derive(Serialize)]
struct PkgAttachment<'a> {
    content_type: &'a str,
    data: &'a str,
    length: u64,
}

#[derive(Serialize)]
struct PkgDist<'a> {
    integrity: &'a str,
    shasum: &'a str,
    tarball: &'a str,
}

#[derive(Serialize)]
struct PkgVersion<'a> {
    name: &'a str,
    version: &'a str,
    main: &'a str,
    author: &'a str,
    license: &'a str,
    _id: &'a str,
    readme: &'a str,
    #[serde(rename = "_nodeVersion")]
    _node_version: &'a str,
    #[serde(rename = "_npmVersion")]
    _npm_version: &'a str,
    dist: PkgDist<'a>,
}

#[derive(Serialize)]
struct PkgMetadata<'a> {
    _id: &'a str,
    name: &'a str,
    #[serde(rename = "dist-tags")]
    dist_tags: HashMap<&'a str, &'a str>,
    versions: HashMap<&'a str, PkgVersion<'a>>,
    access: Option<&'a str>,
    _attachments: HashMap<&'a str, PkgAttachment<'a>>,
}

#[derive(Serialize, Deserialize)]
struct PkgManifest<'a> {
    name: &'a str,
    version: &'a str,
    main: &'a str,
    author: &'a str,
    license: &'a str,
    repository: &'a str,
    description: &'a str,
}

impl Npm {
    /// Upload an empty package stub to the npm registry.
    pub async fn publish_empty_stub(&self, params: &str, module: &str) -> Result<i32, ApiError> {
        let params: PublishEmptyStubParams =
            serde_json::from_str(params).map_err(|_| ApiError::BadRequest)?;
        
        info!("Publishing new empty npm package [{}] on behalf of [{module}]", params.package_name);
        
        let (package_json, tarball_data) =
            create_package_tarball(&self.config.npm_scope, &params.package_name)?;
        let data_length = tarball_data.len();
        let sha1_digest = hashes::sha1_hex(&tarball_data);
        let sha512_digest = hashes::sha512_base64(&tarball_data);

        // Safe unwrap: we are hardcoding the json content, so the deserialization will not fail
        let manifest = serde_json::from_str::<PkgManifest>(&package_json).unwrap();

        let scoped_pkg_name = format!("@{}/{}", self.config.npm_scope, params.package_name);

        // Build the (somewhat complex) data structure that npm expects
        let pkg_version_obj = PkgVersion {
            name: &scoped_pkg_name,
            version: manifest.version,
            main: manifest.main,
            author: manifest.author,
            license: manifest.license,
            _id: &format!("{}@{}", &scoped_pkg_name, manifest.version),
            readme: "Empty",
            _node_version: NODE_VERSION,
            _npm_version: NPM_VERSION,
            dist: PkgDist {
                integrity: &format!("sha512-{}", sha512_digest),
                shasum: &sha1_digest,
                tarball: &format!(
                    "{}/{}/-/{}-{}.tgz",
                    REGISTRY_URL, &scoped_pkg_name, &scoped_pkg_name, manifest.version
                ),
            },
        };

        let attachment_name = format!("{}-{}.tgz", &scoped_pkg_name, manifest.version);
        let base64_data = base64::encode(tarball_data);
        let pkg_access_level = params.access_level.to_string();

        let pkg_metadata = PkgMetadata {
            _id: &scoped_pkg_name,
            name: &scoped_pkg_name,
            dist_tags: [("latest", manifest.version)].into(),
            versions: [(manifest.version, pkg_version_obj)].into(),
            access: Some(&pkg_access_level),
            _attachments: [(
                attachment_name.as_str(),
                PkgAttachment {
                    content_type: "application/octet-stream",
                    data: &base64_data,
                    length: data_length
                        .try_into()
                        .map_err(|_| ApiError::NpmError(NpmError::RegistryUploadError))?,
                },
            )]
            .into(),
        };

        // Send everything to the npm registry: this will result in the creation of a new package
        let client = Client::new();
        client
            .put(format!(
                "{}/{}",
                REGISTRY_URL,
                &scoped_pkg_name.replace("/", "%2f")
            ))
            .header("Content-Type", "application/json")
            .header(
                "Authorization",
                format!("Bearer {}", self.config.automation_token),
            )
            .header("npm-auth-type", "web")
            .header("npm-command", "publish")
            .header("user-agent", &self.config.user_agent)
            .json(&pkg_metadata)
            .send()
            .await
            .map(|_| Ok(0))
            .map_err(|_| ApiError::NpmError(NpmError::RegistryUploadError))?
    }
}

/// Helper function. Return a tuple (manifest, tarball) where
/// * **manifest** is the string representation of the npm package's package.json
/// * **tarball** is a bytes vector that encodes a .tar.gz archive that contains the package.json file
///
/// The archive is ready to be uploaded to the NPM registry.
fn create_package_tarball(pkg_scope: &str, pkg_name: &str) -> Result<(String, Vec<u8>), ApiError> {
    let package_json = format!(
        r#"{{
            "name": "@{}/{}",
            "version": "0.0.0",
            "main": "",
            "author": "",
            "license": "ISC",
            "repository": "git://github.com/smartcontractkit/{}.git",
            "description": ""
            }}"#,
        pkg_scope, pkg_name, pkg_name
    );

    let mut header = Header::new_gnu();
    header.set_size(package_json.as_bytes().len().try_into().unwrap()); // safe unwrap: data is hardcoded
    header.set_cksum();

    let mut tar_builder = Builder::new(GzEncoder::new(Vec::new(), Compression::default()));
    tar_builder
        .append_data(&mut header, "package/package.json", package_json.as_bytes())
        .map_err(|_| ApiError::NpmError(NpmError::FailedToGenerateArchive))?;
    tar_builder
        .finish()
        .map_err(|_| ApiError::NpmError(NpmError::FailedToGenerateArchive))?;
    let data = tar_builder
        .into_inner()
        .map_err(|_| ApiError::NpmError(NpmError::FailedToGenerateArchive))?
        .finish()
        .map_err(|_| ApiError::NpmError(NpmError::FailedToGenerateArchive))?;
    Ok((package_json, data))
}
