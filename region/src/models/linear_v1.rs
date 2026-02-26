use binrw::{BinRead,BinWrite};

#[derive(BinRead, BinWrite, Debug)]
#[brw(little)]
pub struct SuperBlock
{
    #[brw(magic = b"\xC3\xFF\x13\x18\x3C\xCA\x9D\x9A")]
    pub version: u8,
    pub newest_timestamp: u64,
    pub compression_level: i8,
    pub chunk_count: i16,
    pub compressed_data_length: u32,
    pub reserved: u64
}

#[derive(BinRead, BinWrite, Debug)]
#[brw(little)]
pub struct ChunkHeaders
{
    #[br(count = 1024)]
    pub chunk_headers: Vec<ChunkHeader>,
}

#[derive(BinRead, BinWrite, Debug)]
#[brw(little)]
pub struct ChunkHeader
{
    pub size: u32,
    pub timestamp: u32
}