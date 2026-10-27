//! A site that asks for a human check (IMDb, fernsehserien.de, OFDb) cannot be reached by a
//! program. Searches answer "unavailable right now" for it, not a failure with Retry; an
//! ordinary request failure stays a failure.

use cv_app::artwork::stream::{SourceAnswer, UNAVAILABLE};
use cv_providers::ProviderError;

fn request(message: &str) -> ProviderError {
    ProviderError::Request {
        provider: "IMDb".to_owned(),
        message: message.to_owned(),
    }
}

#[test]
fn a_human_check_is_unavailable_and_not_retryable() {
    let error = request("the site asked for a human check (HTTP 202); try again later");
    assert_eq!(
        SourceAnswer::of_error(&error),
        SourceAnswer::Failed {
            reason: UNAVAILABLE.to_owned(),
            retryable: false
        }
    );
    // Also when the site was not asked at all, because it asked a moment ago.
    let paused = request("the site asked for a human check; it is left alone for a few minutes");
    assert!(paused.is_human_check());
}

#[test]
fn another_request_failure_stays_a_failure_to_retry() {
    let error = request("connection reset");
    assert!(matches!(
        SourceAnswer::of_error(&error),
        SourceAnswer::Failed {
            retryable: true,
            ..
        }
    ));
}
