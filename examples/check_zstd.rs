fn main() {
    for level in [-10, 0, 3, 9, 22, 23, 100] {
        let c = zstd::bulk::Compressor::new(level);
        println!("Level {}: {:?}", level, c.is_ok());
    }
}
