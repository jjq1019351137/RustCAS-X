use super::cloud189::{self, Cloud189Adapter};
use super::quark::{self, QuarkAdapter};
use super::traits::{CloudDriveAdapter, DriveTypeMeta};
use serde_json::Value;

pub fn drive_types() -> Vec<DriveTypeMeta> {
    vec![quark::meta(), cloud189::meta()]
}

pub fn create_adapter(
    drive_type: &str,
    config: Option<&Value>,
    cookie: Option<&str>,
) -> anyhow::Result<Box<dyn CloudDriveAdapter>> {
    match drive_type {
        "quark" => Ok(Box::new(QuarkAdapter::new(config, cookie))),
        "cloud189" => Ok(Box::new(Cloud189Adapter::new(config, cookie))),
        other => anyhow::bail!("unsupported drive_type: {other}"),
    }
}

pub fn serialize_cookie(drive_type: &str, config: &Value) -> String {
    match drive_type {
        "cloud189" => {
            let a = Cloud189Adapter::new(Some(config), None);
            a.cookie().to_string()
        }
        _ => {
            let a = QuarkAdapter::new(Some(config), None);
            a.cookie().to_string()
        }
    }
}
