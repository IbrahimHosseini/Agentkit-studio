use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use std::{fs, path::PathBuf};

pub fn data_root() -> Result<PathBuf> {
    let root = if let Some(root) = std::env::var_os("AGENTKIT_DATA_DIR") {
        PathBuf::from(root)
    } else {
        ProjectDirs::from("app", "agentkit", "Agentkit Studio")
            .context("Cannot determine the application data directory")?
            .data_local_dir()
            .to_path_buf()
    };
    fs::create_dir_all(&root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let meta = fs::symlink_metadata(&root)?;
        if meta.file_type().is_symlink() || meta.uid() != unsafe { libc::geteuid() } {
            bail!("Engine data directory must be owned by the current user and not a symlink");
        }
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
    }
    fs::canonicalize(root).context("Cannot resolve engine data directory")
}

#[cfg(unix)]
pub fn endpoint(root: &std::path::Path) -> Result<PathBuf> {
    let endpoint = root.join("engine.sock");
    if endpoint.as_os_str().len() > 100 {
        bail!(
            "Engine data directory is too long for a Unix socket; set a shorter AGENTKIT_DATA_DIR"
        );
    }
    Ok(endpoint)
}
#[cfg(windows)]
pub fn endpoint(root: &std::path::Path) -> Result<String> {
    use std::hash::{Hash, Hasher};
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    root.hash(&mut hash);
    Ok(format!(r"\\.\pipe\agentkit-studio-{:016x}", hash.finish()))
}

pub fn token(root: &std::path::Path) -> Result<String> {
    let path = root.join("engine.token");
    match fs::read_to_string(&path) {
        Ok(token) if token.len() >= 32 => Ok(token),
        Ok(_) => bail!("Invalid engine authentication token"),
        Err(err) => Err(err).context("Engine authentication token is unavailable"),
    }
}
