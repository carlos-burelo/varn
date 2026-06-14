"use strict";

const vscode = require("vscode");
const path = require("path");
const { resolveTargetFile, runInOutputChannel, resolveCliPath, formatTerminalCommand } = require("./base");
const { updateBuildModeStatusBar } = require("../status");

/**
 * Run a Varn file (via vn run).
 * @param {vscode.ExtensionContext} context
 * @param {vscode.Uri | undefined} uri
 * @param {{ terminal: boolean, verbose?: boolean }} opts
 */
function runVarnFile(context, uri, opts) {
    const file = resolveTargetFile(uri);
    if (!file) return;

    const cliPath = resolveCliPath(context);
    if (!cliPath) {
        vscode.window.showErrorMessage(
            "Varn binary not found. Build with `cargo build --release --bin vn` or set Varn.cli.path."
        );
        return;
    }

    const cfg = vscode.workspace.getConfiguration("Varn");
    const verbose = opts.verbose ?? cfg.get("run.verbose") ?? false;

    const args = ["run", file];
    if (verbose) {
        args.push("--verbose");
    }

    if (opts.terminal) {
        const t = vscode.window.createTerminal({
            name: `Varn Run: ${path.basename(file)}`,
            cwd: path.dirname(file),
        });
        t.show(true);
        t.sendText(formatTerminalCommand(cliPath, args));
    } else {
        runInOutputChannel(cliPath, args, path.dirname(file), `Run: ${path.basename(file)}`);
    }
}

/**
 * Check types in a Varn file (via vn check).
 * @param {vscode.ExtensionContext} context
 * @param {vscode.Uri | undefined} uri
 */
function runCheck(context, uri) {
    const file = resolveTargetFile(uri);
    if (!file) return;

    const cliPath = resolveCliPath(context);
    if (!cliPath) {
        vscode.window.showErrorMessage("Varn binary not found.");
        return;
    }

    runInOutputChannel(cliPath, ["check", file], path.dirname(file), `Check: ${path.basename(file)}`);
}

/**
 * Compile/Build a Varn file (via vn build).
 * @param {vscode.ExtensionContext} context
 * @param {vscode.Uri | undefined} uri
 */
function runBuild(context, uri) {
    const file = resolveTargetFile(uri);
    if (!file) return;

    const cliPath = resolveCliPath(context);
    if (!cliPath) {
        vscode.window.showErrorMessage("Varn binary not found.");
        return;
    }

    runInOutputChannel(cliPath, ["build", file], path.dirname(file), `Build: ${path.basename(file)}`);
}

/**
 * Inspect specific debug phase(s) (via vn debug -p <phases>).
 * @param {vscode.ExtensionContext} context
 * @param {vscode.Uri | undefined} uri
 * @param {string[]} phases
 * @param {string} label
 */
function runWithDebugPhase(context, uri, phases, label) {
    const file = resolveTargetFile(uri);
    if (!file) return;

    const cliPath = resolveCliPath(context);
    if (!cliPath) {
        vscode.window.showErrorMessage("Varn binary not found.");
        return;
    }

    const args = ["debug", file, "-p", phases.join(",")];
    runInOutputChannel(cliPath, args, path.dirname(file), `${label}: ${path.basename(file)}`);
}

/**
 * Show bytecode disassembly of a file (via vn debug -p bytecode).
 * @param {vscode.ExtensionContext} context
 * @param {vscode.Uri | undefined} uri
 */
function runBytecode(context, uri) {
    runWithDebugPhase(context, uri, ["bytecode"], "Bytecode");
}

/**
 * Benchmark a Varn file (via vn bench).
 * @param {vscode.ExtensionContext} context
 * @param {vscode.Uri | undefined} uri
 */
function runBench(context, uri) {
    const file = resolveTargetFile(uri);
    if (!file) return;

    const cliPath = resolveCliPath(context);
    if (!cliPath) {
        vscode.window.showErrorMessage("Varn binary not found.");
        return;
    }

    runInOutputChannel(cliPath, ["bench", file], path.dirname(file), `Bench: ${path.basename(file)}`);
}

/**
 * Prompt user for debug phases and run debug tool.
 * @param {vscode.ExtensionContext} context
 * @param {vscode.Uri | undefined} uri
 */
async function runWithPhases(context, uri) {
    const file = resolveTargetFile(uri);
    if (!file) return;

    const phases = [
        "tokens", "ast", "bytecode", "symbols", "binds", 
        "modules", "types", "expr", "errors", "trace", 
        "calls", "consts", "scope", "graph"
    ];

    const selected = await vscode.window.showQuickPick(phases, {
        canPickMany: true,
        placeHolder: "Select debug phases to enable"
    });

    if (selected && selected.length > 0) {
        runWithDebugPhase(context, uri, selected, "Debug");
    }
}

/**
 * Toggle build mode setting.
 */
async function toggleBuildMode() {
    const config = vscode.workspace.getConfiguration("Varn");
    const current = config.get("buildMode", "debug");
    const next = current === "debug" ? "release" : "debug";
    const target = vscode.workspace.workspaceFolders ? vscode.ConfigurationTarget.Workspace : vscode.ConfigurationTarget.Global;
    await config.update("buildMode", next, target);
    updateBuildModeStatusBar();
}

module.exports = {
    runVarnFile,
    runCheck,
    runBuild,
    runWithDebugPhase,
    runBytecode,
    runBench,
    runWithPhases,
    toggleBuildMode
};
