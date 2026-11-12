.pragma library
.import "logic.js" as Logic
// The Scheduling page's words: questions it asks, labels and one-line summaries. No Qt calls,
// so Node tests it.

// Questions the page asks itself before a D-Bus write (uQ asks for [[command]]s on its own).
var CONFIRMS = {
    "pause": "Pause scdr? Every app's priority goes back to normal until you resume.",
    "rule-remove": "Remove your rule for %s?",
    "profile-remove": "Delete the profile %s?",
    "profile-revert": "Undo your changes to the profile %s?"
};

function roleLabel(role) {
    switch (role) {
    case "foreground": return "The app you're using";
    case "audio": return "Apps playing sound";
    case "game": return "Games";
    case "background": return "Apps in the background when the CPU is busy";
    default: return Logic.clip(role, 60);
    }
}

function confirmText(id, arg) {
    var t = CONFIRMS[id] || "";
    return t.split("%s").join(arg === undefined ? "" : String(arg));
}

function pressureText(state) {
    return state && state.cpuPressure !== null ? "CPU busy: " + Math.round(state.cpuPressure) + " %" : "";
}

function managedText(state) {
    var n = state ? state.managed : 0;
    return n + (n === 1 ? " app managed" : " apps managed");
}

function boostedLine(b) {
    return b.name + " — " + b.profile;
}

function trialText(trial) {
    if (!trial)
        return "";
    return "Trying " + trial.trying + " (was " + trial.was + "). It isn't saved; stop the trial to go back.";
}

// The extra schedulers' state, in a line, or "" when there is nothing to say.
function loaderText(settings) {
    if (!settings || settings.loaderError === "")
        return "";
    if (settings.installed.length === 0)
        return "No extra schedulers are installed.";
    return "Only the schedulers scdr runs itself can be used right now. Install the scheduler tools for the others.";
}

// What `scdrctl schedulers which` printed, as a known installer name, or "".
function installerOf(result) {
    if (!result || result.error || typeof result.output !== "string")
        return "";
    var word = result.output.trim();
    return ["pamac", "packagekit", "terminal"].indexOf(word) >= 0 ? word : "";
}

function installerLabel(name) {
    switch (name) {
    case "pamac": return "Installs with Pamac.";
    case "packagekit": return "Installs with PackageKit.";
    case "terminal": return "Installs with pacman in a terminal window.";
    default: return "No installer found: install scx-scheds and scx-tools with your package manager.";
    }
}

// The line to show after an installer run, or "" when the person cancelled.
function installResult(result) {
    if (result && result.cancelled)
        return "";
    if (result && result.error)
        return Logic.writeError(result);
    return "Done.";
}

function cardText(card) {
    return card.description !== "" ? card.description : "No description.";
}
