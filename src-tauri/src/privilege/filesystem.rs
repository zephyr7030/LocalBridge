use std::fs::{self, OpenOptions};
use std::io::Write as _;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

use super::{PrivilegedFilesystemAction, PrivilegedFilesystemResult, PrivilegedFilesystemSpec};

pub(crate) fn run_privileged_filesystem(
    spec: PrivilegedFilesystemSpec,
) -> Result<PrivilegedFilesystemResult, ()> {
    spec.validate().map_err(|_| ())?;
    let action = spec.action;
    let path = spec.path.clone();
    match action {
        PrivilegedFilesystemAction::ReadFile => {
            let bytes = fs::read(&path).map_err(|_| ())?;
            if bytes.len() > super::MAX_PRIVILEGED_FILE_BYTES {
                return Err(());
            }
            Ok(PrivilegedFilesystemResult {
                action,
                path,
                destination: None,
                content_base64: Some(STANDARD.encode(&bytes)),
                bytes: bytes.len() as u32,
            })
        }
        PrivilegedFilesystemAction::WriteFile => {
            let encoded = spec.content_base64.as_deref().ok_or(())?;
            let bytes = STANDARD.decode(encoded).map_err(|_| ())?;
            if bytes.len() > super::MAX_PRIVILEGED_FILE_BYTES {
                return Err(());
            }
            let mut file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&path)
                .map_err(|_| ())?;
            file.write_all(&bytes).map_err(|_| ())?;
            file.flush().map_err(|_| ())?;
            Ok(PrivilegedFilesystemResult {
                action,
                path,
                destination: None,
                content_base64: None,
                bytes: bytes.len() as u32,
            })
        }
        PrivilegedFilesystemAction::CreateDirectory => {
            fs::create_dir_all(&path).map_err(|_| ())?;
            Ok(PrivilegedFilesystemResult {
                action,
                path,
                destination: None,
                content_base64: None,
                bytes: 0,
            })
        }
        PrivilegedFilesystemAction::Rename => {
            let destination = spec.destination.clone().ok_or(())?;
            fs::rename(&path, &destination).map_err(|_| ())?;
            Ok(PrivilegedFilesystemResult {
                action,
                path,
                destination: Some(destination),
                content_base64: None,
                bytes: 0,
            })
        }
        PrivilegedFilesystemAction::Delete => {
            let metadata = fs::symlink_metadata(&path).map_err(|_| ())?;
            if metadata.file_type().is_symlink() {
                if fs::metadata(&path).is_ok_and(|metadata| metadata.is_dir()) {
                    fs::remove_dir(&path).map_err(|_| ())?;
                } else {
                    fs::remove_file(&path).map_err(|_| ())?;
                }
            } else if metadata.is_dir() {
                if spec.recursive {
                    fs::remove_dir_all(&path).map_err(|_| ())?;
                } else {
                    fs::remove_dir(&path).map_err(|_| ())?;
                }
            } else {
                fs::remove_file(&path).map_err(|_| ())?;
            }
            Ok(PrivilegedFilesystemResult {
                action,
                path,
                destination: None,
                content_base64: None,
                bytes: 0,
            })
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "localbridge-privileged-filesystem-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn structured_privileged_filesystem_supports_binary_roundtrip_rename_and_delete() {
        let root = temp_root();
        let dir = root.join("admin-dir");
        let file = dir.join("payload.bin");
        let renamed = dir.join("renamed.bin");
        let payload = b"schema33\0binary";

        run_privileged_filesystem(PrivilegedFilesystemSpec {
            action: PrivilegedFilesystemAction::CreateDirectory,
            path: dir.to_string_lossy().into_owned(),
            destination: None,
            content_base64: None,
            recursive: false,
        })
        .unwrap();
        run_privileged_filesystem(PrivilegedFilesystemSpec {
            action: PrivilegedFilesystemAction::WriteFile,
            path: file.to_string_lossy().into_owned(),
            destination: None,
            content_base64: Some(STANDARD.encode(payload)),
            recursive: false,
        })
        .unwrap();
        let read = run_privileged_filesystem(PrivilegedFilesystemSpec {
            action: PrivilegedFilesystemAction::ReadFile,
            path: file.to_string_lossy().into_owned(),
            destination: None,
            content_base64: None,
            recursive: false,
        })
        .unwrap();
        assert_eq!(
            STANDARD.decode(read.content_base64.unwrap()).unwrap(),
            payload
        );
        run_privileged_filesystem(PrivilegedFilesystemSpec {
            action: PrivilegedFilesystemAction::Rename,
            path: file.to_string_lossy().into_owned(),
            destination: Some(renamed.to_string_lossy().into_owned()),
            content_base64: None,
            recursive: false,
        })
        .unwrap();
        assert!(renamed.is_file());
        run_privileged_filesystem(PrivilegedFilesystemSpec {
            action: PrivilegedFilesystemAction::Delete,
            path: dir.to_string_lossy().into_owned(),
            destination: None,
            content_base64: None,
            recursive: true,
        })
        .unwrap();
        assert!(!dir.exists());
        fs::remove_dir_all(root).unwrap();
    }
}
