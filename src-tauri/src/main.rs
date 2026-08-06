#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    match qingzhou_ssh_lib::run_process_mode(std::env::args_os()) {
        Ok(true) => return,
        Ok(false) => qingzhou_ssh_lib::run(),
        Err(_) => std::process::exit(1),
    }
}
