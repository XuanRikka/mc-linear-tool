use std::error::Error;
use std::io::{Read, Seek, SeekFrom};
use std::num::ParseIntError;
use std::path::{Path, PathBuf};

use binrw::{BinRead, Error::BadMagic};

/// 返回(x,z)
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
    if parts.len() < 3 || (parts[0] != "r" && parts[0] != "c") {
        return Err(format!("invalid filename format: {}", filename));
    }

    let region_x: i32 = parts[1].parse().map_err(|e: ParseIntError| e.to_string())?;
    let region_z: i32 = parts[2].parse().map_err(|e: ParseIntError| e.to_string())?;

    Ok((region_x, region_z))
}

/// 传入mca的完整path，自动解析并收集所在目录下的mcc文件
pub fn collect_mcc_files(path: impl AsRef<Path>) -> Result<Option<Vec<PathBuf>>, Box<dyn Error>> {
    let path = path.as_ref();
    let (region_x, region_z) = parse_region_coords(path)
        .map_err(|e| -> Box<dyn Error> { e.into() })?;

    let parent = path
        .parent()
        .ok_or_else(|| "no parent directory".to_string())?;

    let mut files = Vec::new();

    for i in 0..1024 {
        let chunk_x = region_x * 32 + (i % 32);
        let chunk_z = region_z * 32 + (i / 32);
        let filename = format!("c.{chunk_x}.{chunk_z}.mcc");
        let candidate = parent.join(filename);
        if candidate.is_file() {
            files.push(candidate);
        }
    }

    if files.is_empty() {
        Ok(None)
    } else {
        Ok(Some(files))
    }
}

#[derive(BinRead, Debug)]
#[brw(big)]
pub struct Linear
{
    #[brw(magic = b"\xC3\xFF\x13\x18\x3C\xCA\x9D\x9A")]
    pub version: u8,
}

#[derive(Clone, Debug)]
pub enum FileType
{
    Anvil,
    LinearV1,
    LinearV2
}

/// 简单判断文件类型，由于mca无文件头所以无法准确判断是否为mca文件，因此仅linear系列文件可信
pub fn get_file_type<R: Read + Seek>(file: &mut R) -> Result<FileType, Box<dyn Error + Sync + Send>>
{
    let pos = file.stream_position()?;
    let result = Linear::read(file);
    // 恢复游标（无论成功失败都恢复）
    file.seek(SeekFrom::Start(pos))?;

    match result {
        Err(BadMagic { .. }) => {
            Ok(FileType::Anvil)
        },

        Err(e) => Err(Box::new(e)),

        Ok(linear) => {
            match linear.version
            {
                1 | 2 => Ok(FileType::LinearV1),
                3 => Ok(FileType::LinearV2),
                _ => Err("未知的linear版本".into())
            }
        }
    }
}

