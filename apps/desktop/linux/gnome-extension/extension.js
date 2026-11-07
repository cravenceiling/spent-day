// Exports the focused window's app id on the session bus as org.gnome.Shell /day/spent/SpentDay/Focus.
// Spent Day polls Get() once a second. Only the owner of day.spent.SpentDay.Focus, which Spent Day
// takes when it starts, gets an answer. Other apps on the session bus, including Flatpak apps
// that may talk to org.gnome.Shell, are turned down.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';

const READER = 'day.spent.SpentDay.Focus';

const IFACE = `
<node>
  <interface name="day.spent.SpentDay.Focus">
    <method name="Get">
      <arg type="s" direction="out" name="app_id"/>
    </method>
  </interface>
</node>`;

export default class SpentDayFocus extends Extension {
    enable() {
        this._object = Gio.DBusExportedObject.wrapJSObject(IFACE, this);
        this._object.export(Gio.DBus.session, '/day/spent/SpentDay/Focus');
    }

    disable() {
        this._object.unexport();
        this._object = null;
    }

    // Asks the bus who owns READER on every call, so only the current owner gets an answer.
    GetAsync(_params, invocation) {
        const sender = invocation.get_sender();
        Gio.DBus.session.call(
            'org.freedesktop.DBus', '/org/freedesktop/DBus', 'org.freedesktop.DBus', 'GetNameOwner',
            new GLib.Variant('(s)', [READER]), new GLib.VariantType('(s)'),
            Gio.DBusCallFlags.NONE, 1000, null,
            (bus, result) => {
                let owner = null;
                try {
                    [owner] = bus.call_finish(result).deepUnpack();
                } catch {
                    // Nobody owns READER.
                }
                if (!sender || owner !== sender) {
                    invocation.return_dbus_error('org.freedesktop.DBus.Error.AccessDenied',
                        `Only the owner of ${READER} may ask`);
                    return;
                }
                invocation.return_value(new GLib.Variant('(s)', [focusedAppId()]));
            });
    }
}

// Only the app id. The window title is never read, because a private browser window shows its
// page title there. For Wayland clients Mutter stores the xdg app_id as the WM class.
function focusedAppId() {
    return global.display.focus_window?.get_wm_class() ?? '';
}
