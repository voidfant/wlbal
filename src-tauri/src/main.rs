fn main() {
    let command = std::env::args().nth(1);
    if matches!(
        command.as_deref(),
        Some("status" | "switch" | "pause" | "resume")
    ) {
        wlbal_lib::run_cli_client();
    } else {
        wlbal_lib::run();
    }
}
