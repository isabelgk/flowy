mod cli;
mod commands;
mod core;
mod output;
mod web;

fn main() {
    let matches = cli::build().get_matches();
    let json = matches.get_flag("json");
    if let Err(e) = commands::dispatch(matches) {
        if json {
            output::eprint_error_json(&e);
        } else {
            eprintln!("Error: {e:#}");
        }
        std::process::exit(1);
    }
}
