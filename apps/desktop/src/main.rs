mod app;
mod colors;
mod day;
mod demo;
mod format;
#[cfg(target_os = "linux")]
mod gpu;
#[cfg(target_os = "linux")]
mod helpers;
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
    let mut argv = argv.into_iter().peekable();
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--background" | "-b" => opts.launch = Launch::Background,
            "--dashboard" => opts.launch = Launch::Dashboard,
            "--settings" => opts.launch = Launch::Settings,
            "--demo" => opts.demo = true,
            // A bad date shows today.
            "--day" => {
                opts.day = Some(
                    argv.next_if(|value| !value.starts_with('-'))
                        .and_then(|d| d.parse().ok())
                        .unwrap_or_else(|| day::today(&jiff::tz::TimeZone::system())),
                );
            }
            _ => opts.rune.push(arg),
        }
    }
    opts
}

fn main() -> ExitCode {
    #[cfg(target_os = "linux")]
    if std::env::args().any(|arg| arg == "--install-helpers") {
        return match helpers::install() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("Could not install desktop helpers: {error}");
                ExitCode::FAILURE
            }
        };
    }
    #[cfg(target_os = "linux")]
    if std::env::args().any(|arg| arg == "--enable-autostart") {
        return match platform::set_autostart(true) {
            Ok(_) => {
                println!("Spent Day will start at login. Settings > Launch at login turns it off.");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("{error}");
                ExitCode::FAILURE
            }
        };
    }
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
        assert_eq!(
            parse(["--day", "28/10/2026"].map(String::from)).day,
            Some(crate::day::today(&jiff::tz::TimeZone::system()))
        );
    }

    #[test]
    fn last_launch_flag_wins() {
        for (args, expected) in [
            (vec![], Launch::Default),
            (vec!["--background"], Launch::Background),
            (vec!["-b"], Launch::Background),
            (vec!["-b", "--dashboard"], Launch::Dashboard),
            (vec!["--dashboard", "--settings"], Launch::Settings),
            (vec!["--settings", "--background"], Launch::Background),
        ] {
            assert_eq!(parse(args.into_iter().map(String::from)).launch, expected);
        }
    }

    #[test]
    fn missing_or_invalid_day_keeps_following_flags() {
        let today = Some(crate::day::today(&jiff::tz::TimeZone::system()));
        for value in ["2026-02-30", "not-a-day", "2026-13-01"] {
            assert_eq!(parse(["--day", value].map(String::from)).day, today);
        }
        assert_eq!(parse(["--day".into()]).day, today);
        let opts = parse(["--day", "--settings", "--tree", "out.json"].map(String::from));
        assert_eq!(opts.day, today);
        assert_eq!(opts.launch, Launch::Settings);
        assert_eq!(opts.rune, ["--tree", "out.json"]);
        assert_eq!(
            parse(["--demo", "--day", "bad"].map(String::from)).day,
            today
        );
    }

    #[test]
    fn unknown_flags_and_values_keep_their_order() {
        let opts = parse(["--future", "value", "--demo", "--window", "fatal"].map(String::from));
        assert!(opts.demo);
        assert_eq!(opts.rune, ["--future", "value", "--window", "fatal"]);
    }
}
