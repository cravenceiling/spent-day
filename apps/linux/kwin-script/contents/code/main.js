// Calls day.spent.SpentDay.Focus.Report(app_id) on Spent Day whenever focus changes. KWin scripts
// cannot answer D-Bus calls, so Spent Day cannot ask. Only the app id is sent: the window title
// is never read, because a private browser window shows its page title there.

function report(window) {
    callDBus("day.spent.SpentDay.Focus", "/day/spent/SpentDay/Focus", "day.spent.SpentDay.Focus", "Report",
             window ? String(window.resourceClass) : "");
}

workspace.windowActivated.connect(report);
report(workspace.activeWindow);
