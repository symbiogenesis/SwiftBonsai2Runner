//! Publish complete settings bytes through a synced, create-new sibling file.

use std::{
    fs::{self, OpenOptions},
    io::{self, Write as _},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

// Serialization, encryption and storage limits belong to the individual stores.
pub(super) fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    bonsai_windows_credentials::validate_external_path(path)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let temporary = path.with_extension(format!("{}.{stamp}.pending", std::process::id()));
    bonsai_windows_credentials::validate_external_path(&temporary)?;
    let mut created = false;
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        created = true;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        bonsai_windows_credentials::validate_external_path(path)?;
        fs::rename(&temporary, path)
    })();
    // A create-new collision belongs to somebody else and must survive cleanup.
    if result.is_err() && created {
        let _removed = fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt as _;

    #[test]
    fn settings_replace_preserves_previous_bytes_and_cleans_up_after_failed_publication()
    -> io::Result<()> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "BonsaiAtomicSettings-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&directory)?;
        let path = directory.join("settings.json");
        replace(&path, b"original")?;
        replace(&path, b"complete replacement")?;
        assert_eq!(fs::read(&path)?, b"complete replacement");
        // Deny delete sharing so publication fails after the sibling is written.
        let retained = OpenOptions::new().read(true).share_mode(1).open(&path)?;
        assert!(replace(&path, b"must not replace the retained file").is_err());
        assert_eq!(fs::read(&path)?, b"complete replacement");
        assert_eq!(fs::read_dir(&directory)?.count(), 1);
        drop(retained);
        fs::remove_file(path)?;
        fs::remove_dir(directory)
    }
}
