use region::models::linear_v2::*;

use std::fs::File;
use std::io::Write;

use binrw::{BinRead, BinWrite};
use sha2::{Sha256, Digest};
use hex;

fn main() {
    let a = "r.0.0.linearv2";
    let mut f = File::open(a).unwrap();
    println!("{:?}", SuperBlock::read(&mut f).unwrap());
    let bitmap = ChunkBitMap::read(&mut f).unwrap();
    println!("{:?}", deserialize_hashmap(&mut f).unwrap())
}

