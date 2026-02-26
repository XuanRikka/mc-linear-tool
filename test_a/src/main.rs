use region::region::Region;

use std::fs::File;
use std::hash::Hash;

fn main() {
    let a = "r.0.0.linearv2";
    let mut file = File::open(a).unwrap();
    let r = Region::from_linear_v2(file).unwrap();
    println!("{:016x}", r.hash());
    // for (i,c) in r.chunks.iter().enumerate()
    // {
    //     println!("Chunk{}: ({},{})",i,c.x,c.z)
    // }
}

