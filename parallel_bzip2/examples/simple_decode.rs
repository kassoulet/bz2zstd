use anyhow::Result;
use parallel_bzip2::Bz2Decoder;
use std::env;
use std::fs::File;
use std::io::{Read, Write};

fn main() -> Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <input.bz2> [output]", args[0]);
        return Ok(());
    }

    let input_path = &args[1];
    let mut decoder = Bz2Decoder::open(input_path)?;
    let mut buffer = [0u8; 8192];
    let mut out: Box<dyn Write> = if args.len() > 2 {
        Box::new(File::create(&args[2])?)
    } else {
        Box::new(std::io::stdout())
    };

    loop {
        let n = decoder.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        out.write_all(&buffer[..n])?;
    }

    Ok(())
}
