use region::region::Region;

use std::fs::File;
use std::hash::Hash;
use std::path::PathBuf;

fn main() {
    let a = "r.0.0.mca";
    let r = Region::from_anvil(PathBuf::from(a)).unwrap();
    println!("{:016x}", r.hash());

    let b = "r.0.0.mca2";
    let file = File::create(b).unwrap();
    r.to_anvil(6, 2, file, PathBuf::from(b)).expect("REASON");
    let r2 = Region::from_anvil(PathBuf::from(b)).unwrap();
    println!("{:016x}", r2.hash());
}

