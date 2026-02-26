use std::collections::HashMap;
use std::error::Error;
use std::io;
use std::io::{Cursor, Read, Seek, Write};

use binrw::{BinRead, BinWrite};
use zstd::{decode_all};
use zstd::stream::write::Encoder;
use zstd::zstd_safe::CompressionLevel;
use xxhash_rust::xxh64::xxh64;

pub const MAGIC: &[u8; 8] = b"\xC3\xFF\x13\x18\x3C\xCA\x9D\x9A";

#[derive(BinRead, BinWrite, Debug)]
#[brw(big)]
pub struct SuperBlock
{
    #[brw(magic = b"\xC3\xFF\x13\x18\x3C\xCA\x9D\x9A")]
    pub version: u8,
    pub newest_timestamp: u64,
    pub grid_size: i8,
    pub region_x: i32,
    pub region_z: i32,
}


#[derive(BinRead, BinWrite, Debug)]
#[brw(big)]
pub struct ChunkBitMap {
    #[br(map = |x| deserialize_bitmap(&x))]
    #[bw(map = |x| serialize_bitmap(&x))]
    pub bit_map: [bool; 1024],
}


#[derive(BinRead, BinWrite, Debug)]
#[brw(big)]
pub struct BucketHeader
{
    pub bucket_size: u32,
    pub compress_level: i8,
    pub xxhash64: u64
}


#[derive(BinRead, BinWrite, Debug)]
#[brw(big)]
pub struct BucketChunk {
    pub chunk_size: u32,
    pub timestamp: u64,

    #[br(count = chunk_data_len(chunk_size))]
    pub chunk_data: Vec<u8>,
}


fn chunk_data_len(size: u32) -> usize {
    if size < 8 {
        0
    } else {
        (size - 8) as usize
    }
}


pub fn serialize_bucket<W: Write + Seek>(writer: &mut W, grid_size: i8, bucket_datas: Vec<Vec<u8>>,
                                  compression_level: CompressionLevel)
                                  -> Result<(), Box<dyn Error>>
{
    if !matches!(grid_size, 1 | 2 | 4 | 8 | 16 | 32) {
        return Err(format!(
            "非法的 grid_size: {}，允许值为 1, 2, 4, 8, 16, 32",
            grid_size
        ).into());
    }

    let expected_bucket_count = (grid_size as usize) * (grid_size as usize);
    if bucket_datas.len() != expected_bucket_count {
        return Err(format!(
            "bucket 数量不匹配: 期望 {}，实际 {}",
            expected_bucket_count,
            bucket_datas.len()
        ).into());
    }

    let mut compression_data: Vec<Vec<u8>> = Vec::new();

    for data in bucket_datas
    {
        // 不知道为什么要写这个，但是python源代码里有，所以加上比较保险
        if data.len() == 64 {
            compression_data.push(Vec::new());
            continue;
        }

        let mut encoder = Encoder::new(Vec::new(), compression_level)?;
        encoder.include_checksum(true)?;
        encoder.write_all(&data)?;
        compression_data.push(
            encoder.finish()?
        )
    }

    let mut bucket_headers: Vec<BucketHeader> = Vec::new();
    for bucket in &compression_data
    {
        bucket_headers.push(
            BucketHeader {
                bucket_size: bucket.len() as u32,
                compress_level: compression_level as i8,
                xxhash64: xxh64(bucket, 0)
            }
        )
    }

    serialize_bucket_header(writer, &bucket_headers)?;

    for data in &compression_data
    {
        writer.write_all(data)?;
    }

    Ok(())
}


pub fn deserialize_bucket<R: Read + Seek>(render: &mut R, grid_size: i8)
    -> Result<Vec<Vec<u8>>, Box<dyn Error>>
{
    if !matches!(grid_size, 1 | 2 | 4 | 8 | 16 | 32) {
        return Err(format!("非法的 grid_size: {}，允许值为 1, 2, 4, 8, 16, 32", grid_size).into());
    }
    let bucket_headers = deserialize_bucket_header(render, grid_size)?;

    let compress_data_ = deserialize_bucket_data(
        render, bucket_headers, false
    )?;

    let mut decompress_data = Vec::new();

    for x in compress_data_ {
        let decoded = decode_all(Cursor::new(x))?;
        decompress_data.push(decoded);
    };

    Ok(decompress_data)
}



pub fn deserialize_bucket_data<R: Read>(render: &mut R, bucket_headers: Vec<BucketHeader>, ignore_hash: bool)
                                   -> Result<Vec<Vec<u8>>, Box<dyn Error>>
{
    let mut buckets: Vec<Vec<u8>> = Vec::new();

    for header in &bucket_headers
    {
        let mut buckets_data: Vec<u8> = vec![0u8; header.bucket_size as usize];
        render.read_exact(&mut buckets_data)?;
        let xxhash: u64 = xxh64(&buckets_data, 0);
        if xxhash != header.xxhash64 && !ignore_hash
        {
            return Err(format!(
                "xxhash校验失败，应为：{:016x}，实际为：{:016x}",
                header.xxhash64, xxhash).into()
            )
        }
        buckets.push(buckets_data);
    };

    Ok(buckets)
}


pub fn serialize_bucket_header<W: Write + Seek>(writer: &mut W, bucket_headers: &Vec<BucketHeader>)
    -> Result<(), Box<dyn Error>>
{
    let bucket_length = bucket_headers.len();
    if !matches!(bucket_length, 1 | 4 | 16 | 64 | 256 | 1024) {
        return Err(format!("非法的 bucket_length: {}，允许值为 1, 4, 16, 64, 256, 1024",
                           bucket_length).into());
    };

    for bucket_header in bucket_headers
    {
        bucket_header.write(writer)?;
    };

    Ok(())
}


pub fn deserialize_bucket_header<R: Read + Seek>(reader: &mut R, grid_size: i8)
    -> Result<Vec<BucketHeader>, Box<dyn Error>>
{
    if !matches!(grid_size, 1 | 2 | 4 | 8 | 16 | 32) {
        return Err(format!("非法的 grid_size: {}，允许值为 1, 2, 4, 8, 16, 32", grid_size).into());
    }

    let bucket_count = grid_size * grid_size;

    let mut bucket_headers: Vec<BucketHeader> = Vec::new();

    for _ in 0..bucket_count
    {
        bucket_headers.push(
            BucketHeader::read(reader)?
        );
    };

    Ok(bucket_headers)
}


pub fn serialize_hashmap<W: Write>(dict: &HashMap<String, u32>, writer: &mut W) -> io::Result<()> {
    for (key, value) in dict {
        let key_bytes = key.as_bytes();
        if key_bytes.len() > 255 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("字符串 '{}' 的长度超过了 255 字节", key),
            ));
        }
        writer.write_all(&[key_bytes.len() as u8])?;
        writer.write_all(key_bytes)?;
        writer.write_all(&value.to_be_bytes())?;
    }
    writer.write_all(&[0])?; // 结束标记
    Ok(())
}

pub fn deserialize_hashmap<R: Read>(reader: &mut R) -> io::Result<HashMap<String, u32>> {
    let mut map = HashMap::new();
    loop {
        let mut key_len_buf = [0u8; 1];
        reader.read_exact(&mut key_len_buf)?;
        let key_len = key_len_buf[0] as usize;
        if key_len == 0 {
            break; // 结束标记
        }

        let mut key_bytes = vec![0u8; key_len];
        reader.read_exact(&mut key_bytes)?;
        let key = String::from_utf8(key_bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "非法UTF-8键"))?;

        let mut value_bytes = [0u8; 4];
        reader.read_exact(&mut value_bytes)?;
        let value = u32::from_be_bytes(value_bytes);

        map.insert(key, value);
    }
    Ok(map)
}


pub fn serialize_bitmap(bits: &[bool; 1024]) -> [u8; 128] {
    let mut out = [0u8;128];

    for i in 0..128 {
        let mut byte = 0u8;
        for j in 0..8 {
            if bits[i*8 + j] {
                byte |= 1 << (7 - j);
            }
        }
        out[i] = byte;
    }

    out
}

pub fn deserialize_bitmap(data: &[u8; 128]) -> [bool; 1024] {
    let mut bits = [false; 1024];

    for i in 0..128 {
        let byte = data[i];
        for j in 0..8 {
            bits[i*8 + j] = (byte >> (7 - j)) & 1 == 1;
        }
    }

    bits
}
