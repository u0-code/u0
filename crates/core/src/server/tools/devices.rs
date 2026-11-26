//! Ghost's tools for the simulators and emulators of the machine the
//! server runs on (`devices`), as the Devices tab has them: listed, their
//! screen read, and (asking you first, as every tool that acts does)
//! started, stopped, touched, swiped, scrolled, turned and typed into.

use base64::Engine as _;
use rmcp::{
    handler::server::wrapper::Parameters, model::ContentBlock, schemars, tool, tool_router,
};
use serde::{Deserialize, Serialize};

use super::{Tools, off, said};
use crate::devices::{self, Act, Device};

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct Named {
    /// The device, by its id as `list_devices` gives it.
    device: String,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
struct UseDevice {
    /// The device, by its id as `list_devices` gives it.
    device: String,
    /// What to do to it; a tap is in the pixels of its screen as
    /// `device_screen` shows it.
    #[serde(flatten)]
    act: Act,
}

/// The device here whose id is `id`.
async fn found(id: String) -> Result<Device, String> {
    let (found, _) = off(devices::list).await?;
    (found.into_iter().find(|d| d.id == id))
        .ok_or_else(|| format!("No device here is {id}: list_devices says which are"))
}

#[tool_router(router = device_router, vis = "pub(super)")]
impl Tools {
    /// The iOS simulators and Android emulators where Ghost runs: each
    /// one's id, name, whether it is Android and whether it runs; and what
    /// is missing to have any.
    #[tool]
    async fn list_devices(&self) -> Result<String, String> {
        let (found, missing) = off(devices::list).await?;
        said(&serde_json::json!({"found": found, "missing": missing}))
    }

    /// What a running device shows now, as a picture.
    #[tool]
    async fn device_screen(
        &self,
        Parameters(ask): Parameters<Named>,
    ) -> Result<ContentBlock, String> {
        let device = found(ask.device).await?;
        let picture = off(move || devices::screen(&device)).await??;
        let kind = if picture.starts_with(b"\x89PNG") {
            "image/png"
        } else {
            "image/jpeg"
        };
        let data = base64::engine::general_purpose::STANDARD.encode(picture);
        Ok(ContentBlock::image(data, kind))
    }

    /// Start or stop a device; touch a point of its screen, hold it (a
    /// long press), swipe from one point to another, or scroll toward a
    /// side; turn it left or right; type into it, press its Home key (an
    /// Android one's Back or Recents), or set its appearance dark or light.
    #[tool]
    async fn use_device(&self, Parameters(ask): Parameters<UseDevice>) -> Result<String, String> {
        let device = found(ask.device.clone()).await?;
        let title = format!("Use the device {}", device.name);
        self.allowed("use_device", title, &ask).await?;
        let act = ask.act;
        off(move || devices::act(&device, &act)).await??;
        Ok("Done.".into())
    }
}
