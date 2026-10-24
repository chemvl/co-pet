//! Windows microphone consent (`…\CapabilityAccessManager\ConsentStore\microphone`): the global
//! switch and Co-pet's own switch, which is the package's entry (`<PackageFamilyName>`) in the
//! MSIX package and the "Let desktop apps access your microphone" switch (`NonPackaged`)
//! otherwise. Pure, so it is tested on every OS; only `win::permissions` reads the registry.
#![cfg_attr(not(windows), allow(dead_code))]

/// `"denied"` when either switch is `Deny`, else `"granted"` (Co-pet gets no in-app prompt).
pub fn microphone_status(global: Option<&str>, app: Option<&str>) -> &'static str {
    let deny = |v: Option<&str>| v.is_some_and(|v| v.eq_ignore_ascii_case("Deny"));
    if deny(global) || deny(app) {
        "denied"
    } else {
        "granted"
    }
}

#[cfg(test)]
mod tests {
    use super::microphone_status;

    #[test]
    fn either_switch_off_denies() {
        assert_eq!(microphone_status(Some("Allow"), Some("Allow")), "granted");
        assert_eq!(microphone_status(None, None), "granted");
        assert_eq!(microphone_status(Some("Deny"), Some("Allow")), "denied");
        assert_eq!(microphone_status(Some("Allow"), Some("deny")), "denied");
    }
}
