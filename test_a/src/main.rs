use region::region::Region;

use std::fs::File;
use std::hash::Hash;

fn main() {
    let a = "r.0.0.linearv2";
    let mut file = File::open(a).unwrap();
    let r = Region::from_linear_v2(file).unwrap();
    println!("{:016x}", r.hash());

    {
        let b = "r.0.0.linearv22";
        let mut file2 = File::create(b).unwrap();
        r.to_linear_v2(&mut file2, 1, 2).unwrap();
    }

    let b = "r.0.0.linearv22";
    let mut file3 = File::open(b).unwrap();
    let r = Region::from_linear_v2(file3).unwrap();
    println!("{:016x}", r.hash());
}

