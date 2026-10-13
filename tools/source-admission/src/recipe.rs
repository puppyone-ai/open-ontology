//! Registration recipe. This states the published consumer contract.
//! It does not insert an allowlist entry, raise a driver quota, or issue a lease.

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const REGISTRATION_RECIPE_PROTOCOL: &str = "oo.source-admission.registration-recipe/v1";

/// `approved_admission_bin` in the committed consumer source.
/// This crate does not edit that function.
const PUBLISHED_ADMISSION_BINS: &[&str] = &[
    "a205eb5797f69d2c95e4e1cc533fce5642be5424df7669fb8a49fef4026eeaf2",
    "ae7dc727181ae101bab84e328d39c38c3fdd6613255f79062404cbc9c8069577",
    "e465241fc1292b6eb5a13cf12efbffe360a9fbf1cbb03c02affb0241a4e58e1f",
];

/// Exact `(digest, byte length)` pairs `APPROVED_DRIVER_RESOURCES` names.
/// Any other file above [`OPEN_SERVER_REGISTRATION_MAX_BYTES`] stays refused.
const PUBLISHED_DRIVER_RESOURCES: &[(&str, u64)] = &[
    (
        "d5dc8555ad76ecc9d4550ac5d2abd6b00bb2fee6ca5a17c7c206d294b206010c",
        115_155_008,
    ),
    (
        "874d2588595d63b597139358b47e3097fcc5f45f772ffb5000ecd371e8e88b2e",
        116_863_584,
    ),
];

/// Byte cap of committed `open_server_registration` / `read_driver` for an unlisted digest.
pub const OPEN_SERVER_REGISTRATION_MAX_BYTES: u64 = 32 * 1024 * 1024;

pub struct RegistrationRecipe {
    pub validator_sha256: String,
    pub validator_bytes: u64,
    pub driver_sha256: String,
    pub driver_bytes: u64,
    pub approved_by_consumer: bool,
    pub driver_quota: &'static str,
}

pub fn consumer_published(digest: &str) -> bool {
    PUBLISHED_ADMISSION_BINS.contains(&digest)
}

pub fn quota_for(digest: &str, len: u64) -> &'static str {
    if len == 0 {
        return "empty";
    }
    if let Some((_, declared)) = PUBLISHED_DRIVER_RESOURCES
        .iter()
        .find(|(item, _)| *item == digest)
    {
        return if len == *declared {
            "approved_driver_resource"
        } else {
            "exceeds_open_server_registration"
        };
    }
    if len > OPEN_SERVER_REGISTRATION_MAX_BYTES {
        "exceeds_open_server_registration"
    } else {
        "within_open_server_registration"
    }
}

pub fn registration_recipe(validator: &[u8], driver: &[u8]) -> RegistrationRecipe {
    let validator_sha256 = sha256_hex(validator);
    let driver_sha256 = sha256_hex(driver);
    let driver_bytes = driver.len() as u64;
    RegistrationRecipe {
        approved_by_consumer: consumer_published(&validator_sha256),
        validator_sha256,
        validator_bytes: validator.len() as u64,
        driver_quota: quota_for(&driver_sha256, driver_bytes),
        driver_sha256,
        driver_bytes,
    }
}

pub fn recipe_json(recipe: &RegistrationRecipe) -> Vec<u8> {
    let value = json!({
        "protocol": REGISTRATION_RECIPE_PROTOCOL,
        "trusted": false,
        "allowlist_updated": false,
        "approved_by_consumer": recipe.approved_by_consumer,
        "validator_sha256": recipe.validator_sha256,
        "validator_bytes": recipe.validator_bytes,
        "driver_sha256": recipe.driver_sha256,
        "driver_bytes": recipe.driver_bytes,
        "driver_quota": recipe.driver_quota,
        "quota_owner": "DL01-W1-06",
        "receipt_registry_sha256": "preimage",
        "oo_native_binding_registry_sha256": "full_file",
        "grade_source": "capturer",
        "self_written_grade_accepted": false,
        "lease_issued": false,
        "lease_owner": "DL01-W1-06"
    });
    let mut bytes = serde_json::to_vec_pretty(&value).unwrap_or_else(|_| Vec::new());
    if bytes.is_empty() {
        bytes = serde_json::to_vec(&Value::Null).unwrap_or_default();
    }
    bytes.push(b'\n');
    bytes
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
