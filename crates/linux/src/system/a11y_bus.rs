use std::os::unix::fs::MetadataExt;
use std::time::Duration;

use agent_desktop_core::{AdapterError, Deadline, ErrorCode};
use zbus::Connection;

const A11Y_BUS_NAME: &str = "org.a11y.Bus";
const A11Y_BUS_PATH: &str = "/org/a11y/bus";
const A11Y_ADDRESS_ENV: &str = "AT_SPI_BUS_ADDRESS";
const SESSION_ADDRESS_ENV: &str = "DBUS_SESSION_BUS_ADDRESS";
const METHOD_TIMEOUT: Duration = Duration::from_secs(5);

const ENABLE_INSTRUCTIONS: &str = "AT-SPI2 must be reachable on the D-Bus session bus. \
GNOME: AT-SPI2 is enabled by default; check `gsettings get org.gnome.desktop.interface toolkit-accessibility` and that `at-spi-bus-launcher` runs in your login session. \
Other desktops: install `at-spi2-core` and ensure `at-spi-bus-launcher` is running. \
Flatpak/Snap: grant `--talk-name=org.a11y.Bus`. \
Over SSH, run agent-desktop as the user who owns the graphical session.";

/// One connection to the AT-SPI2 accessibility bus, shared by every read an
/// adapter instance makes so a command never reconnects per element.
pub(crate) struct A11yBus {
    connection: Connection,
}

impl A11yBus {
    pub(crate) fn connection(&self) -> &Connection {
        &self.connection
    }
}

/// Connects to the accessibility bus, resolving its address the same way
/// libatspi does: `AT_SPI_BUS_ADDRESS` first, then `org.a11y.Bus.GetAddress`
/// on the session bus. A session bus that is not advertised through the
/// environment (SSH, an MCP client that narrows the environment) is found at
/// the login session's runtime directory.
pub(crate) fn connect(deadline: Deadline) -> Result<A11yBus, AdapterError> {
    zbus::block_on(crate::tree::deadline::within(deadline, async {
        let address = resolve_address().await?;
        let connection = zbus::connection::Builder::address(address.as_str())
            .map_err(unavailable)?
            .method_timeout(METHOD_TIMEOUT)
            .build()
            .await
            .map_err(unavailable)?;
        Ok(A11yBus { connection })
    }))
}

async fn resolve_address() -> Result<String, AdapterError> {
    if let Some(address) = non_empty_env(A11Y_ADDRESS_ENV) {
        return Ok(address);
    }
    let session = session_connection().await?;
    let reply = session
        .call_method(
            Some(A11Y_BUS_NAME),
            A11Y_BUS_PATH,
            Some(A11Y_BUS_NAME),
            "GetAddress",
            &(),
        )
        .await
        .map_err(unavailable)?;
    let address = reply.body().deserialize::<String>().map_err(unavailable)?;
    if address.is_empty() {
        return Err(unavailable("org.a11y.Bus returned an empty address"));
    }
    Ok(address)
}

async fn session_connection() -> Result<Connection, AdapterError> {
    let address = match non_empty_env(SESSION_ADDRESS_ENV) {
        Some(address) => address,
        None => format!("unix:path={}/bus", runtime_dir()?),
    };
    zbus::connection::Builder::address(address.as_str())
        .map_err(unavailable)?
        .method_timeout(METHOD_TIMEOUT)
        .build()
        .await
        .map_err(unavailable)
}

fn runtime_dir() -> Result<String, AdapterError> {
    if let Some(directory) = non_empty_env("XDG_RUNTIME_DIR") {
        return Ok(directory);
    }
    let uid = std::fs::metadata("/proc/self").map_err(unavailable)?.uid();
    Ok(format!("/run/user/{uid}"))
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn unavailable(detail: impl std::fmt::Display) -> AdapterError {
    AdapterError::new(
        ErrorCode::PlatformNotSupported,
        "The AT-SPI2 accessibility bus is not reachable",
    )
    .with_suggestion(ENABLE_INSTRUCTIONS)
    .with_platform_detail(detail.to_string())
}
