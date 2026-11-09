// Reports the focused window's process to the SCDR session agent, which
// relays it to scdrd. Loaded at runtime by `scdrctl agent`; nothing is
// written to kwinrc.

// The desktop shell's own windows (KRunner, overview, panel and applet
// popups) must not count as the focused app.
const SHELL_CLASSES = ["plasmashell", "krunner", "org.kde.plasmashell", "org.kde.krunner",
                       "ksmserver", "ksmserver-logout-greeter", "kscreenlocker_greet"];

function isShellWindow(window) {
    const names = [window.resourceClass, window.desktopFileName];
    return names.some(n => n && SHELL_CLASSES.indexOf(String(n).toLowerCase()) >= 0);
}

// Transient or decorative window types; properties missing on older KWin
// versions are simply undefined (falsy).
function isTransientType(window) {
    return !!(window.desktopWindow || window.dock || window.specialWindow ||
              window.notification || window.criticalNotification || window.onScreenDisplay ||
              window.popupMenu || window.dropdownMenu || window.comboBox || window.tooltip ||
              window.appletPopup || window.dndIcon || window.splash);
}

function report(window) {
    if (!window || !(window.pid > 0)) {
        return;
    }
    if (isTransientType(window) || isShellWindow(window)) {
        return;
    }
    // As a string: callDBus picks the D-Bus type of JS numbers itself.
    callDBus("org.scdr.Session1", "/org/scdr/Session1", "org.scdr.Session1",
             "SetForeground", String(window.pid));
}

if (workspace.windowActivated !== undefined) {   // Plasma 6
    workspace.windowActivated.connect(report);
    report(workspace.activeWindow);
} else {                                         // Plasma 5
    workspace.clientActivated.connect(report);
    report(workspace.activeClient);
}
