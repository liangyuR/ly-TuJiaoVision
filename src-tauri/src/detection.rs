use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    backend: &'static str,
    ready: bool,
    message: &'static str,
}

pub fn status() -> EngineStatus {
    EngineStatus {
        backend: "LyFlow",
        ready: false,
        message: "算子库尚未接入",
    }
}
