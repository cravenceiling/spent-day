mod app;
mod collector;
mod dbus;
mod demo;
mod desktop;
mod format;
mod idle;
mod media;
mod paths;
mod range;
mod session;
mod store;
mod tracker;
mod ui;

fn main() -> gtk::glib::ExitCode {
    app::run()
}
