use serde::de::DeserializeOwned;
use zbus::Connection;
use zbus::zvariant::{DynamicType, OwnedObjectPath, OwnedValue, Type};

pub(crate) const REGISTRY_BUS: &str = "org.a11y.atspi.Registry";
pub(crate) const ROOT_PATH: &str = "/org/a11y/atspi/accessible/root";
const NULL_PATH: &str = "/org/a11y/atspi/null";

pub(crate) const ACCESSIBLE: &str = "org.a11y.atspi.Accessible";
pub(crate) const APPLICATION: &str = "org.a11y.atspi.Application";
pub(crate) const COMPONENT: &str = "org.a11y.atspi.Component";
pub(crate) const ACTION: &str = "org.a11y.atspi.Action";
pub(crate) const VALUE: &str = "org.a11y.atspi.Value";
pub(crate) const TEXT: &str = "org.a11y.atspi.Text";
pub(crate) const EDITABLE_TEXT: &str = "org.a11y.atspi.EditableText";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
const DBUS_NAME: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";

pub(crate) const COORD_SCREEN: u32 = 0;
pub(crate) const COORD_WINDOW: u32 = 1;

/// An AT-SPI2 object reference: the owning application's unique bus name
/// and the object's path on it. AT-SPI2 has no handle to retain, so this pair
/// is the element identity every read addresses.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ElementRef {
    pub(crate) bus: String,
    pub(crate) path: String,
}

/// The wire shape of an object reference, `(so)`.
pub(crate) type WireRef = (String, OwnedObjectPath);

impl ElementRef {
    pub(crate) fn registry_root() -> Self {
        Self {
            bus: REGISTRY_BUS.to_string(),
            path: ROOT_PATH.to_string(),
        }
    }

    /// Converts a wire reference, dropping the null object some toolkits
    /// return in place of a missing child.
    pub(crate) fn from_wire(wire: WireRef) -> Option<Self> {
        let (bus, path) = wire;
        let path = path.as_str().to_string();
        if bus.is_empty() || path == NULL_PATH {
            return None;
        }
        Some(Self { bus, path })
    }

    pub(crate) fn application_root(&self) -> Self {
        Self {
            bus: self.bus.clone(),
            path: ROOT_PATH.to_string(),
        }
    }
}

pub(crate) async fn call<B, R>(
    connection: &Connection,
    element: &ElementRef,
    interface: &str,
    method: &str,
    body: &B,
) -> zbus::Result<R>
where
    B: serde::Serialize + DynamicType,
    R: DeserializeOwned + Type,
{
    let reply = connection
        .call_method(
            Some(element.bus.as_str()),
            element.path.as_str(),
            Some(interface),
            method,
            body,
        )
        .await?;
    reply.body().deserialize::<R>()
}

/// Reads one property with `org.freedesktop.DBus.Properties.Get`.
///
/// Never `GetAll`, which is also why libatspi does not use it: when one getter
/// fails inside a `GetAll` reply, at-spi2-atk leaves that dictionary entry
/// without a value and libdbus aborts the whole application. Chromium fails
/// `Value.MinimumValue` on its scroll bars, so a single `GetAll` on one took
/// Chrome down during development, while `Get` of the same property returns
/// an ordinary D-Bus error.
pub(crate) async fn property(
    connection: &Connection,
    element: &ElementRef,
    interface: &str,
    name: &str,
) -> zbus::Result<OwnedValue> {
    call(connection, element, PROPERTIES, "Get", &(interface, name)).await
}

pub(crate) async fn string_property(
    connection: &Connection,
    element: &ElementRef,
    interface: &str,
    name: &str,
) -> zbus::Result<String> {
    let value = property(connection, element, interface, name).await?;
    String::try_from(value).map_err(zbus::Error::from)
}

pub(crate) async fn int_property(
    connection: &Connection,
    element: &ElementRef,
    interface: &str,
    name: &str,
) -> zbus::Result<i32> {
    let value = property(connection, element, interface, name).await?;
    i32::try_from(value).map_err(zbus::Error::from)
}

pub(crate) async fn float_property(
    connection: &Connection,
    element: &ElementRef,
    interface: &str,
    name: &str,
) -> zbus::Result<f64> {
    let value = property(connection, element, interface, name).await?;
    f64::try_from(value).map_err(zbus::Error::from)
}

pub(crate) async fn children(
    connection: &Connection,
    element: &ElementRef,
) -> zbus::Result<Vec<ElementRef>> {
    let wire: Vec<WireRef> = call(connection, element, ACCESSIBLE, "GetChildren", &()).await?;
    Ok(wire.into_iter().filter_map(ElementRef::from_wire).collect())
}

pub(crate) async fn process_id(connection: &Connection, bus: &str) -> zbus::Result<u32> {
    let reply = connection
        .call_method(
            Some(DBUS_NAME),
            DBUS_PATH,
            Some(DBUS_NAME),
            "GetConnectionUnixProcessID",
            &bus,
        )
        .await?;
    reply.body().deserialize::<u32>()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire(bus: &str, path: &str) -> WireRef {
        (bus.to_string(), OwnedObjectPath::try_from(path).unwrap())
    }

    #[test]
    fn the_null_object_is_not_an_element() {
        assert!(ElementRef::from_wire(wire(":1.4", NULL_PATH)).is_none());
        assert!(ElementRef::from_wire(wire("", "/org/a11y/atspi/accessible/3")).is_none());
    }

    #[test]
    fn a_child_reference_keeps_its_bus_and_path() {
        let element = ElementRef::from_wire(wire(":1.4", "/org/a11y/atspi/accessible/3")).unwrap();
        assert_eq!(element.bus, ":1.4");
        assert_eq!(element.path, "/org/a11y/atspi/accessible/3");
        assert_eq!(element.application_root().path, ROOT_PATH);
    }
}
