use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::fs;

use crate::error::{ImagenError, Result};
use crate::sandbox::validate_output_path;
use crate::types::OutputFormat;

/// Generate a deterministic file path for a job artifact.
///
/// The path is: `{output_dir}/{job_id}/{hash}.{extension}`
/// where hash is derived from job_id + index for predictability.
pub fn artifact_path(
    output_dir: &str,
    job_id: &str,
    index: u32,
    format: &OutputFormat,
) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(job_id.as_bytes());
    hasher.update(index.to_le_bytes());
    let hash = hex::encode(&hasher.finalize()[..8]);

    Path::new(output_dir)
        .join(job_id)
        .join(format!("{hash}.{}", format.extension()))
}

/// Save image bytes to the artifact path, creating directories as needed.
///
/// Validates that the path is within the specified output_dir before writing.
pub async fn save_artifact(path: &Path, data: &[u8], output_dir: &str) -> Result<u64> {
    // Validate that the artifact path is within the output directory
    validate_output_path(path, output_dir)?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await.map_err(|e| {
            ImagenError::FileError(format!(
                "Failed to create directory {}: {e}",
                parent.display()
            ))
        })?;
    }

    fs::write(path, data).await.map_err(|e| {
        ImagenError::FileError(format!("Failed to write artifact {}: {e}", path.display()))
    })?;

    Ok(data.len() as u64)
}

/// Read artifact bytes from a path.
pub async fn read_artifact(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).await.map_err(|e| {
        ImagenError::FileError(format!("Failed to read artifact {}: {e}", path.display()))
    })
}

/// Check if an artifact exists at the given path.
pub async fn artifact_exists(path: &Path) -> bool {
    fs::metadata(path).await.is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_artifact_path_deterministic() {
        let path1 = artifact_path("/tmp/out", "job-123", 0, &OutputFormat::Png);
        let path2 = artifact_path("/tmp/out", "job-123", 0, &OutputFormat::Png);
        assert_eq!(path1, path2);
    }

    #[test]
    fn test_artifact_path_different_index() {
        let path1 = artifact_path("/tmp/out", "job-123", 0, &OutputFormat::Png);
        let path2 = artifact_path("/tmp/out", "job-123", 1, &OutputFormat::Png);
        assert_ne!(path1, path2);
    }

    #[test]
    fn test_artifact_path_format() {
        let path = artifact_path("/output", "abc", 0, &OutputFormat::Webp);
        assert!(path.to_string_lossy().ends_with(".webp"));
        assert!(path.to_string_lossy().starts_with("/output/abc/"));
    }

    #[test]
    fn test_artifact_path_jpeg_extension() {
        let path = artifact_path("/output", "job-1", 0, &OutputFormat::Jpeg);
        assert!(path.to_string_lossy().ends_with(".jpeg"));
    }

    #[test]
    fn test_artifact_path_png_extension() {
        let path = artifact_path("/out", "job-2", 2, &OutputFormat::Png);
        assert!(path.to_string_lossy().ends_with(".png"));
        assert!(path.to_string_lossy().contains("/job-2/"));
    }

    #[test]
    fn test_artifact_path_different_jobs() {
        let path1 = artifact_path("/out", "job-a", 0, &OutputFormat::Png);
        let path2 = artifact_path("/out", "job-b", 0, &OutputFormat::Png);
        assert_ne!(path1, path2);
    }

    #[tokio::test]
    async fn test_save_and_read_artifact() {
        let dir = std::env::temp_dir().join("imagen-test-artifacts");
        let path = dir.join("test-job").join("test.png");

        let data = b"fake image data";
        let size = save_artifact(&path, data, dir.to_str().unwrap()).await.unwrap();
        assert_eq!(size, data.len() as u64);

        let read_back = read_artifact(&path).await.unwrap();
        assert_eq!(read_back, data);

        // Cleanup
        let _ = fs::remove_dir_all(&dir).await;
    }
}
