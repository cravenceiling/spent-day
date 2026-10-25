use std::time::Duration;
use zbus::blocking::{Connection, Proxy, connection, proxy::Builder};
use zbus::proxy::CacheProperties;

/// How long a method call may take. The tracker thread makes these calls, and a service that
/// stops answering must not stall tracking or quitting.
pub const TIMEOUT: Duration = Duration::from_secs(1);

pub fn session() -> zbus::Result<Connection> {
    connection::Builder::session()?.method_timeout(TIMEOUT).build()
}

pub fn system() -> zbus::Result<Connection> {
    connection::Builder::system()?.method_timeout(TIMEOUT).build()
}

/// A proxy that never prefetches properties, so creating one costs no round trip.
pub fn proxy(conn: &Connection, dest: &str, path: &str, iface: &str) -> zbus::Result<Proxy<'static>> {
    Builder::new(conn)
        .destination(dest.to_owned())?
        .path(path.to_owned())?
        .interface(iface.to_owned())?
        .cache_properties(CacheProperties::No)
        .build()
}
