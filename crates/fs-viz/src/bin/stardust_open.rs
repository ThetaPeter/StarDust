#![windows_subsystem = "windows"]

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    fs_viz::shell::trampoline_main(&args);
}
