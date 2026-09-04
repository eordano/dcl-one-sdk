use serde::Serialize;

/// The `{ "ok": true, "data": ... }` success envelope every upstream Decentraland
/// service wraps its 200 bodies in.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ApiOk<T> {
    pub ok: bool,
    pub data: T,
}

impl<T> ApiOk<T> {
    pub fn new(data: T) -> Self {
        Self { ok: true, data }
    }

    pub fn ok(data: T) -> Self {
        Self::new(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_serializes_ok_before_data() {
        assert_eq!(
            serde_json::to_string(&ApiOk::new(vec![1, 2])).unwrap(),
            r#"{"ok":true,"data":[1,2]}"#
        );
    }

    #[test]
    fn both_constructors_produce_the_same_envelope() {
        let new = serde_json::to_value(ApiOk::new("x")).unwrap();
        let ok = serde_json::to_value(ApiOk::ok("x")).unwrap();
        assert_eq!(new, ok);
        assert_eq!(new, serde_json::json!({ "ok": true, "data": "x" }));
    }
}
