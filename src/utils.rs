use std::error::Error;
use std::path::{Path, PathBuf};
use std::thread::available_parallelism;
use walkdir::WalkDir;


pub fn get_dir_file<P: AsRef<Path>>(path: P, suffix: &str, max_depth: usize) -> Result<Vec<PathBuf>, Box<dyn Error + Send + Sync>> {
    Ok(WalkDir::new(path)
        .max_depth(max_depth)
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

pub fn get_cpu_num() -> usize
{
    available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

pub fn split_into_chunks<T>(mut vec: Vec<T>, n: usize) -> Vec<Vec<T>> {
    if n == 0 || vec.is_empty() {
        return vec![];
    }

    let mut chunks = Vec::with_capacity(n.min(vec.len()));

    for i in 0..n {
        if vec.is_empty() {
            break;
        }

        let remaining_chunks = n - i;
        let current_chunk_size = (vec.len() + remaining_chunks - 1) / remaining_chunks;

        if i == n - 1 {
            chunks.push(vec);
            break;
        }

        let chunk = vec.split_off(current_chunk_size);
        chunks.push(chunk);
    }

    chunks
}

