use std::env;

use music_shell::player::probe_duration;

fn main() {
    let mut bad = 0;
    for path in env::args().skip(1) {
        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
        match probe_duration(std::path::Path::new(&path)) {
            Some(d) => println!("{name:<32} {d:.2?}"),
            None => {
                println!("{name:<32} 解码失败");
                bad += 1;
            }
        }
    }
    if bad > 0 {
        std::process::exit(1);
    }
}
