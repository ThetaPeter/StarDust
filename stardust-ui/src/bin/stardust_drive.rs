#![windows_subsystem = "windows"]
//! HWND drive / user-dock helper. HTTP stays in slim `fs-viz`.

mod host {
    use std::path::PathBuf;

    use serde_json::{json, Value};

    #[derive(Debug)]
    pub struct ApiError {
        pub status: u16,
        pub body: Value,
    }

    pub type ApiResult = Result<Value, ApiError>;

    impl ApiError {
        pub fn bad(msg: impl Into<String>) -> Self {
            Self {
                status: 400,
                body: json!({ "error": msg.into() }),
            }
        }

        pub fn not_found() -> Self {
            Self {
                status: 404,
                body: json!({ "error": "not found" }),
            }
        }

        pub fn conflict(msg: impl Into<String>) -> Self {
            Self {
                status: 409,
                body: json!({ "error": msg.into() }),
            }
        }

        pub fn internal(msg: impl Into<String>) -> Self {
            Self {
                status: 500,
                body: json!({ "error": msg.into() }),
            }
        }
    }

    pub fn parse_abs(s: &str) -> Result<PathBuf, ApiError> {
        if s.is_empty() {
            return Err(ApiError::bad("missing path"));
        }
        let p = PathBuf::from(s);
        if !p.is_absolute() {
            return Err(ApiError::bad("path must be absolute"));
        }
        Ok(p)
    }
}

#[path = "../desktop_drive.rs"]
mod desktop_drive;

fn main() {
    desktop_drive::run_helper();
}
