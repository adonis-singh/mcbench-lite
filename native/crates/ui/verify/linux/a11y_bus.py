"""The session's stand-in for `at-spi-bus-launcher`: owns `org.a11y.Bus` on
the private session bus and answers what clients ask of it, the accessibility
bus's address and the `org.a11y.Status` properties.

    a11y_bus.py <accessibility bus address> <on|off>

The real launcher is not used: it picks `dbus-broker-launch --scope user`
where dbus-broker is installed (which talks to the user's systemd manager)
and writes `IsEnabled` to the user's dconf. Here the accessibility bus is a
plain `dbus-daemon` that session.sh started on a private socket, and the
properties live in this process only."""

import sys

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

ADDRESS = sys.argv[1]
STATUS = {"IsEnabled": sys.argv[2] == "on", "ScreenReaderEnabled": False}

XML = """
<node>
  <interface name="org.a11y.Bus">
    <method name="GetAddress"><arg type="s" direction="out"/></method>
  </interface>
  <interface name="org.a11y.Status">
    <property name="IsEnabled" type="b" access="readwrite"/>
    <property name="ScreenReaderEnabled" type="b" access="readwrite"/>
  </interface>
</node>
"""


def method_call(connection, sender, path, interface, method, params, invocation):
    invocation.return_value(GLib.Variant("(s)", (ADDRESS,)))


def get_property(connection, sender, path, interface, name):
    return GLib.Variant("b", STATUS[name])


def set_property(connection, sender, path, interface, name, value):
    STATUS[name] = value.unpack()
    connection.emit_signal(
        None, path, "org.freedesktop.DBus.Properties", "PropertiesChanged",
        GLib.Variant("(sa{sv}as)", (interface, {name: value}, [])),
    )
    return True


def acquired(connection, name):
    node = Gio.DBusNodeInfo.new_for_xml(XML)
    for info in node.interfaces:
        connection.register_object("/org/a11y/bus", info, method_call, get_property, set_property)


def lost(connection, name):
    sys.exit("a11y_bus.py: lost org.a11y.Bus")


Gio.bus_own_name(Gio.BusType.SESSION, "org.a11y.Bus", Gio.BusNameOwnerFlags.NONE, acquired, None, lost)
GLib.MainLoop().run()
