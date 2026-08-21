use std::fs::{self, File};
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::runtime::{CodingToolsRuntimeError, RuntimeIntegrityComponent};

pub(crate) const PYTHON_VERSION: &str = "3.12.10";
pub(crate) const PYTHON_EXE_SHA256: &str =
    "4d6f5f81a4bca11191c4c7c6b43632694d0a4ce74e068619d8fdc161d469859a";
const PYTHON_DLL_SHA256: &str = "9a0e3435aaa680d868150f87ab3e388ad2eebc22f87e036155c7b4eda8cd2120";
const PYTHON_STDLIB_SHA256: &str =
    "fb131c0ef7e35cc5250a74c8cd18744bf4115fb8163710711f3758d7df3d1f88";
const PYTHON_PTH_SHA256: &str = "3840e706682aa41ec7e599a50763bec6c6ddd6bde66e81c64afe2394539ea4fa";
const PYTHON_TREE_SHA256: &str = "48546587a8bb59d03016ea4edf82c292a477dec6acec530745b78c8935558682";

pub(crate) const CODING_TOOLS_VERSION: &str = "0.2.2";
const CODING_TOOLS_COMMIT: &str = "311c1f2529d0f047ad2a8b68db6bf92dbb93d6bc";
const CODING_TOOLS_TREE: &str = "5ef5e638a12aa74dc8836ca02a88490c2fe019a6";
const CODING_TOOLS_FULL_ARCHIVE_SHA256: &str =
    "227b94128eaacda8d63d391911db1918e0bd813718d9ea5d276b5aab7eac73fd";
const CODING_TOOLS_SUBSET_SHA256: &str =
    "cc2171854ce0035942b752ce88bb3aec2e286cdf9603dd51ad41734ea70dcda6";
const CODING_TOOLS_TREE_SHA256: &str =
    "ce044634ac614a23cf5a4d6b6b27c7563460eb9df9cb1a2e09ab939a9a301f71";
const PYJWT_VERSION: &str = "2.10.1";
const PYJWT_WHEEL_SHA256: &str = "dcdd193e30abefd5debf142f9adfcdd2b58004e644f25406ffaebd50bd98dacb";

#[derive(Debug, Deserialize)]
struct PythonMetadata {
    schema_version: u32,
    runtime: String,
    version: String,
    source_archive_sha256: String,
    executable: String,
    executable_sha256: String,
    python312_dll_sha256: String,
    stdlib_zip_sha256: String,
    pth_sha256: String,
    isolated: bool,
    user_site_enabled: bool,
    runtime_pip_present: bool,
    external_python_fallback: bool,
    payload_tree_sha256: String,
}

#[derive(Debug, Deserialize)]
struct CodingMetadata {
    schema_version: u32,
    runtime: String,
    version: String,
    git_commit: String,
    git_tree: String,
    full_git_archive_sha256: String,
    runtime_subset_archive_sha256: String,
    entry_module: String,
    dependency_pyjwt_version: String,
    dependency_pyjwt_wheel_sha256: String,
    runtime_pip_present: bool,
    payload_tree_sha256: String,
}

pub(crate) struct VerifiedBundle {
    pub(crate) python_executable: PathBuf,
}

pub(crate) fn verify_bundle(
    install_root: &Path,
) -> Result<VerifiedBundle, CodingToolsRuntimeError> {
    let python_root = install_root.join("runtime").join("python");
    let coding_root = install_root.join("runtime").join("coding-tools-mcp");
    let python_executable = python_root.join("python.exe");
    let coding_package = coding_root.join("coding_tools_mcp").join("__init__.py");

    require_file(&python_executable, RuntimeIntegrityComponent::Python)?;
    require_file(&coding_package, RuntimeIntegrityComponent::CodingTools)?;

    verify_hash(
        &python_executable,
        PYTHON_EXE_SHA256,
        RuntimeIntegrityComponent::Python,
    )?;
    verify_hash(
        &python_root.join("python312.dll"),
        PYTHON_DLL_SHA256,
        RuntimeIntegrityComponent::Python,
    )?;
    verify_hash(
        &python_root.join("python312.zip"),
        PYTHON_STDLIB_SHA256,
        RuntimeIntegrityComponent::Python,
    )?;
    verify_hash(
        &python_root.join("python312._pth"),
        PYTHON_PTH_SHA256,
        RuntimeIntegrityComponent::Python,
    )?;

    let python_meta: PythonMetadata = read_metadata(
        &python_root.join("runtime-metadata.json"),
        RuntimeIntegrityComponent::Python,
    )?;
    if python_meta.schema_version != 1
        || python_meta.runtime != "python-embedded"
        || python_meta.version != PYTHON_VERSION
        || python_meta.source_archive_sha256
            != "4acbed6dd1c744b0376e3b1cf57ce906f9dc9e95e68824584c8099a63025a3c3"
        || python_meta.executable != "runtime/python/python.exe"
        || python_meta.executable_sha256 != PYTHON_EXE_SHA256
        || python_meta.python312_dll_sha256 != PYTHON_DLL_SHA256
        || python_meta.stdlib_zip_sha256 != PYTHON_STDLIB_SHA256
        || python_meta.pth_sha256 != PYTHON_PTH_SHA256
        || !python_meta.isolated
        || python_meta.user_site_enabled
        || python_meta.runtime_pip_present
        || python_meta.external_python_fallback
        || python_meta.payload_tree_sha256 != PYTHON_TREE_SHA256
    {
        return Err(CodingToolsRuntimeError::RuntimeChecksumMismatch(
            RuntimeIntegrityComponent::Python,
        ));
    }

    let coding_meta: CodingMetadata = read_metadata(
        &coding_root.join("runtime-metadata.json"),
        RuntimeIntegrityComponent::CodingTools,
    )?;
    if coding_meta.schema_version != 1
        || coding_meta.runtime != "coding-tools-mcp"
        || coding_meta.version != CODING_TOOLS_VERSION
        || coding_meta.git_commit != CODING_TOOLS_COMMIT
        || coding_meta.git_tree != CODING_TOOLS_TREE
        || coding_meta.full_git_archive_sha256 != CODING_TOOLS_FULL_ARCHIVE_SHA256
        || coding_meta.runtime_subset_archive_sha256 != CODING_TOOLS_SUBSET_SHA256
        || coding_meta.entry_module != "coding_tools_mcp"
        || coding_meta.dependency_pyjwt_version != PYJWT_VERSION
        || coding_meta.dependency_pyjwt_wheel_sha256 != PYJWT_WHEEL_SHA256
        || coding_meta.runtime_pip_present
        || coding_meta.payload_tree_sha256 != CODING_TOOLS_TREE_SHA256
    {
        return Err(CodingToolsRuntimeError::RuntimeChecksumMismatch(
            RuntimeIntegrityComponent::CodingTools,
        ));
    }

    if tree_sha256(&python_root, "runtime-metadata.json")? != PYTHON_TREE_SHA256 {
        return Err(CodingToolsRuntimeError::RuntimeChecksumMismatch(
            RuntimeIntegrityComponent::Python,
        ));
    }
    if tree_sha256(&coding_root, "runtime-metadata.json")? != CODING_TOOLS_TREE_SHA256 {
        return Err(CodingToolsRuntimeError::RuntimeChecksumMismatch(
            RuntimeIntegrityComponent::CodingTools,
        ));
    }

    Ok(VerifiedBundle { python_executable })
}

fn require_file(
    path: &Path,
    component: RuntimeIntegrityComponent,
) -> Result<(), CodingToolsRuntimeError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(()),
        Ok(_) => Err(CodingToolsRuntimeError::RuntimeMissing(component)),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            Err(CodingToolsRuntimeError::RuntimeMissing(component))
        }
        Err(_) => Err(CodingToolsRuntimeError::RuntimeChecksumMismatch(component)),
    }
}

fn read_metadata<T: for<'de> Deserialize<'de>>(
    path: &Path,
    component: RuntimeIntegrityComponent,
) -> Result<T, CodingToolsRuntimeError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Err(CodingToolsRuntimeError::RuntimeMissing(component));
        }
        Err(_) => return Err(CodingToolsRuntimeError::RuntimeChecksumMismatch(component)),
    };
    serde_json::from_slice(&bytes)
        .map_err(|_| CodingToolsRuntimeError::RuntimeChecksumMismatch(component))
}

fn verify_hash(
    path: &Path,
    expected: &str,
    component: RuntimeIntegrityComponent,
) -> Result<(), CodingToolsRuntimeError> {
    require_file(path, component)?;
    let actual = file_sha256(path)
        .map_err(|_| CodingToolsRuntimeError::RuntimeChecksumMismatch(component))?;
    if actual == expected {
        Ok(())
    } else {
        Err(CodingToolsRuntimeError::RuntimeChecksumMismatch(component))
    }
}

fn file_sha256(path: &Path) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn tree_sha256(root: &Path, excluded_name: &str) -> Result<String, CodingToolsRuntimeError> {
    let component = if root.ends_with("python") {
        RuntimeIntegrityComponent::Python
    } else {
        RuntimeIntegrityComponent::CodingTools
    };
    let mut files = Vec::new();
    collect_files(root, root, excluded_name, &mut files)
        .map_err(|_| CodingToolsRuntimeError::RuntimeChecksumMismatch(component))?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let canonical = files
        .into_iter()
        .map(|(relative, hash)| format!("{relative}\0{hash}"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut digest = Sha256::new();
    digest.update(canonical.as_bytes());
    Ok(format!("{:x}", digest.finalize()))
}

fn collect_files(
    root: &Path,
    directory: &Path,
    excluded_name: &str,
    out: &mut Vec<(String, String)>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_files(root, &path, excluded_name, out)?;
        } else if file_type.is_file()
            && path.file_name().and_then(|name| name.to_str()) != Some(excluded_name)
        {
            let relative = path
                .strip_prefix(root)
                .expect("collected file must remain under runtime root")
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, file_sha256(&path)?));
        }
    }
    Ok(())
}
