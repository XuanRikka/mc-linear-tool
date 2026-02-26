use std::collections::HashMap;
use std::error::Error;
use std::hash::Hasher;
use std::io::{Cursor, Read, Seek, Write};

use binrw::{BinRead,BinWrite};
use xxhash_rust::xxh64::Xxh64;
use zstd::{decode_all};
use zstd::zstd_safe::CompressionLevel;
use zstd::stream::write::Encoder;

use crate::models::{linear_v1, linear_v2};

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
        let headers = linear_v1::ChunkHeaders::read(&mut decompress_buf)?;

        let mut chunks: Vec<Chunk> = Vec::with_capacity(1024);
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

    pub fn to_linear_v1<F: Write + Seek>(self, mut f: F, compression_level: CompressionLevel)
        -> Result<(), Box<dyn Error>>
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

    pub fn from_linear_v2<F: Read + Seek>(mut f: F)
        -> Result<Region, Box<dyn Error>>
    {
        let superblock = linear_v2::SuperBlock::read(&mut f)?;
        let grid_size = superblock.grid_size as usize;

        if ![1, 2, 4, 8, 16, 32].contains(&grid_size) {
            return Err(format!("Incorrect grid_size: {}", grid_size).into());
        }

        let chunk_bitmap = linear_v2::ChunkBitMap::read(&mut f)?;

        let nbt_features = linear_v2::deserialize_hashmap(&mut f)?;

        let bucket_datas = linear_v2::deserialize_bucket(&mut f, superblock.grid_size)?;

        let chunks_per_bucket = 32 / grid_size;
        let mut chunks: Vec<Chunk> = Vec::with_capacity(1024);

        let parsed_buckets: Vec<Vec<linear_v2::BucketChunk>> = bucket_datas
            .iter()
            .map(|b| linear_v2_parse_bucket_chunks(b, chunks_per_bucket))
            .collect();

        for chunk_index in 0..1024 {
            let x_in_region = chunk_index % 32;
            let z_in_region = chunk_index / 32;

            let bucket_x = x_in_region / chunks_per_bucket;
            let bucket_z = z_in_region / chunks_per_bucket;
            let ix = x_in_region % chunks_per_bucket;
            let iz = z_in_region % chunks_per_bucket;
            let bucket_index = bucket_x * grid_size + bucket_z;

            let (raw, ts) = if chunk_bitmap.bit_map[chunk_index] {
                if let Some(bucket) = parsed_buckets.get(bucket_index) {
                    let local_index = ix * chunks_per_bucket + iz;
                    if let Some(bc) = bucket.get(local_index) {
                        (bc.chunk_data.clone(), bc.timestamp)
                    } else {
                        (Vec::new(), 0)
                    }
                } else {
                    (Vec::new(), 0)
                }
            } else {
                (Vec::new(), 0)
            };

            chunks.push(Chunk {
                raw_chunk: raw,
                timestamps: ts,
                x: superblock.region_x as i64 * 32 + x_in_region as i64,
                z: superblock.region_z as i64 * 32 + z_in_region as i64,
            });
        }

        Ok(Region::new(
            chunks,
            superblock.region_x,
            superblock.region_z,
            nbt_features,
        ))
    }

    pub fn to_linear_v2<W: Write + Seek>(mut self, f: &mut W, compression_level: CompressionLevel,
                                  grid_size: i8) -> Result<(), Box<dyn Error>>
    {
        if ![1, 2, 4, 8, 16, 32].contains(&grid_size) {
            return Err(format!("Incorrect grid_size: {}", grid_size).into());
        }

        let superblock = linear_v2::SuperBlock {
            version: 3,
            newest_timestamp: self.get_newest_timestamp(),
            grid_size: grid_size,
            region_x: self.region_x,
            region_z: self.region_z,
        };
        superblock.write(f)?;

        let chunk_bitmap: Vec<bool> = self.chunks.iter().map(|x| !x.is_empty()).collect();
        let bitmap = linear_v2::ChunkBitMap {
            bit_map: <[bool; 1024]>::try_from(chunk_bitmap).expect("严重错误：单Region实例存储的区块数不为1024")
        };
        bitmap.write(f)?;

        linear_v2::serialize_hashmap(&self.nbt_features, f)?;

        let cpb = 32 / grid_size as usize;

        let mut buckets_data: Vec<Vec<u8>> =
            Vec::with_capacity((grid_size * grid_size) as usize);

        for bx in 0..grid_size as usize {
            for bz in 0..grid_size as usize {
                let mut data = Cursor::new(Vec::new());

                for ix in 0..cpb {
                    for iz in 0..cpb {
                        let global_x = bx * cpb + ix;
                        let global_z = bz * cpb + iz;
                        let idx = global_x + global_z * 32;

                        let chunk = &mut self.chunks[idx];

                        let chunk_data = linear_v2::BucketChunk {
                            // 如果区块为空的话chunk_size必须为0，但是timestamp照写，相当于不包含timestamp的长度
                            chunk_size: if chunk.is_empty() { 0 } else { chunk.raw_chunk.len() as u32 + 8 },
                            timestamp: chunk.timestamps,
                            chunk_data: std::mem::take(&mut chunk.raw_chunk),
                        };

                        chunk_data.write(&mut data)?;
                    }
                }

                buckets_data.push(data.into_inner());
            }
        }

        linear_v2::serialize_bucket(
            f,
            grid_size,
            buckets_data,
            compression_level
        )?;

        f.write_all(linear_v2::MAGIC)?;

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


fn linear_v2_parse_bucket_chunks(bucket: &[u8], chunks_per_bucket: usize) -> Vec<linear_v2::BucketChunk>
{
    let mut cursor = Cursor::new(bucket);
    let mut result = Vec::new();

    while (cursor.position() as usize) < bucket.len() {
        match linear_v2::BucketChunk::read(&mut cursor) {
            Ok(chunk) => result.push(chunk),
            Err(_) => break,
        }
    }

    result
}
