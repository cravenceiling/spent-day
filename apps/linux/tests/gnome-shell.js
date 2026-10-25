// Stands in for GNOME Shell for verify.sh. Loads gnome-extension/extension.js, claims
// org.gnome.Shell on the session bus and reports $2 as the focused window's app id.
//
//   gjs -m tests/gnome-shell.js <extension.js> <app id>

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

const [extensionPath, appId] = ARGV;

// The extension imports its base class from GNOME Shell's resources, so serve a stub there.
const dir = GLib.dir_make_tmp('spentday-shell-XXXXXX');
GLib.file_set_contents(`${dir}/extension.js`, 'export class Extension { constructor(metadata) { this.metadata = metadata; } }\n');
GLib.file_set_contents(`${dir}/shell.gresource.xml`, `<?xml version="1.0" encoding="UTF-8"?>
<gresources><gresource prefix="/org/gnome/shell/extensions"><file>extension.js</file></gresource></gresources>
`);
const [ok, , err, status] = GLib.spawn_sync(dir, ['glib-compile-resources', 'shell.gresource.xml'], null,
    GLib.SpawnFlags.SEARCH_PATH, null);
if (!ok || status !== 0)
    throw new Error(`glib-compile-resources failed: ${new TextDecoder().decode(err)}`);
Gio.Resource.load(`${dir}/shell.gresource`)._register();

globalThis.global = {display: {focus_window: {get_wm_class: () => appId}}};
const {default: SpentDayFocus} = await import(`file://${extensionPath}`);
// GNOME Shell keeps every enabled extension. Without a reference here the extension, and the
// D-Bus object it exports, would be garbage collected a few seconds in.
globalThis.spentDayFocus = new SpentDayFocus({uuid: 'spentday@spent.day'});
globalThis.spentDayFocus.enable();

const loop = new GLib.MainLoop(null, false);
Gio.bus_own_name_on_connection(Gio.DBus.session, 'org.gnome.Shell', Gio.BusNameOwnerFlags.NONE,
    () => print('ready'), () => loop.quit());
loop.run();
