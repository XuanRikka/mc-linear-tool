use std::error::Error;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;


pub fn get_dir_file<P: AsRef<Path>>(path: P, suffix: &str, max_depth: usize) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    Ok(WalkDir::new(path)
        .max_depth(1)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .map(|ext| ext.to_string_lossy() == suffix)
                .unwrap_or(false)
        })
        .map(|e| e.into_path())
        .collect())
}


