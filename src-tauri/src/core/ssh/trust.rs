use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustDecision {
    Trusted,
    NeedsApproval,
    Changed,
}

pub fn decide(stored: Option<&str>, observed: &str) -> TrustDecision {
    match stored {
        None => TrustDecision::NeedsApproval,
        Some(value) if value == observed => TrustDecision::Trusted,
        Some(_) => TrustDecision::Changed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_host_requires_approval() {
        assert_eq!(decide(None, "SHA256:new"), TrustDecision::NeedsApproval);
    }

    #[test]
    fn matching_host_is_trusted() {
        assert_eq!(decide(Some("SHA256:x"), "SHA256:x"), TrustDecision::Trusted);
    }

    #[test]
    fn changed_host_is_blocked() {
        assert_eq!(
            decide(Some("SHA256:old"), "SHA256:new"),
            TrustDecision::Changed
        );
    }
}
