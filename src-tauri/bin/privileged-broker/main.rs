#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let args = localbridge_lib::privilege::parse_broker_args(std::env::args().skip(1))
        .and_then(localbridge_lib::privilege::run_broker_process);
    if let Err(error) = args {
        eprintln!("LocalBridge privileged broker failed: {error}");
        std::process::exit(2);
    }
}
