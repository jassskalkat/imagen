use std::path::Path;

use crate::error::{ImagenError, Result};

/// Validate that a given output path resolves within the allowed output directory.
///
/// Since neither the target file nor its parent directories may exist yet,
/// we walk both sides up to their nearest existing ancestor and canonicalize
/// from there so symlinks (e.g. `/var` → `/private/var` on macOS) are
/// resolved consistently on both sides before the prefix check.
pub fn validate_output_path(path: &Path, output_dir: &str) -> Result<()> {
    let output_base = resolve_existing_ancestor(Path::new(output_dir));
    let resolved = resolve_existing_ancestor(path);

    if !resolved.starts_with(&output_base) {
        return Err(ImagenError::InvalidInput(format!(
            "Path '{}' escapes the output directory '{}'",
            path.display(),
            output_dir
        )));
    }

    Ok(())
}

/// Walk a path up to its nearest existing ancestor, canonicalize that ancestor,
/// then re-append the non-existing tail components.
fn resolve_existing_ancestor(path: &Path) -> std::path::PathBuf {
    if path.exists() {
        return std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    }

    let mut ancestor = path.to_path_buf();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();

    loop {
        if let Some(parent) = ancestor.parent() {
            if parent == ancestor {
                // Hit the filesystem root; return as-is
                return path.to_path_buf();
            }
            if let Some(name) = ancestor.file_name() {
                tail.push(name.to_os_string());
            }
            ancestor = parent.to_path_buf();
            if ancestor.exists() {
                let mut result = std::fs::canonicalize(&ancestor).unwrap_or(ancestor);
                for component in tail.into_iter().rev() {
                    result = result.join(component);
                }
                return result;
            }
        } else {
            return path.to_path_buf();
        }
    }
}

/// Validate that an input file path is safe to read.
///
/// Checks:
/// - No null bytes in the path string
/// - Path does not contain `..` traversal components
/// - Path exists and is a regular file
pub async fn validate_input_path(path: &str) -> Result<()> {
    // Reject null bytes
    if path.contains('\0') {
        return Err(ImagenError::InvalidInput(
            "Path contains null bytes".to_string(),
        ));
    }

    // Reject path traversal via `..` components
    let p = Path::new(path);
    for component in p.components() {
        if let std::path::Component::ParentDir = component {
            return Err(ImagenError::InvalidInput(format!(
                "Path traversal detected in '{path}': contains '..' component"
            )));
        }
    }

    // Verify path exists and is a regular file (async to avoid blocking the runtime)
    let metadata = tokio::fs::metadata(p)
        .await
        .map_err(|_| ImagenError::InvalidInput(format!("Path does not exist: '{path}'")))?;

    if !metadata.is_file() {
        return Err(ImagenError::InvalidInput(format!(
            "Path is not a regular file: '{path}'"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[tokio::test]
    async fn test_validate_input_path_valid_file() {
        let dir = std::env::temp_dir().join("sandbox-test-input");
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("valid.png");
        fs::write(&file_path, b"test").unwrap();

        let result = validate_input_path(file_path.to_str().unwrap()).await;
        assert!(result.is_ok());

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_validate_input_path_null_bytes() {
        let result = validate_input_path("/tmp/file\0.png").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("null bytes"), "Error was: {err}");
    }

    #[tokio::test]
    async fn test_validate_input_path_traversal() {
        let result = validate_input_path("/tmp/images/../../../etc/passwd").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("traversal"), "Error was: {err}");
    }

    #[tokio::test]
    async fn test_validate_input_path_nonexistent() {
        let result = validate_input_path("/nonexistent/path/file.png").await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("does not exist"), "Error was: {err}");
    }

    #[tokio::test]
    async fn test_validate_input_path_directory_rejected() {
        let dir = std::env::temp_dir().join("sandbox-test-dir");
        fs::create_dir_all(&dir).unwrap();

        let result = validate_input_path(dir.to_str().unwrap()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not a regular file"), "Error was: {err}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_validate_output_path_within_dir() {
        let dir = std::env::temp_dir().join("sandbox-test-output");
        fs::create_dir_all(&dir).unwrap();

        let target = dir.join("job-123").join("image.png");
        let result = validate_output_path(&target, dir.to_str().unwrap());
        assert!(result.is_ok());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_validate_output_path_traversal_rejected() {
        let dir = std::env::temp_dir().join("sandbox-test-output-traversal");
        fs::create_dir_all(&dir).unwrap();

        let target = dir.join("..").join("..").join("etc").join("passwd");
        let result = validate_output_path(&target, dir.to_str().unwrap());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("escapes"), "Error was: {err}");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_validate_output_path_absolute_outside_rejected() {
        let dir = std::env::temp_dir().join("sandbox-test-output-abs");
        fs::create_dir_all(&dir).unwrap();

        let target = Path::new("/etc/passwd");
        let result = validate_output_path(target, dir.to_str().unwrap());
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&dir);
    }
}
