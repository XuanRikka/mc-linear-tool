use std::num::ParseIntError;
use std::path::Path;

pub fn parse_region_coords(path: impl AsRef<Path>) -> Result<(i32, i32), String> {
    let path = path.as_ref();

    if path.is_dir() {
        return Err("path is a directory".to_string());
    }

    let filename = path.file_name()
        .ok_or("no filename")?
        .to_str()
        .ok_or("invalid filename")?;

    let parts: Vec<&str> = filename.split('.').collect();
    if parts.len() != 4 || parts[0] != "r" || parts[3] != "linear" {
        return Err(format!("invalid filename format: {}", filename));
    }

    let region_x: i32 = parts[1].parse().map_err(|e: ParseIntError| e.to_string())?;
    let region_z: i32 = parts[2].parse().map_err(|e: ParseIntError| e.to_string())?;

    Ok((region_x, region_z))
}