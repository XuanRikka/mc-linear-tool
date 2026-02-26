use std::collections::HashMap;
use std::error::Error;
use std::hash::Hasher;
use std::io::{copy, Cursor, Read, Seek, Write};

use binrw::{BinRead,BinWrite};
use xxhash_rust::xxh64::Xxh64;
use zstd::{decode_all};
use zstd::zstd_safe::CompressionLevel;
use zstd::stream::write::Encoder;
use crate::models::linear_v1;
use crate::models::linear_v1::ChunkHeaders;

#[derive(Debug)]
pub struct Region {
    pub chunks: Vec<Chunk>,
    pub region_x: i32,
    pub region_z: i32,
    pub nbt_features: HashMap<String, u32>
}

impl Region {
    pub fn new(chunks: Vec<Chunk>, region_x: i32, region_z: i32,
               nbt_features: HashMap<String, u32>) -> Region
    {
        Region {
            chunks,
            region_x,
            region_z,
            nbt_features
        }
    }


    pub fn from_linear_v1<F: Read + Seek>(mut file: F, region_x: i32, region_z: i32)
        -> Result<Region, Box<dyn Error>>
    {
        let superblock = linear_v1::SuperBlock::read(&mut file)?;

        let mut compress_buf = vec![0u8; superblock.compressed_data_length as usize];
        file.read_exact(&mut compress_buf)?;
        let compress_cursor = Cursor::new(compress_buf);

        let mut decompress_buf =  Cursor::new(decode_all(compress_cursor)?);
        let headers = ChunkHeaders::read(&mut decompress_buf)?;

        let mut chunks: Vec<Chunk> = Vec::new();
        for (index, chunk_header) in headers.chunk_headers.iter().enumerate()
        {
            let mut data = vec![0u8; chunk_header.size as usize];
            decompress_buf.read_exact(&mut data)?;
            chunks.push(
                Chunk::new(
                    data,
                    chunk_header.timestamp as u64,
                    index as u16
                )
            );
        };

        Ok(Region::new(
            chunks,
            region_x,
            region_z,
            HashMap::new()
        ))
    }

    pub fn to_linear_v1<F: Write + Seek>(&self, mut f: F, compression_level: CompressionLevel) -> Result<(), Box<dyn Error>>
    {
        let mut data: Cursor<Vec<u8>> = Cursor::new(Vec::new());


        // 写入头部数据
        for chunk in &self.chunks
        {
            let header = linear_v1::ChunkHeader {
                size: chunk.raw_chunk.len() as u32,
                timestamp: chunk.timestamps as u32
            };
            header.write(&mut data)?;
        };

        // 写入实际数据
        for chunk in &self.chunks
        {
            data.write_all(&chunk.raw_chunk)?;
        };

        // 这里不直接一开始就使用Encoder的原因是因为binrw要求实现了Seek的类型
        data.seek(std::io::SeekFrom::Start(0))?;
        let mut encoder = Encoder::new(Vec::new(), compression_level)?;
        encoder.include_checksum(true)?;
        encoder.write_all(&data.get_ref())?;
        let compress_data = encoder.finish()?;


        let superblock = linear_v1::SuperBlock {
            version: 1,
            newest_timestamp: self.get_newest_timestamp(),
            compression_level: compression_level as i8,
            chunk_count: self.chunk_count(),
            compressed_data_length: compress_data.len() as u32,
            reserved: 0,
        };

        superblock.write(&mut f)?;
        f.write_all(&compress_data)?;
        f.write_all(linear_v1::MAGIC)?;
        f.flush()?;
        Ok(())
    }


    pub fn chunk_count(&self) -> i16
    {
        self.chunks.iter().filter(|x| !x.is_empty()).count() as i16
    }

    pub fn hash(&self) -> u64
    {
        let mut xxhash = Xxh64::new(0);
        let mut data = [0u8; 8];
        data[0..4].copy_from_slice(&self.region_x.to_be_bytes());
        data[4..8].copy_from_slice(&self.region_z.to_be_bytes());
        xxhash.write(&data);

        for chunk in &self.chunks
        {
            if chunk.is_empty()
            {
                xxhash.write(b"\x00")
            }
            else
            {
                xxhash.write("\x01".as_bytes());
                let len = chunk.raw_chunk.len() as u32;
                xxhash.write(&len.to_be_bytes());
                xxhash.write(&chunk.raw_chunk);
            }
        }

        xxhash.digest()
    }

    pub fn get_newest_timestamp(&self) -> u64
    {
        self.chunks.iter().map(|chunk| chunk.timestamps).max().unwrap_or(0)
    }
}

#[derive(Debug)]
pub struct Chunk
{
    pub raw_chunk: Vec<u8>,
    pub timestamps: u64,
    pub x: i64,
    pub z: i64,
}

impl Chunk
{
    pub fn new(raw_chunk: Vec<u8>, timestamps: u64, index: u16) -> Chunk {
        let x = (index % 32) as i64;
        let z = (index / 32) as i64;

        Chunk {
            raw_chunk,
            timestamps,
            x,
            z,
        }
    }

    pub fn is_empty(&self) -> bool
    {
        self.raw_chunk.is_empty()
    }
}

