// Add file-based logging to debug the crash
use std::fs::OpenOptions;
use std::io::Write;

fn log(msg: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open("C:\\Users\\Minik\\AppData\\Local\\Temp\\opencode\\faast-debug.txt") {
        let _ = writeln!(f, "{}", msg);
    }
}