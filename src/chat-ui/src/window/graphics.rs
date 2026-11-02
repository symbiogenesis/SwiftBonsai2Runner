//! Retain bounded native renderer attribution in the launcher's stderr evidence.

use serde_json::json;
use std::io::Write as _;

const MAX_FIELD_CHARS: usize = 2_048;

fn bounded(value: &str) -> String {
    value.chars().take(MAX_FIELD_CHARS).collect()
}

fn emit(value: &serde_json::Value) {
    let mut stderr = std::io::stderr().lock();
    let _written = serde_json::to_writer(&mut stderr, value)
        .and_then(|()| stderr.write_all(b"\n").map_err(serde_json::Error::io));
    let _flushed = stderr.flush();
}

pub(super) fn install(context: &eframe::CreationContext<'_>) {
    let Some(render) = &context.wgpu_render_state else {
        return;
    };
    let adapter = render.adapter.get_info();
    emit(&json!({
        "kind": "bonsai-chat-graphics", "stage": "adapter-selected",
        "backend": format!("{:?}", adapter.backend),
        "deviceType": format!("{:?}", adapter.device_type),
        "vendor": adapter.vendor, "device": adapter.device,
        "name": bounded(&adapter.name), "driver": bounded(&adapter.driver),
        "driverInfo": bounded(&adapter.driver_info),
    }));
    // wgpu reports DeviceLost separately from uncaptured errors. Its default
    // callback uses logging, which a windows-subsystem application may not have
    // configured. Retain the actual reason before egui's later staging panic.
    // This changes no backend selection and never retries a failed frame/model.
    render.device.set_device_lost_callback(|reason, message| {
        if reason != wgpu::DeviceLostReason::Destroyed {
            emit(&json!({
                "kind": "bonsai-chat-graphics", "stage": "device-lost",
                "reason": format!("{reason:?}"), "message": bounded(&message),
            }));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphics_diagnostic_fields_bound_unicode_without_breaking_json() -> std::io::Result<()> {
        let value = bounded(&"é\n\"".repeat(MAX_FIELD_CHARS));
        assert_eq!(value.chars().count(), MAX_FIELD_CHARS);
        let encoded = serde_json::to_string(&json!({"message": value}))?;
        assert_eq!(encoded.lines().count(), 1);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&encoded)?["message"],
            value
        );
        Ok(())
    }
}
