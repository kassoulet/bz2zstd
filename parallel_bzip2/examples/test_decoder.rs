use parallel_bzip2::Bz2Decoder;
use std::io::Read;

fn main() {
    eprintln!("Starting test...");

    let path = std::env::args()
        .nth(1)
        .expect("Usage: test_decoder <file.bz2>");
    eprintln!("Opening file: {}", path);

    let decoder = Bz2Decoder::open(&path);
    eprintln!("Decoder created: {:?}", decoder.is_ok());

    if let Ok(mut decoder) = decoder {
        eprintln!("Reading data...");
        let mut buffer = Vec::new();
        match decoder.read_to_end(&mut buffer) {
            Ok(n) => {
                eprintln!("Read {} bytes", n);
                println!("Output length: {}", buffer.len());
            }
            Err(e) => {
                eprintln!("Error reading: {}", e);
            }
        }
    } else {
        eprintln!("Failed to create decoder");
    }

    eprintln!("Done");
}
