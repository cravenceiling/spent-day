//! Framework proof: the event loop returns after each quit path, so `Tracking::stop` can flush
//! after it. winit builds one event loop per process, so every case runs in a child process of
//! this binary (`LIFECYCLE_CASE`), the way the app runs: `try_run_windows_with_args`, accessory.
//!
//! A child exits 0 when the loop returned and the code after it ran, 2 when no display exists
//! (Linux CI without X11 or Wayland), and anything else, or a hang past the deadline, fails.

use rune::{App, AppSpec, Cx, El, RunOptions, WindowSpec, el};
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

const CASES: [&str; 5] = [
    "handle-quit",
    "tx-quit",
    "sigterm",
    "sigterm-with-window",
    "sigint",
];
const NO_DISPLAY: i32 = 2;

fn view(_: &u32, _: &mut Cx<u32>) -> El {
    el().w_full().h_full()
}

fn child(case: &str) -> ExitCode {
    use signal_hook::consts::{SIGINT, SIGTERM};
    let app = App::new(AppSpec::new("lifecycle", (200.0, 120.0), || 0u32, view)).windowless();
    let handle = app.handle();
    if case == "sigterm-with-window" {
        handle.open();
    }
    let mut signals = signal_hook::iterator::Signals::new([SIGTERM, SIGINT]).unwrap();
    let on_signal = handle.clone();
    std::thread::spawn(move || {
        if signals.forever().next().is_some() {
            on_signal.quit();
        }
    });
    let case_name = case.to_owned();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        match case_name.as_str() {
            "handle-quit" => handle.quit(),
            "tx-quit" => handle.update(|_, tx| tx.quit()),
            "sigint" => unsafe {
                libc::kill(libc::getpid(), SIGINT);
            },
            _ => unsafe {
                libc::kill(libc::getpid(), SIGTERM);
            },
        }
    });
    let started = Instant::now();
    let result = rune::try_run_windows_with_args(
        RunOptions { accessory: true },
        Vec::<String>::new(),
        move |_| vec![WindowSpec::from_app("main", app).boxed()],
    );
    match result {
        Ok(()) => {
            println!(
                "loop returned after {} ms; code after the loop ran",
                started.elapsed().as_millis()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            println!("{e}");
            ExitCode::from(NO_DISPLAY as u8)
        }
    }
}

fn main() -> ExitCode {
    if let Ok(case) = std::env::var("LIFECYCLE_CASE") {
        return child(&case);
    }
    let exe = std::env::current_exe().unwrap();
    let mut failed = false;
    for case in CASES {
        let mut proc = Command::new(&exe)
            .env("LIFECYCLE_CASE", case)
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        let status = loop {
            if let Some(status) = proc.try_wait().unwrap() {
                break Some(status);
            }
            if Instant::now() > deadline {
                let _ = proc.kill();
                let _ = proc.wait();
                break None;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let out = proc
            .wait_with_output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
            .unwrap_or_default();
        match status.map(|s| s.code()) {
            Some(Some(0)) => println!("lifecycle {case}: ok, {out}"),
            Some(Some(NO_DISPLAY)) => println!("lifecycle {case}: skipped, {out}"),
            Some(code) => {
                println!("lifecycle {case}: FAILED, exit {code:?}, {out}");
                failed = true;
            }
            None => {
                println!("lifecycle {case}: FAILED, the loop did not return within 15 s");
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
