mod utils;
use std::error::Error;
use std::fs;
use std::fs::File;
use std::path::{PathBuf};
use std::thread::{spawn, JoinHandle};

use clap::{Args, Parser, Subcommand};

use utils::*;
use mclinear::region::Region;
use mclinear::utils::{get_file_type, parse_region_coords, FileType};

#[derive(Parser)]
#[command(version, about)]
struct Main
{
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands
{
    #[command(name = "to-linear-v1")]
    ToLinearV1(ConvertArgs),
    #[command(name = "to-linear-v2", visible_alias = "to-linear")]
    ToLinearV2(ConvertArgs),
    #[command(name = "to-anvil")]
    ToAnvil(ConvertArgs),
}

#[derive(Args, Debug, Clone)]
struct ConvertArgs {
    /// 输入文件的目录
    input_path: PathBuf,

    /// 输出的目录
    output_path: PathBuf,

    /// 压缩等级
    #[arg(
        long,
        default_value_t = 1,
        long_help = "压缩等级，具体范围由格式所采用的压缩算法决定\n\
                     linear 系列 (zstd): 1-22\n\
                     anvil 系列 (deflate): 1-9"
    )]
    #[arg(long, default_value_t = 1)]
    compress_level: i32,

    /// 控制转换的线程数，默认为cpu线程数
    #[arg(long, default_value_t = get_cpu_num())]
    cpu_num: usize,

    /// linearv2特有参数，非linearv2时无效
    #[arg(
        long,
        default_value_t = 1,
        long_help = "linearv2特有参数，转换目标非linearv2时无效\n\
                     用于控制分桶的数量，只能为1, 2, 4, 8, 16, 32\n\
                     一般来说1压缩率最大，32压缩率最小"
    )]
    grid_size:  i8,

    /// 是否遍历多层目录，用于方便整个村的的转换
    #[arg(long, default_value_t = false)]
    walk: bool
}

fn main() {
    let args = Main::parse();

    match args.command {
        Commands::ToAnvil(args) => {
            handle_command(FileType::Anvil, args).expect("转换失败");
        }
        Commands::ToLinearV1(args) => {
            handle_command(FileType::LinearV1, args).expect("转换失败");
        }
        Commands::ToLinearV2(args) => {
            handle_command(FileType::LinearV2, args).expect("转换失败");
        }
    }

}


fn handle_command(to: FileType, args: ConvertArgs) -> Result<(), Box<dyn Error + Sync + Send>>
{
    if ![1, 2, 4 ,8 ,16 ,32].contains(&args.grid_size)
    {
        return Err("grid_size只能为1,2,4,8,16,32".into())
    }
    if !args.input_path.is_dir() || !args.input_path.exists()
    {
        return Err("输入的目录不是目录或者不存在".into())
    }
    if !args.output_path.is_dir() || !args.output_path.exists()
    {
        fs::create_dir_all(&args.output_path)?;
        println!("输出目录不存在，已自动创建");
    }

    let max_depth = if args.walk {114} else {1};

    let mut files = get_dir_file(&args.input_path, "mca", max_depth)?;
    files.extend(get_dir_file(&args.input_path, "linear", max_depth)?);
    println!("收集存档文件完成");

    let files_thread: Vec<Vec<PathBuf>> = split_into_chunks(files, args.cpu_num);

    let mut handles: Vec<JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>> =
        Vec::with_capacity(files_thread.len());

    for subfiles in files_thread {
        let thread_args = args.clone();
        let to = to.clone();
        handles.push(spawn(move || {
            handle_convert(subfiles, to, thread_args)
        }));
    }

    for handle in handles {
        handle.join()
            .map_err(|e| format!("线程 panic: {:?}", e))??;
    }

    println!("转换完成！");

    Ok(())
}


fn handle_convert(files: Vec<PathBuf>, to: FileType, args: ConvertArgs) -> Result<(), Box<dyn Error + Send + Sync>>
{
    for path in files
    {
        if fs::metadata(&path)?.len() == 0
        {
            continue;
        }

        let file_type: FileType;
        {
            let mut file = File::open(&path)?;
            file_type = get_file_type(&mut file)?;
        }

        let r: Region;
        match file_type {
            FileType::Anvil => {
                r = Region::from_anvil(&path).expect(format!("读取 {} 时失败", path.display()).as_str());
            }
            FileType::LinearV1 => {
                let (region_x, region_z) = parse_region_coords(&path).expect(format!("解析 {} 的区域坐标时失败", path.display()).as_str());
                let file = File::open(&path)?;
                r = Region::from_linear_v1(file, region_x, region_z).expect(format!("读取 {} 时失败", path.display()).as_str());
            }
            FileType::LinearV2 => {
                let file = File::open(&path)?;
                r = Region::from_linear_v2(file).expect(format!("读取 {} 时失败", path.display()).as_str());
            }
        }

        if !path.parent().unwrap().exists()
        {
            fs::create_dir_all(path.parent().unwrap())?
        }

        let file_name = path.file_name().unwrap();
        let output_file_path: PathBuf;

        let output_path: PathBuf;
        if args.walk
        {
            let input_path_strip_prefix = path.strip_prefix(&args.input_path)?;
            output_path = args.output_path.join(input_path_strip_prefix.parent().unwrap()).to_path_buf();
            if !output_path.exists()
            {
                fs::create_dir_all(&output_path)?;
            }
        }
        else
        {
            output_path = args.output_path.clone();
        }

        match to {
            FileType::Anvil => {
                output_file_path = output_path.join(file_name).with_extension("mca");
                let mut output_file = File::create(&output_file_path)?;
                r.to_anvil(args.compress_level as u8, 2, &mut output_file, &output_file_path)
                    .expect(format!("转换 {} 时失败", path.display()).as_str());
            }
            FileType::LinearV1 => {
                output_file_path = output_path.join(file_name).with_extension("linear");
                let mut output_file = File::create(&output_file_path)?;
                r.to_linear_v1(&mut output_file, args.compress_level)
                    .expect(format!("转换 {} 时失败", path.display()).as_str());
            }
            FileType::LinearV2 => {
                output_file_path = output_path.join(file_name).with_extension("linear");
                let mut output_file = File::create(&output_file_path)?;
                r.to_linear_v2(&mut output_file, args.compress_level, args.grid_size)
                    .expect(format!("转换 {} 时失败", path.display()).as_str());
            }
        }
        println!("转换 {} -> {} 成功！", path.display(), output_file_path.display());
    }

    Ok(())
}
