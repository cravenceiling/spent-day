// The skeleton declares contracts that later units fill in; unit 6 removes this allow.
#![allow(dead_code)]

mod app;
mod colors;
mod day;
mod demo;
mod format;
mod icons;
mod jobs;
mod layout;
mod palette;
mod platform;
mod scenes;
mod stats;
mod store;
mod sync;
mod ui;

use app::{Launch, Opts};
use std::process::ExitCode;

/// Spent Day's own flags; everything else is Rune's (`--snap`, `--scene`, `--window`, ...).
fn parse(argv: impl IntoIterator<Item = String>) -> Opts {
    let mut opts = Opts {
        launch: Launch::Default,
        day: None,
        demo: false,
        rune: Vec::new(),
    };
    let mut argv = argv.into_iter();
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--background" | "-b" => opts.launch = Launch::Background,
            "--dashboard" => opts.launch = Launch::Dashboard,
            "--settings" => opts.launch = Launch::Settings,
            "--demo" => opts.demo = true,
            // A bad date shows today.
            "--day" => opts.day = argv.next().and_then(|d| d.parse().ok()),
            _ => opts.rune.push(arg),
        }
    }
    opts
}

fn main() -> ExitCode {
    app::run(parse(std::env::args().skip(1)))
}

#[cfg(test)]
mod tests {
    use super::{Launch, parse};
    use jiff::civil::date;

    #[test]
    fn own_flags_are_taken_and_the_rest_goes_to_rune() {
        let argv = [
            "--demo",
            "--snap",
            "out.png",
            "--day",
            "2026-10-28",
            "--scene",
            "day",
            "-b",
        ];
        let opts = parse(argv.map(String::from));
        assert!(opts.demo);
        assert_eq!(opts.day, Some(date(2026, 10, 28)));
        assert_eq!(opts.launch, Launch::Background);
        assert_eq!(opts.rune, ["--snap", "out.png", "--scene", "day"]);
        assert_eq!(parse(["--day", "28/10/2026"].map(String::from)).day, None);
    }
}
