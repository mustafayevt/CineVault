//! Updating the indexer service ("Update").
//!
//! The pinned manifest names one version of the indexer service. Because the sites it reaches
//! change often, Settings can also install the newest release. This is the one place where CineVault
//! installs something its manifest does not pin, so it is strict: the release must come from the
//! service's own GitHub project, the file must be the same kind of file as the pinned one, and the
//! release must publish a SHA-256 digest for it. Without a well-formed digest nothing is
//! downloaded; with one, the download goes through the same staging, checksum and unpack steps as
//! any other tool, so a file that does not match is discarded.

use cv_media::tools::PlatformSpec;
use serde_json::Value;

/// Where the newest release of the indexer service is described.
pub const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/Jackett/Jackett/releases/latest";

/// Downloads of a release must come from here.
pub const DOWNLOAD_PREFIX: &str = "https://github.com/Jackett/Jackett/releases/download/";

/// The largest release file CineVault will fetch, in bytes.
pub const MAX_RELEASE_BYTES: u64 = 300 * 1024 * 1024;

/// Why a release was not used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UpdateRefused {
    #[error("The release information could not be read.")]
    Unreadable,
    #[error("The newest release has no download for this computer.")]
    NoDownload,
    #[error(
        "The newest release does not publish a checksum for its download, so it was not installed."
    )]
    NoDigest,
    #[error(
        "The newest release points at a download CineVault does not trust, so it was not installed."
    )]
    UntrustedUrl,
    #[error("The newest release version ({0}) is not one CineVault can compare.")]
    BadVersion(String),
}

/// A newer release, ready to install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The version, without the leading `v`.
    pub version: String,
    /// What to install: the pinned spec with this release's address, size and digest.
    pub spec: PlatformSpec,
}

/// The newest version a release describes (its tag without the `v`).
///
/// # Errors
/// [`UpdateRefused::Unreadable`] or [`UpdateRefused::BadVersion`].
pub fn release_version(release: &Value) -> Result<String, UpdateRefused> {
    let tag = release
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or(UpdateRefused::Unreadable)?;
    let version = tag.trim().trim_start_matches('v');
    if parse_version(version).is_none() {
        return Err(UpdateRefused::BadVersion(version.to_owned()));
    }
    Ok(version.to_owned())
}

/// What to install to move from `installed` to the release, or `None` when the release is not
/// newer. The file is the release asset with the same name as the pinned one, and its address
/// must start with `trusted_prefix` ([`DOWNLOAD_PREFIX`] in the app).
///
/// # Errors
/// [`UpdateRefused`] when the release cannot be trusted or has no matching download.
pub fn plan_update(
    release: &Value,
    pinned: &PlatformSpec,
    installed: &str,
    trusted_prefix: &str,
) -> Result<Option<Plan>, UpdateRefused> {
    let version = release_version(release)?;
    let (Some(new), Some(old)) = (parse_version(&version), parse_version(installed)) else {
        return Err(UpdateRefused::BadVersion(version));
    };
    if new <= old {
        return Ok(None);
    }
    let pinned_artifact = pinned.artifacts.first().ok_or(UpdateRefused::NoDownload)?;
    let wanted = pinned_artifact
        .url
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .ok_or(UpdateRefused::NoDownload)?;
    let asset = release
        .get("assets")
        .and_then(Value::as_array)
        .and_then(|assets| {
            assets
                .iter()
                .find(|asset| asset.get("name").and_then(Value::as_str) == Some(wanted))
        })
        .ok_or(UpdateRefused::NoDownload)?;

    let url = asset
        .get("browser_download_url")
        .and_then(Value::as_str)
        .ok_or(UpdateRefused::NoDownload)?;
    if !url.starts_with(trusted_prefix) || url.contains(['?', '#']) || !url.ends_with(wanted) {
        return Err(UpdateRefused::UntrustedUrl);
    }
    let digest = asset
        .get("digest")
        .and_then(Value::as_str)
        .and_then(|text| text.strip_prefix("sha256:"))
        .filter(|hex| cv_media::tools::is_sha256_hex(hex))
        .ok_or(UpdateRefused::NoDigest)?;
    let size = asset
        .get("size")
        .and_then(Value::as_u64)
        .filter(|size| (1..=MAX_RELEASE_BYTES).contains(size))
        .ok_or(UpdateRefused::NoDownload)?;

    let mut spec = pinned.clone();
    spec.version.clone_from(&version);
    let artifact = spec
        .artifacts
        .first_mut()
        .ok_or(UpdateRefused::NoDownload)?;
    artifact.url = url.to_owned();
    artifact.sha256 = digest.to_owned();
    artifact.size = size;
    Ok(Some(Plan { version, spec }))
}

/// A version as numbers, `0.24.2903` -> `[0, 24, 2903]`. At least two parts, at most four.
#[must_use]
pub fn parse_version(text: &str) -> Option<Vec<u64>> {
    let parts: Vec<u64> = text
        .split('.')
        .map(|part| {
            (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
                .then(|| part.parse().ok())
                .flatten()
        })
        .collect::<Option<_>>()?;
    (2..=4).contains(&parts.len()).then_some(parts)
}

/// Whether version `a` is newer than version `b`. Unreadable versions are never newer.
#[must_use]
pub fn is_newer(a: &str, b: &str) -> bool {
    matches!((parse_version(a), parse_version(b)), (Some(a), Some(b)) if a > b)
}

#[cfg(test)]
mod tests {
    use cv_media::tools::Manifest;
    use cv_media::tools::{Package, Platform};
    use serde_json::json;

    use super::*;

    fn pinned() -> PlatformSpec {
        Manifest::embedded()
            .platform_spec(Package::IndexerService, Platform::WindowsX64)
            .expect("pinned")
            .clone()
    }

    const DIGEST: &str = "abababababababababababababababababababababababababababababababab";

    fn release(tag: &str, digest: Option<&str>) -> Value {
        let mut asset = json!({
            "name": "Jackett.Binaries.Windows.zip",
            "size": 58_000_000,
            "browser_download_url":
                format!("{DOWNLOAD_PREFIX}{tag}/Jackett.Binaries.Windows.zip"),
        });
        if let Some(digest) = digest {
            asset["digest"] = json!(digest);
        }
        json!({
            "tag_name": tag,
            "assets": [
                { "name": "Jackett.Binaries.macOS.tar.gz", "size": 1,
                  "browser_download_url": format!("{DOWNLOAD_PREFIX}{tag}/Jackett.Binaries.macOS.tar.gz"),
                  "digest": format!("sha256:{DIGEST}") },
                asset
            ]
        })
    }

    #[test]
    fn a_newer_release_with_a_digest_becomes_a_plan_for_the_same_kind_of_file() {
        let plan = plan_update(
            &release("v0.24.3000", Some(&format!("sha256:{DIGEST}"))),
            &pinned(),
            "0.24.2903",
            DOWNLOAD_PREFIX,
        )
        .expect("plan")
        .expect("newer");
        assert_eq!(plan.version, "0.24.3000");
        assert_eq!(plan.spec.version, "0.24.3000");
        let artifact = &plan.spec.artifacts[0];
        assert_eq!(
            artifact.url,
            format!("{DOWNLOAD_PREFIX}v0.24.3000/Jackett.Binaries.Windows.zip")
        );
        assert_eq!(artifact.sha256, DIGEST);
        assert_eq!(artifact.size, 58_000_000);
        // Everything else is what the pinned entry says: tree mode, root and program.
        assert_eq!(artifact.format, pinned().artifacts[0].format);
        assert_eq!(artifact.root.as_deref(), Some("Jackett/"));
        assert_eq!(
            artifact.program.as_ref().map(|p| p.path.as_str()),
            Some("JackettConsole.exe")
        );
        plan.spec
            .validate(Package::IndexerService)
            .expect("the plan follows the manifest rules");
    }

    #[test]
    fn a_release_without_a_checksum_is_refused() {
        let refused = plan_update(
            &release("v0.24.3000", None),
            &pinned(),
            "0.24.2903",
            DOWNLOAD_PREFIX,
        );
        assert_eq!(refused, Err(UpdateRefused::NoDigest));
    }

    #[test]
    fn a_checksum_that_is_not_a_sha256_is_refused() {
        for digest in [
            "sha256:",
            "sha256:xyz",
            "sha1:1111111111111111111111111111111111111111",
            "md5:abcdef",
            &format!("sha256:{}", DIGEST.to_uppercase()),
            &format!("sha256:{}0", DIGEST),
            DIGEST,
        ] {
            let refused = plan_update(
                &release("v0.24.3000", Some(digest)),
                &pinned(),
                "0.24.2903",
                DOWNLOAD_PREFIX,
            );
            assert_eq!(refused, Err(UpdateRefused::NoDigest), "{digest}");
        }
    }

    #[test]
    fn a_download_from_anywhere_else_is_refused() {
        for url in [
            "https://example.com/Jackett.Binaries.Windows.zip",
            "http://github.com/Jackett/Jackett/releases/download/v1/Jackett.Binaries.Windows.zip",
            "https://github.com/Other/Jackett/releases/download/v1/Jackett.Binaries.Windows.zip",
            "https://github.com/Jackett/Jackett/releases/download/v1/Jackett.Binaries.Windows.zip?x=1",
        ] {
            let mut release = release("v0.24.3000", Some(&format!("sha256:{DIGEST}")));
            release["assets"][1]["browser_download_url"] = json!(url);
            assert_eq!(
                plan_update(&release, &pinned(), "0.24.2903", DOWNLOAD_PREFIX),
                Err(UpdateRefused::UntrustedUrl),
                "{url}"
            );
        }
    }

    #[test]
    fn a_release_that_is_not_newer_changes_nothing() {
        let digest = format!("sha256:{DIGEST}");
        for installed in ["0.24.3000", "0.24.3001", "0.25.0", "1.0"] {
            assert_eq!(
                plan_update(
                    &release("v0.24.3000", Some(&digest)),
                    &pinned(),
                    installed,
                    DOWNLOAD_PREFIX
                ),
                Ok(None),
                "{installed}"
            );
        }
    }

    #[test]
    fn a_release_with_no_file_for_this_computer_is_refused() {
        let mut other = release("v0.24.3000", Some(&format!("sha256:{DIGEST}")));
        other["assets"] = json!([{ "name": "Jackett.Binaries.LinuxAMDx64.tar.gz", "size": 1,
            "browser_download_url": format!("{DOWNLOAD_PREFIX}v0.24.3000/x") }]);
        assert_eq!(
            plan_update(&other, &pinned(), "0.24.2903", DOWNLOAD_PREFIX),
            Err(UpdateRefused::NoDownload)
        );
        assert_eq!(
            plan_update(&json!({}), &pinned(), "0.24.2903", DOWNLOAD_PREFIX),
            Err(UpdateRefused::Unreadable)
        );
        assert_eq!(
            plan_update(
                &release("nightly", None),
                &pinned(),
                "0.24.2903",
                DOWNLOAD_PREFIX
            ),
            Err(UpdateRefused::BadVersion("nightly".into()))
        );
    }

    #[test]
    fn an_absurd_size_is_refused() {
        for size in [
            json!(0),
            json!(MAX_RELEASE_BYTES + 1),
            json!("big"),
            json!(null),
        ] {
            let mut release = release("v0.24.3000", Some(&format!("sha256:{DIGEST}")));
            release["assets"][1]["size"] = size;
            assert_eq!(
                plan_update(&release, &pinned(), "0.24.2903", DOWNLOAD_PREFIX),
                Err(UpdateRefused::NoDownload)
            );
        }
    }

    #[test]
    fn versions_compare_by_number_not_by_text() {
        assert!(is_newer("0.24.10000", "0.24.2903"));
        assert!(is_newer("0.25.1", "0.24.9999"));
        assert!(!is_newer("0.24.2903", "0.24.2903"));
        assert!(!is_newer("0.9.1", "0.24.2903"));
        assert!(!is_newer("junk", "0.24.1"));
        assert!(!is_newer("1.2", "junk"));
        assert_eq!(parse_version("1.2.3"), Some(vec![1, 2, 3]));
        for bad in ["", "1", "1.2.3.4.5", "1..2", "v1.2", "1.2-beta", "1.-2"] {
            assert_eq!(parse_version(bad), None, "{bad:?}");
        }
    }
}
