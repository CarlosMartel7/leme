use std::process::ExitCode;

fn main() -> ExitCode {
    match leme::commands::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("leme: {error}");
            ExitCode::FAILURE
        }
    }
}
