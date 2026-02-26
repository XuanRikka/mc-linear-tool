use region::region::Region;
use region::utils::parse_region_coords;

use std::fs::File;


fn main() {
    let a = "r.0.0.linear";

    let (rx,rz) = parse_region_coords(a).expect("解析输入文件的文件名失败");

    let f = File::open(a).expect("打开文件失败");
    let region = Region::from_linear_v1(f,rx,rz).expect("解析区域文件失败");
    println!("{:?}",region);
    println!("{:?}",region.hash());
}
