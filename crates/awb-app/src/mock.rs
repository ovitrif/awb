//! Mock pairing, to check the pairing screens and transitions without a phone.
//!
//! `AWB_MOCK_PAIRING=success` or `AWB_MOCK_PAIRING=failure` swaps the mDNS and
//! adb pairing flow for a scripted one that feeds the same pairing events to
//! the app: a QR code, then a simulated scan after a few seconds (`:seconds`
//! overrides the default, e.g. `success:1.5`), the pairing progress steps, and
//! finally success, which lists a mock phone, or a failure message.

use std::sync::OnceLock;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outcome {
    Success,
    Failure,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MockPairing {
    pub outcome: Outcome,
    pub scan_after: Duration,
}

pub const PHONE_NAME: &str = "Pixel 9 Pro (mock)";
pub const PHONE_ENDPOINT: &str = "192.168.1.42:41235";

const DEFAULT_SCAN_AFTER: Duration = Duration::from_secs(3);

/// The mock requested through `AWB_MOCK_PAIRING`, read once per process.
pub fn pairing() -> Option<MockPairing> {
    static MOCK: OnceLock<Option<MockPairing>> = OnceLock::new();
    *MOCK.get_or_init(|| parse(&std::env::var("AWB_MOCK_PAIRING").ok()?))
}

fn parse(value: &str) -> Option<MockPairing> {
    let (outcome, seconds) = value
        .split_once(':')
        .map_or((value, None), |(outcome, seconds)| (outcome, Some(seconds)));
    let outcome = match outcome.trim() {
        "success" => Outcome::Success,
        "failure" => Outcome::Failure,
        _ => return None,
    };
    let scan_after = match seconds {
        Some(seconds) => Duration::from_secs_f64(seconds.trim().parse().ok()?),
        None => DEFAULT_SCAN_AFTER,
    };
    Some(MockPairing {
        outcome,
        scan_after,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_outcome_and_optional_scan_delay() {
        assert_eq!(
            parse("success"),
            Some(MockPairing {
                outcome: Outcome::Success,
                scan_after: DEFAULT_SCAN_AFTER,
            })
        );
        assert_eq!(
            parse("failure:1.5"),
            Some(MockPairing {
                outcome: Outcome::Failure,
                scan_after: Duration::from_millis(1500),
            })
        );
        assert_eq!(parse("maybe"), None);
        assert_eq!(parse("success:soon"), None);
    }
}
