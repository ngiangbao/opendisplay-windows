fn main() {
    od_sender::init_tracing();
    std::process::exit(od_sender::run_cli(std::env::args_os()));
}
