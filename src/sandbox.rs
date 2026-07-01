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

/// Maximum size (in bytes) accepted for an input image or mask file.
/// Providers read the entire file into memory (and clone it into a multipart
/// body on each retry attempt), so this bounds worst-case memory use per
/// concurrent job. 25 MiB is generous for any image format while blocking
/// pathological or accidental huge-file inputs.
pub const MAX_INPUT_FILE_SIZE: u64 = 25 * 1024 * 1024;

/// Validate that an input file path is safe to read.
///
/// Checks:
/// - No null bytes in the path string
/// - Path does not contain `..` traversal components
/// - Path exists and is a regular file
/// - File size does not exceed [`MAX_INPUT_FILE_SIZE`]
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

    if metadata.len() > MAX_INPUT_FILE_SIZE {
        return Err(ImagenError::InvalidInput(format!(
            "File '{path}' is {} bytes, which exceeds the maximum allowed size of {} bytes ({} MiB)",
            metadata.len(),
            MAX_INPUT_FILE_SIZE,
            MAX_INPUT_FILE_SIZE / (1024 * 1024)
        )));
    }

    Ok(())
}

/// Maximum size (in bytes) accepted for a mask file. The OpenAI/Azure image
/// edit APIs require masks to be a valid PNG under 4 MiB, stricter than the
/// general input image limit.
pub const MAX_MASK_FILE_SIZE: u64 = 4 * 1024 * 1024;

/// Validate a mask file the same way as [`validate_input_path`], but against
/// the stricter [`MAX_MASK_FILE_SIZE`] limit documented for mask uploads.
pub async fn validate_mask_path(path: &str) -> Result<()> {
    validate_input_path(path).await?;

    // validate_input_path already confirmed the file exists and is a regular
    // file, so this metadata call cannot fail for reasons other than a race
    // with something deleting the file between the two calls.
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|_| ImagenError::InvalidInput(format!("Path does not exist: '{path}'")))?;

    if metadata.len() > MAX_MASK_FILE_SIZE {
        return Err(ImagenError::InvalidInput(format!(
            "Mask file '{path}' is {} bytes, which exceeds the maximum allowed mask size of {} bytes ({} MiB)",
            metadata.len(),
            MAX_MASK_FILE_SIZE,
            MAX_MASK_FILE_SIZE / (1024 * 1024)
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

    #[tokio::test]
    async fn test_validate_input_path_oversized_file_rejected() {
        let dir = std::env::temp_dir().join("sandbox-test-oversized");
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("huge.png");

        // Write a sparse file just over the limit without actually allocating
        // MAX_INPUT_FILE_SIZE bytes of disk/memory for the test.
        let file = fs::File::create(&file_path).unwrap();
        file.set_len(MAX_INPUT_FILE_SIZE + 1).unwrap();

        let result = validate_input_path(file_path.to_str().unwrap()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("exceeds the maximum allowed size"),
            "Error was: {err}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_validate_input_path_at_size_limit_accepted() {
        let dir = std::env::temp_dir().join("sandbox-test-at-limit");
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("at_limit.png");

        let file = fs::File::create(&file_path).unwrap();
        file.set_len(MAX_INPUT_FILE_SIZE).unwrap();

        let result = validate_input_path(file_path.to_str().unwrap()).await;
        assert!(
            result.is_ok(),
            "File exactly at the limit should be accepted"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_validate_mask_path_oversized_rejected() {
        let dir = std::env::temp_dir().join("sandbox-test-mask-oversized");
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("mask.png");

        // Just over the mask-specific 4 MiB limit, but well under the general
        // 25 MiB input file limit, to prove the mask check is actually stricter.
        let file = fs::File::create(&file_path).unwrap();
        file.set_len(MAX_MASK_FILE_SIZE + 1).unwrap();

        let result = validate_mask_path(file_path.to_str().unwrap()).await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("exceeds the maximum allowed mask size"),
            "Error was: {err}"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_validate_mask_path_at_limit_accepted() {
        let dir = std::env::temp_dir().join("sandbox-test-mask-at-limit");
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("mask.png");

        let file = fs::File::create(&file_path).unwrap();
        file.set_len(MAX_MASK_FILE_SIZE).unwrap();

        let result = validate_mask_path(file_path.to_str().unwrap()).await;
        assert!(
            result.is_ok(),
            "Mask exactly at the mask-specific limit should be accepted"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn test_validate_mask_path_under_general_limit_but_over_mask_limit() {
        // Prove validate_mask_path is stricter than validate_input_path: a file
        // between 4 MiB and 25 MiB passes the general check but must fail the
        // mask-specific one.
        let dir = std::env::temp_dir().join("sandbox-test-mask-vs-general");
        fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("mask.png");

        let file = fs::File::create(&file_path).unwrap();
        file.set_len(10 * 1024 * 1024).unwrap(); // 10 MiB

        assert!(validate_input_path(file_path.to_str().unwrap())
            .await
            .is_ok());
        assert!(validate_mask_path(file_path.to_str().unwrap())
            .await
            .is_err());

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
