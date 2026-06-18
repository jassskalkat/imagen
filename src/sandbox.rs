use std::path::Path;

use crate::error::{ImagenError, Result};

/// Validate that a given output path resolves within the allowed output directory.
///
/// Since the target file may not yet exist, we canonicalize the parent directory
/// and verify that the resulting path is within the allowed base.
pub fn validate_output_path(path: &Path, output_dir: &str) -> Result<()> {
    let output_base = std::fs::canonicalize(output_dir).unwrap_or_else(|_| {
        // If the output_dir itself doesn't exist yet, use it as-is
        Path::new(output_dir).to_path_buf()
    });

    // For the target path, canonicalize as much as possible.
    // If the path exists, canonicalize it directly.
    // Otherwise, canonicalize its nearest existing ancestor.
    let resolved = if path.exists() {
        std::fs::canonicalize(path).map_err(|e| {
            ImagenError::InvalidInput(format!(
                "Cannot resolve path '{}': {e}",
                path.display()
            ))
        })?
    } else {
        // Walk up to find an existing ancestor
        let mut ancestor = path.to_path_buf();
        let mut components_to_append = Vec::new();

        loop {
            if let Some(parent) = ancestor.parent() {
                if parent.exists() {
                    let canonical_parent = std::fs::canonicalize(parent).map_err(|e| {
                        ImagenError::InvalidInput(format!(
                            "Cannot resolve parent '{}': {e}",
                            parent.display()
                        ))
                    })?;
                    // Rebuild with the remaining component
                    if let Some(file_name) = ancestor.file_name() {
                        components_to_append.push(file_name.to_os_string());
                    }
                    let mut result = canonical_parent;
                    for comp in components_to_append.into_iter().rev() {
                        result = result.join(comp);
                    }
                    break result;
                } else {
                    if let Some(file_name) = ancestor.file_name() {
                        components_to_append.push(file_name.to_os_string());
                    }
                    ancestor = parent.to_path_buf();
                }
            } else {
                // Reached filesystem root without finding existing ancestor
                break path.to_path_buf();
            }
        }
    };

    if !resolved.starts_with(&output_base) {
        return Err(ImagenError::InvalidInput(format!(
            "Path '{}' escapes the output directory '{}'",
            path.display(),
            output_dir
        )));
    }

    Ok(())
}

/// Validate that an input file path is safe to read.
///
/// Checks:
/// - No null bytes in the path string
/// - Path does not contain `..` traversal components
/// - Path exists and is a regular file
pub fn validate_input_path(path: &str) -> Result<()> {
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

    // Verify path exists and is a regular file
    let metadata = std::fs::metadata(p).map_err(|_| {
        ImagenError::InvalidInput(format!("Path does not exist: '{path}'"))
    })?;

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

    #[test]
    fn test_validate_input_path_valid_file() {
        let dir = std::env::temp_dir().join("sandbox-test-input");
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("valid.png");
        fs::write(&file_path, b"test").unwrap();

        let result = validate_input_path(file_path.to_str().unwrap());
        assert!(result.is_ok());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_validate_input_path_null_bytes() {
        let result = validate_input_path("/tmp/file\0.png");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("null bytes"), "Error was: {err}");
    }

    #[test]
    fn test_validate_input_path_traversal() {
        let result = validate_input_path("/tmp/images/../../../etc/passwd");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("traversal"), "Error was: {err}");
    }

    #[test]
    fn test_validate_input_path_nonexistent() {
        let result = validate_input_path("/nonexistent/path/file.png");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("does not exist"), "Error was: {err}");
    }

    #[test]
    fn test_validate_input_path_directory_rejected() {
        let dir = std::env::temp_dir().join("sandbox-test-dir");
        fs::create_dir_all(&dir).unwrap();

        let result = validate_input_path(dir.to_str().unwrap());
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
