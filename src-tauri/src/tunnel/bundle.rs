use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::fault::TunnelError;

pub(crate) const TUNNEL_CLIENT_SHA256: &str =
    "7d3c7d492ce84b52835e11865a835a8a5bcd4a669dee84e169aa11b314dc952a";
const LICENSE_SHA256: &str = "f4c1d7ba32ef5bcf5cf03e2eefec5825ebafedf50fa330a36700a49c605c1ef4";

#[derive(Debug, Clone)]
pub(crate) struct VerifiedTunnelBundle {
    pub(crate) executable: PathBuf,
}

pub(crate) fn verify_bundle(install_root: &Path) -> Result<VerifiedTunnelBundle, TunnelError> {
    let root = install_root.join("runtime").join("tunnel-client");
    let executable = root.join("tunnel-client.exe");
    for (path, expected) in [
        (&executable, TUNNEL_CLIENT_SHA256),
        (&root.join("LICENSE"), LICENSE_SHA256),
    ] {
        verify_file(path, expected)?;
    }
    Ok(VerifiedTunnelBundle { executable })
}

fn verify_file(path: &Path, expected: &str) -> Result<(), TunnelError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(TunnelError::RuntimeMissing);
        }
        Err(_) => return Err(TunnelError::RuntimeChecksumMismatch),
    };
    let actual = format!("{:x}", Sha256::digest(&bytes));
    if actual == expected {
        Ok(())
    } else {
        Err(TunnelError::RuntimeChecksumMismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("src-tauri has repository parent")
            .to_path_buf()
    }

    fn temp_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "localbridge-lb008-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn pinned_standalone_tunnel_bundle_hashes_are_valid() {
        let verified = verify_bundle(&repo_root()).expect("vendored LB-008 bundle must verify");
        assert!(verified.executable.ends_with("tunnel-client.exe"));
        assert!(!repo_root()
            .join("runtime/tunnel-client/cloudflared.exe")
            .exists());
        assert!(!repo_root()
            .join("runtime/tunnel-client/cloudflared-manifest.json")
            .exists());
    }

    #[test]
    fn missing_and_corrupt_bundle_fail_closed() {
        let missing = temp_root("missing");
        fs::create_dir_all(&missing).unwrap();
        assert!(matches!(
            verify_bundle(&missing),
            Err(TunnelError::RuntimeMissing)
        ));

        let corrupt = temp_root("corrupt");
        let runtime = corrupt.join("runtime").join("tunnel-client");
        fs::create_dir_all(&runtime).unwrap();
        fs::write(runtime.join("tunnel-client.exe"), b"corrupt").unwrap();
        assert!(matches!(
            verify_bundle(&corrupt),
            Err(TunnelError::RuntimeChecksumMismatch)
        ));

        fs::remove_dir_all(missing).unwrap();
        fs::remove_dir_all(corrupt).unwrap();
    }
}
