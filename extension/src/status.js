"use strict";

const vscode = require("vscode");

/** @type {vscode.StatusBarItem | undefined} */
let lspStatusBar;
/** @type {vscode.StatusBarItem | undefined} */
let runStatusBar;
/** @type {vscode.StatusBarItem | undefined} */
let buildModeStatusBar;

function initStatusBar(context) {
    lspStatusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 10);
    lspStatusBar.command = "Varn.restartServer";
    setLspStatus("starting");
    lspStatusBar.show();

    runStatusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 9);
    runStatusBar.command = "Varn.runFile";
    runStatusBar.text = "$(play) Run";
    runStatusBar.tooltip = "Run current Varn file";
    runStatusBar.show();

    buildModeStatusBar = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 8);
    buildModeStatusBar.command = "Varn.toggleBuildMode";
    updateBuildModeStatusBar();
    buildModeStatusBar.show();

    context.subscriptions.push(lspStatusBar, runStatusBar, buildModeStatusBar);
}

/**
 * @param {"starting"|"running"|"stopped"|"error"} state
 */
function setLspStatus(state) {
    if (!lspStatusBar) return;
    switch (state) {
        case "starting":
            lspStatusBar.text    = "$(loading~spin) Varn";
            lspStatusBar.tooltip = "Varn Language Server — Starting";
            lspStatusBar.backgroundColor = undefined;
            break;
        case "running":
            lspStatusBar.text    = "$(check) Varn";
            lspStatusBar.tooltip = "Varn Language Server — Running  (click to restart)";
            lspStatusBar.backgroundColor = undefined;
            break;
        case "stopped":
            lspStatusBar.text    = "$(circle-slash) Varn";
            lspStatusBar.tooltip = "Varn Language Server — Stopped  (click to restart)";
            lspStatusBar.backgroundColor = new vscode.ThemeColor("statusBarItem.warningBackground");
            break;
        case "error":
            lspStatusBar.text    = "$(error) Varn";
            lspStatusBar.tooltip = "Varn Language Server — Binary not found";
            lspStatusBar.backgroundColor = new vscode.ThemeColor("statusBarItem.errorBackground");
            break;
    }
}

function updateBuildModeStatusBar() {
    if (!buildModeStatusBar) return;
    const mode = vscode.workspace.getConfiguration("Varn").get("buildMode", "debug");
    if (mode === "release") {
        buildModeStatusBar.text = "$(zap) Release";
        buildModeStatusBar.tooltip = "Build Mode: Release (click to toggle to Debug)";
    } else {
        buildModeStatusBar.text = "$(beaker) Debug";
        buildModeStatusBar.tooltip = "Build Mode: Debug (click to toggle to Release)";
    }
}

module.exports = {
    initStatusBar,
    setLspStatus,
    updateBuildModeStatusBar
};
