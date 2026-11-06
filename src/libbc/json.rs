use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use std::{any::type_name, panic::Location};

/// Decode external JSON without including potentially sensitive payloads in errors.
#[track_caller]
pub(crate) fn decode<T: DeserializeOwned>(body: &[u8], source: &str) -> Result<T> {
    let caller = Location::caller();
    serde_json::from_slice(body).with_context(|| {
        format!(
            "failed to decode {source} as {} ({} bytes; at {}:{}:{}):{body:?}",
            type_name::<T>(),
            body.len(),
            caller.file(),
            caller.line(),
            caller.column()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct Response {
        band_id: i64,
    }

    #[test]
    fn decodes_valid_response() {
        let response: Response = decode(br#"{"band_id":42}"#, "test API").unwrap();
        assert_eq!(response.band_id, 42);
    }

    #[test]
    fn missing_field_preserves_cause_and_identifies_source_model_and_caller() {
        let body = br#"{"secret":"must-not-appear"}"#;
        let call_line = line!() + 1;
        let error = decode::<Response>(
            body,
            "Bandcamp Discover API (https://bandcamp.com/api/discover/1/discover_web)",
        )
        .unwrap_err();
        let report = format!("{error:#}");
        assert!(report.contains("Bandcamp Discover API"));
        assert!(report.contains("https://bandcamp.com/api/discover/1/discover_web"));
        assert!(report.contains(type_name::<Response>()));
        assert!(report.contains(&format!("{}:{call_line}:", file!())));
        assert!(report.contains("missing field `band_id`"));
        assert!(report.contains("line 1 column"));
        assert!(!report.contains("must-not-appear"));
        assert!(error.downcast_ref::<serde_json::Error>().is_some());
    }

    #[test]
    fn rejects_invalid_json_and_trailing_data() {
        for body in [b"not json".as_slice(), br#"{"band_id":42} {}"#] {
            let error = decode::<Response>(body, "test API").unwrap_err();
            assert!(error.downcast_ref::<serde_json::Error>().is_some());
            assert!(format!("{error:#}").contains("test API"));
        }
    }
}
