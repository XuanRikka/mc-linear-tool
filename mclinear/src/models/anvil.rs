use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom, Write};
use std::iter::zip;
use std::path::Path;

use binrw::{BinRead, BinWrite};
use flate2::read::{GzDecoder, ZlibDecoder};
use flate2::write::{GzEncoder, ZlibEncoder};
use flate2::Compression;
use lz4_flex::frame::{FrameDecoder, FrameEncoder};
use crate::region::Chunk;
use crate::utils::parse_region_coords;

#[derive(BinRead, BinWrite)]
#[brw(big)]
pub struct LocationTable
{
    #[br(count = 1024)]
    pub locations: Vec<LocationData>,
}

#[derive(BinRead, BinWrite)]
#[brw(big)]
pub struct LocationData
{
    #[br(map(|x| u24_be_to_u32(&x)))]
    #[bw(map(|x| u32_to_u24_be(x)))]
    pub sector_offset: u32,
    pub sector_count: u8,
}

#[derive(BinRead, BinWrite)]
#[brw(big)]
pub struct TimestampTable
{
    #[br(count = 1024)]
    pub timestamps_data: Vec<TimestampData>,
}

#[derive(BinRead, BinWrite)]
#[brw(big)]
pub struct TimestampData
{
    pub timestamp: u32
}

#[derive(BinRead, BinWrite)]
#[brw(big)]
pub struct DataHeader
{
    pub chunk_length: u32,
    pub compression_type: u8,
}

/// anvil没有超级块的概念，这是为了对齐逻辑我把头部数据揉在一起做出来的
pub struct SuperBlock
{
    pub chunks_info: Vec<ChunkDataInfo>,
}


pub struct ChunkDataInfo
{
    pub sector_offset: u32,
    pub sector_count: u8,
    pub timestamp: u32,
}

pub fn serialize_chunk_data<F: Write + Seek, P: AsRef<Path>>(f: &mut F, file_path: P,
                                                             chunks: Vec<Chunk>, compression_level: u8, compression_type: u8)
                                                             -> Result<Vec<ChunkDataInfo>, Box<dyn Error>>
{
    let mut chunks_info: Vec<ChunkDataInfo> = Vec::with_capacity(1024);
    let mut sector_offset: u32 = 2;
    for chunk in chunks
    {
        if chunk.is_empty()
        {
            chunks_info.push(ChunkDataInfo {
                sector_offset: 0,
                sector_count: 0,
                timestamp: chunk.timestamps as u32
            });
            continue;
        }
        let t = chunk_compression(chunk.raw_chunk, compression_type, compression_level)?;
        let mut sector_count = (4 + 1 + t.len()).div_ceil(4096);

        if sector_count <= 255
        {
            let header = DataHeader {
                chunk_length: (t.len() as u32) + 1,
                compression_type,
            };
            header.write(f)?;
            f.write_all(&t)?;
            // 填0以对齐4kib
            let pad = (4096 - ((5 + t.len()) % 4096)) % 4096;
            f.write_all(&vec![0u8; pad])?;
        }
        else
        {
            sector_count = 1;
            let header = DataHeader {
                chunk_length: 1,
                compression_type: compression_type+128,
            };
            header.write(f)?;

            let dir = file_path.as_ref().parent().expect("解析路径失败");
            let mcc = dir.join(format!("c.{}.{}.mcc", chunk.x, chunk.z));

            let mut mcc_file = File::create(&mcc)?;

            mcc_file.write_all(&t)?;

            f.write_all(&vec![0u8; 4091])?;
        }

        chunks_info.push(ChunkDataInfo {
            sector_offset: sector_offset,
            sector_count: sector_count as u8,
            timestamp: chunk.timestamps as u32
        });
        sector_offset += sector_count as u32;
    };
    Ok(chunks_info)
}

pub fn deserialize_chunk_data<F: Read + Seek, P: AsRef<Path>>(f: &mut F, mcc: Option<Vec<P>>,
                                                              superblock: &SuperBlock)
    -> Result<Vec<Vec<u8>>, Box<dyn Error>>
{
    let mut index_mcc: HashMap<u32, P>;
    if !mcc.is_none()
    {
        let mcc = mcc.unwrap();
        index_mcc = HashMap::with_capacity(mcc.len());
        for path in mcc
        {
            index_mcc.insert(
                mcc_path_to_index(&path)?,
                path,
            );
        };
    }
    else
    {
        index_mcc = HashMap::new();
    }

    let mut result: Vec<Vec<u8>> = Vec::with_capacity(1024);
    for (index, info) in superblock.chunks_info.iter().enumerate()
    {
        let data: Vec<u8>;
        if info.sector_offset == 0 || info.sector_count == 0
        {
            result.push(Vec::new());
            continue
        }

        f.seek(SeekFrom::Start((info.sector_offset * 4096) as u64))?;
        let header = DataHeader::read(f)?;

        // 压缩类型只到1-4，如果超过3了那就是有外部mcc，需要-128得到实际类型
        if header.compression_type >= 128
        {
            let mut mcc_file = File::open(index_mcc.get(&(index as u32))
                .expect("读取mcc失败"))?;
            let mut raw_data = Vec::new();
            mcc_file.read_to_end(&mut raw_data)?;
            data = chunk_decompress(raw_data, header.compression_type-128)?;
        }
        else
        {
            let max_len = info.sector_count as u32 * 4096 - 4;
            if header.chunk_length == 0 || header.chunk_length > max_len {
                return Err("文件数据异常，可能损坏".into());
            }


            let mut t = vec![0u8; (header.chunk_length-1) as usize];
            f.read_exact(&mut t)?;
            data = chunk_decompress(t, header.compression_type)?
        }
        result.push(data);
    };

    Ok(result)
}

pub fn chunk_decompress(data: Vec<u8>, compression_type: u8) -> Result<Vec<u8>, Box<dyn Error>>
{
    match compression_type
    {
        1 => {
            let mut decoder = GzDecoder::new(Cursor::new(data));
            let mut result = Vec::new();
            decoder.read_to_end(&mut result)?;
            Ok(result)
        }
        2 => {
            let mut decoder = ZlibDecoder::new(Cursor::new(data));
            let mut result = Vec::new();
            decoder.read_to_end(&mut result)?;
            Ok(result)
        },
        3 => {
            Ok(data)
        },
        4 => {
            let mut decoder = FrameDecoder::new(Cursor::new(data));
            let mut result = Vec::new();
            decoder.read_to_end(&mut result)?;
            Ok(result)
        }
        _ => {
            Err("unknown compression type".into())
        }
    }
}

pub fn chunk_compression(data: Vec<u8>, compression_type: u8, compression_level: u8) -> Result<Vec<u8>, Box<dyn Error>>
{
    match compression_type
    {
        1 => {
            let result = Vec::new();
            let mut encoder = GzEncoder::new(result, Compression::new(compression_level as u32));
            encoder.write_all(&data)?;
            Ok(encoder.finish()?)
        }
        2 => {
            let result = Vec::new();
            let mut encoder = ZlibEncoder::new(result, Compression::new(compression_level as u32));
            encoder.write_all(&data)?;
            Ok(encoder.finish()?)
        },
        3 => {
            Ok(data)
        },
        4 => {
            let result = Vec::new();
            let mut encoder = FrameEncoder::new(result);
            encoder.write_all(&data)?;
            Ok(encoder.finish()?)
        }
        _ => {
            Err("unknown compression type".into())
        }
    }
}

pub fn mcc_path_to_index<P: AsRef<Path>>(mcc: P) -> Result<u32, Box<dyn Error>>
{
    let path = mcc.as_ref();
    let (x,z) = parse_region_coords(path)?;
    let local_x = ((x % 32) + 32) % 32;
    let local_z = ((z % 32) + 32) % 32;
    let index = local_z * 32 + local_x;
    Ok(index as u32)
}

pub fn deserialize_superblock<F: Read + Seek>(f: &mut F) -> Result<SuperBlock, Box<dyn Error>>
{
    let location_table = LocationTable::read(f)?;
    let timestamp_table = TimestampTable::read(f)?;

    let mut chunks_info: Vec<ChunkDataInfo> = Vec::with_capacity(1024);

    for (location, timestamp) in
        zip(location_table.locations, timestamp_table.timestamps_data)
    {
        chunks_info.push(
            ChunkDataInfo {
                sector_offset: location.sector_offset,
                sector_count: location.sector_count,
                timestamp: timestamp.timestamp,
            }
        )
    };

    Ok(SuperBlock { chunks_info })
}

pub fn serialize_superblock<F: Write + Seek>(
    f: &mut F,
    chunks_info: &[ChunkDataInfo],
) -> Result<(), Box<dyn Error>> {
    if chunks_info.len() != 1024 {
        return Err("chunks_info must contain 1024 entries".into());
    }

    let locations: Vec<LocationData> = chunks_info
        .iter()
        .map(|c| LocationData {
            sector_offset: c.sector_offset,
            sector_count: c.sector_count,
        })
        .collect();
    let timestamps_data: Vec<TimestampData> = chunks_info
        .iter()
        .map(|c| TimestampData { timestamp: c.timestamp })
        .collect();

    let location_table = LocationTable { locations };
    let timestamp_table = TimestampTable { timestamps_data };

    f.seek(SeekFrom::Start(0))?;
    location_table.write(f)?;
    timestamp_table.write(f)?;
    Ok(())
}


fn u24_be_to_u32(bytes: &[u8; 3]) -> u32 {
    ((bytes[0] as u32) << 16) | ((bytes[1] as u32) << 8) | (bytes[2] as u32)
}

fn u32_to_u24_be(value: &u32) -> [u8; 3] {
    [
        ((value >> 16) & 0xFF) as u8,
        ((value >> 8) & 0xFF) as u8,
        (value & 0xFF) as u8,
    ]
}
