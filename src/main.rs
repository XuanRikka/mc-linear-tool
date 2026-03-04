mod utils;
use std::error::Error;
use std::fs;
use std::fs::File;
use std::path::{PathBuf};
use std::thread::{available_parallelism, spawn, JoinHandle};

use clap::{Args, Parser, Subcommand};

use utils::get_dir_file;
use region::region::Region;
use region::utils::{get_file_type, parse_region_coords, FileType};

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
    #[command(name = "tolinearv1")]
    ToLinearV1(ConvertArgs),
    #[command(name = "tolinearv2")]
    ToLinearV2(ConvertArgs),
    #[command(name = "toanvil")]
    ToAnvil(ConvertArgs),
}

#[derive(Args, Debug, Clone)]
struct ConvertArgs {
    input_path: PathBuf,

    output_path: PathBuf,

    #[arg(long, default_value_t = 1)]
    compress_level: i32,

    #[arg(long, default_value_t = get_cpu_num())]
    cpu_num: usize,
}

fn get_cpu_num() -> usize
{
    available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}

fn split_into_chunks<T>(mut vec: Vec<T>, n: usize) -> Vec<Vec<T>> {
    if n == 0 || vec.is_empty() {
        return vec![];
    }

    let mut chunks = Vec::with_capacity(n.min(vec.len()));

    for i in 0..n {
        if vec.is_empty() {
            break;
        }

        let remaining_chunks = n - i;
        let current_chunk_size = (vec.len() + remaining_chunks - 1) / remaining_chunks;

        if i == n - 1 {
            chunks.push(vec);
            break;
        }

        let chunk = vec.split_off(current_chunk_size);
        chunks.push(chunk);
    }

    chunks
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
    if !args.input_path.is_dir() || !args.input_path.exists()
    {
        return Err("输入的目录不是目录或者不存在".into())
    }
    if !args.output_path.is_dir() || !args.output_path.exists()
    {
        fs::create_dir_all(&args.output_path)?;
        println!("输出目录不存在，已自动创建");
    }

    let mut files = get_dir_file(&args.input_path, "mca", 1)?;
    files.extend(get_dir_file(&args.input_path, "linear", 1)?);
    println!("收集存档文件完成");

    let files_thread: Vec<Vec<PathBuf>> = split_into_chunks(files, args.cpu_num);

    let mut handles: Vec<JoinHandle<Result<(), Box<dyn Error + Send + Sync>>>> =
        Vec::with_capacity(files_thread.len());

    for subfiles in files_thread {
        let output_path = args.output_path.clone();
        let compress_level = args.compress_level.clone();
        let to = to.clone();
        handles.push(spawn(move || {
            handle_convert(subfiles, to, output_path, compress_level)
        }));
    }

    for handle in handles {
        handle.join()
            .map_err(|e| format!("线程 panic: {:?}", e))??;
    }

    println!("转换完成！");

    Ok(())
}


fn handle_convert(files: Vec<PathBuf>, to: FileType, output: PathBuf,
                 compression_level: i32) -> Result<(), Box<dyn Error + Send + Sync>>
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

        let file_name = path.file_name().unwrap();
        let output_file_path: PathBuf;
        match to {
            FileType::Anvil => {
                output_file_path = output.join(file_name).with_extension("mca");
                let mut output_file = File::create(&output_file_path)?;
                r.to_anvil(compression_level as u8, 2, &mut output_file, &output_file_path)
                    .expect(format!("转换 {} 时失败", path.display()).as_str());
            }
            FileType::LinearV1 => {
                output_file_path = output.join(file_name).with_extension("linear");
                let mut output_file = File::create(&output_file_path)?;
                r.to_linear_v1(&mut output_file, compression_level)
                    .expect(format!("转换 {} 时失败", path.display()).as_str());
            }
            FileType::LinearV2 => {
                output_file_path = output.join(file_name).with_extension("linear");
                let mut output_file = File::create(&output_file_path)?;
                r.to_linear_v2(&mut output_file, compression_level, 1)
                    .expect(format!("转换 {} 时失败", path.display()).as_str());
            }
        }
        println!("转换 {} -> {} 成功！", path.display(), output_file_path.display());
    }

    Ok(())
}
