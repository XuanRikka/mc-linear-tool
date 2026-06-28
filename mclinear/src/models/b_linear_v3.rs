use std::error::Error;
use std::io::{Cursor, Read, Seek, Write};

use binrw::{BinRead, BinWrite};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use zstd::stream::write::{Encoder};
use zstd::stream::read::Decoder;
use zstd::zstd_safe::CompressionLevel;
use xxhash_rust::xxh32::xxh32;

use crate::region::Chunk;

pub const MAGIC: [u8; 8] = [0xFF, 0xFF, 0xDF, 0xF7, 0xED, 0xDA, 0xFD, 0x97];

#[derive(BinRead, BinWrite)]
#[brw(big)]
pub struct Header
{
    #[brw(magic = b"\xFF\xFF\xDF\xF7\xED\xDA\xFD\x97")]
    pub version: u8,
    pub zstd_level: i8,
    pub xxhash32_seed: u32
}

/// 记录了16个桶的偏移量
#[derive(BinRead, BinWrite)]
#[brw(big)]
pub struct BucketOffsetTable
{
    pub offsets: [u64; 16]
}

/// 区块数据
#[derive(Clone)]
pub struct ChunkData
{
    pub chunk_size: u32,
    pub write_timestamp: u64,
    pub xxhash_checksum: u32,
    pub data: Vec<u8>
}

pub fn write_chunks<W: Write + Seek>(writer: &mut W, all_chunk: Vec<Chunk>, seed: u32,
                                     compression_level: CompressionLevel) -> Result<[u64; 16], Box<dyn Error>>
{
    let mut offset_table = [0u64; 16];

    for (index, bucket) in all_chunk.chunks(64).enumerate()
    {
        // 判断是不是全都是空的
        if bucket.iter().all(|x| x.is_empty())
        {
            offset_table[index] = 0;
        }
        else
        {
            offset_table[index] = writer.stream_position()?;

            serialize_bucket(writer, bucket, seed, compression_level)?;
        }
    }

    Ok(offset_table)
}

/// 此函数用于解析全部bucket转换为Chunk结构体
pub fn read_chunks<R: Read>(reader: &mut R, offset_table: [u64; 16], seed: u32,
                            region_x: i32, region_z: i32) -> Result<Vec<Chunk>, Box<dyn Error>>
{
    let raw_chunk_data = deserialize_all_bucket(reader, offset_table)?;

    let mut chunk_data = Vec::with_capacity(1024);

    for (bucket_index, bucket) in raw_chunk_data.into_iter().enumerate()
    {
        for (chunk_index_in_bucket, chunk) in bucket.into_iter().enumerate()
        {
            let chunk_index =
                ((bucket_index as i64) << 6) + (chunk_index_in_bucket as i64); // 0-1023

            // 区域里的xz
            let x = chunk_index % 32;
            let z = chunk_index / 32;

            // 世界的xz
            let world_x = (region_x as i64) * 32 + (chunk_index % 32);
            let world_z = (region_z as i64) * 32 + (chunk_index / 32);

            if chunk.is_none()
            {
                chunk_data.push(
                    Chunk {
                        raw_chunk: Vec::new(),
                        timestamps: 0,
                        x: world_x,
                        z: world_z
                    }
                )
            }
            else
            {
                let chunk_ = chunk.unwrap();

                let checksum = xxh32(&chunk_.data, seed);

                if checksum != chunk_.xxhash_checksum
                {
                    println!("警告：区块({},{})校验失败，可能存在数据损坏", world_x, world_z)
                }

                chunk_data.push(
                    Chunk {
                        raw_chunk: chunk_.data,
                        timestamps: chunk_.write_timestamp,
                        x: world_x,
                        z: world_z
                    }
                )
            }
        }
    }

    Ok(chunk_data)
}


/// 此函数用于解析整个文件全部16个bucket
pub fn deserialize_all_bucket<R: Read>(reader: &mut R, offset_table: [u64; 16])
    -> Result<Vec<Vec<Option<ChunkData>>>, Box<dyn Error>>
{
    let mut result: Vec<Vec<Option<ChunkData>>> = Vec::with_capacity(16);

    for i in offset_table
    {
        if i == 0
        {
            result.push(vec![None; 64]);
            continue
        }
        result.push(
            deserialize_bucket(reader)?
        )
    }

    Ok(result)
}

/// 序列化一个bucket的数据，chunks要求长度必须为64
/// compression_level: -6 - 22
pub fn serialize_bucket<W: Write>(writer: &mut W, chunks: &[Chunk], seed: u32,
                                  compression_level: CompressionLevel) -> Result<(), Box<dyn Error>>
{
    let mut encoder = Encoder::new(Vec::new(), compression_level)?;

    _serialize_bucket_data(&mut encoder, chunks, seed)?;

    let data = encoder.finish()?;

    let mut raw_size: u32 = 0;
    for i in chunks
    {
        if i.is_empty()
        {
            raw_size += 4;
        }
        else
        {
            raw_size += 4+4+8+4+(i.raw_chunk.len() as u32)
        }
    }

    // 写入原始长度
    writer.write_u32::<BigEndian>(raw_size)?;
    // 写入压缩后数据长度
    writer.write_u32::<BigEndian>(data.len() as u32)?;
    // 写入实际数据
    writer.write_all(&data)?;

    Ok(())
}

/// 此函数用于解析一个bucket
pub fn deserialize_bucket<R: Read>(reader: &mut R)
    -> Result<Vec<Option<ChunkData>>, Box<dyn Error>>
{
    let raw_size = reader.read_u32::<BigEndian>()? as usize;
    let cmp_size = reader.read_u32::<BigEndian>()? as usize;

    let mut compress_buffer = vec![0u8; cmp_size];
    reader.read_exact(&mut compress_buffer)?;

    let mut decoder = Decoder::new(compress_buffer.as_slice())?;
    _deserialize_bucket_data(&mut decoder)
}

/// 序列化一个bucket的被压缩数据，chunks要求长度必须为64
fn _serialize_bucket_data<W: Write>(writer: &mut W, chunks: &[Chunk], seed: u32)
    -> Result<(), Box<dyn Error>>
{
    for i in chunks
    {
        if i.raw_chunk.len() != 0
        {
            let xxh32_checksum = xxh32(&i.raw_chunk, seed);

            writer.write_u32::<BigEndian>(4+8+4+(i.raw_chunk.len() as u32))?;

            writer.write_u32::<BigEndian>(i.raw_chunk.len() as u32)?;
            writer.write_u64::<BigEndian>(i.timestamps)?;
            writer.write_u32::<BigEndian>(xxh32_checksum)?;
            writer.write_all(&i.raw_chunk)?;
        }
        else
        {
            writer.write_u32::<BigEndian>(0)?;
        }
    }

    Ok(())
}

/// 此函数用于解析bucket解压后的数据
fn _deserialize_bucket_data<R: Read>(mut reader: R)
    -> Result<Vec<Option<ChunkData>>, Box<dyn Error>>
{
    let mut chunks = Vec::with_capacity(64);

    for _ in 0..64
    {
        let entry_size = reader.read_u32::<BigEndian>()?;

        if entry_size == 0
        {
            chunks.push(None);
            continue
        }

        let mut entry_data = vec![0u8; entry_size as usize];
        reader.read_exact(&mut entry_data)?;
        let mut cursor = Cursor::new(entry_data);

        let chunk_size = cursor.read_u32::<BigEndian>()?;
        let write_timestamp = cursor.read_u64::<BigEndian>()?;
        let xxhash_checksum = cursor.read_u32::<BigEndian>()?;

        let n = entry_size as usize - 16;
        let mut data = vec![0u8; n];
        cursor.read_exact(&mut data)?;

        chunks.push(Some(ChunkData {
            chunk_size,
            write_timestamp,
            xxhash_checksum,
            data,
        }));
    }

    Ok(chunks)
}
