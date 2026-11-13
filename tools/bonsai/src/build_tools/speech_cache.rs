//! ASR-only compiler environment for hosted source builds.

use super::{Path, Value, external, github_environment};

pub(super) fn environment(data: &Path, working: &Path) -> crate::Result<Value> {
    let host = std::env::var("NATIVE_SPEECH_HOST_KEY").map_err(|error| error.to_string())?;
    if !crate::identity::is_lower_hex(&host, 64)
        || crate::native_startup::host()?["HostCacheKey"] != host
    {
        return Err("Native preparation requires the checked current host cache identity.".into());
    }
    let native = crate::native_startup::environment(data, working)?;
    for (name, value) in native["Environment"]
        .as_object()
        .ok_or("Native preparation omitted its compiler environment.")?
    {
        github_environment(
            name,
            value
                .as_str()
                .ok_or("Native compiler environment is not text.")?,
        )?;
    }
    let evidence = working.join("SwiftBonsai-NativePreparation");
    external(&evidence)?;
    github_environment("NATIVE_SPEECH_EVIDENCE", &evidence.to_string_lossy())?;
    Ok(native)
}
