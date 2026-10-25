// Reports the focused window's process to the SCDR session agent, which
// relays it to scdrd. Loaded at runtime by `scdrctl agent`; nothing is
// written to kwinrc.

function report(window) {
    if (!window || !(window.pid > 0)) {
        return;
    }
    if (window.desktopWindow || window.dock || window.specialWindow) {
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
