use std::env;
use std::fs::File;
use std::io::BufReader;

use rodio::{Decoder, Source};

fn main() {
    let mut bad = 0;
    for path in env::args().skip(1) {
        let open = File::open(&path);
        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
        let file = match open {
            Ok(f) => f,
            Err(err) => {
                println!("{name:<14} 打开失败: {err}");
                bad += 1;
                continue;
            }
        };
        match Decoder::new(BufReader::new(file)) {
            Ok(decoder) => {
                let rate = decoder.sample_rate().get();
                let channels = decoder.channels().get();
                let n = decoder.count();
                let secs = n as f64 / f64::from(rate);
                println!("{name:<14} OK   {rate} Hz  {channels}ch  {n} samples  {secs:.2}s");
            }
            Err(err) => {
                println!("{name:<14} 解码失败: {err}");
                bad += 1;
            }
        }
    }
    if bad > 0 {
        std::process::exit(1);
    }
}
