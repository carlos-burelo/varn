"use strict";

const vscode = require("vscode");
const path = require("path");
const fs = require("fs");
const { spawn } = require("child_process");
const { resolveCliPath } = require("./binary");
const { updateBuildModeStatusBar } = require("./status");
const { stripAnsi } = require("./utils");

/** @type {vscode.OutputChannel | undefined} */
let runChannel;

/**
 * @param {vscode.ExtensionContext} context
 * @param {vscode.Uri | undefined} uri
 * @param {{ terminal: boolean, verbose?: boolean, debugPhases?: string[], noRun?: boolean }} opts
 */
function runVarnFile(context, uri, opts) {
    const file = resolveTargetFile(uri);
    if (!file) return;

    const cliPath = resolveCliPath(context);
    if (!cliPath) {
        vscode.window.showErrorMessage(
            "Varn binary not found. " +
            "Build with `cargo build --release --bin vn` or set Varn.cli.path."
        );
        return;
    }

    const cfg = vscode.workspace.getConfiguration("Varn");
    const verbose = opts.verbose ?? cfg.get("run.verbose") ?? false;
    const phases  = opts.debugPhases ?? cfg.get("run.debugPhases") ?? [];
    const noRun   = opts.noRun ?? false;

    const args = buildCliArgs(file, { verbose, debugPhases: phases, noRun });

    if (opts.terminal) {
        const t = vscode.window.createTerminal({
            name: `Varn: ${path.basename(file)}`,
            cwd:  path.dirname(file),
        });
        t.show(true);
        t.sendText(`"${cliPath}" ${args.map(a => `"${a}"`).join(" ")}`);
    } else {
        runInOutputChannel(cliPath, args, path.dirname(file), path.basename(file));
    }
}

/**
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

    const args = buildCliArgs(file, { verbose: false, debugPhases: phases, noRun: true });
    runInOutputChannel(cliPath, args, path.dirname(file), `${label}: ${path.basename(file)}`);
}

/**
 * @param {vscode.ExtensionContext} context
 * @param {vscode.Uri | undefined} uri
 */
function runDisasm(context, uri) {
    const file = resolveTargetFile(uri);
    if (!file) return;

    const cliPath = resolveCliPath(context);
    if (!cliPath) {
        vscode.window.showErrorMessage("Varn binary not found.");
        return;
    }

    runInOutputChannel(cliPath, ["disasm", file], path.dirname(file), `Disasm: ${path.basename(file)}`);
}

/**
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
 * Prompt user for debug phases and run.
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
        runVarnFile(context, uri, { terminal: false, debugPhases: selected, noRun: true });
    }
}

/**
 * @param {vscode.ExtensionContext} context
 */
function runDoctor(context) {
    const cliPath = resolveCliPath(context);
    if (!cliPath) {
        vscode.window.showErrorMessage("Varn binary not found.");
        return;
    }
    const cwd = vscode.workspace.workspaceFolders?.[0]?.uri?.fsPath ?? context.extensionPath;
    runInOutputChannel(cliPath, ["doctor"], cwd, "Varn Doctor");
}

/**
 * @param {vscode.ExtensionContext} context
 */
function runInstaller(context) {
    const root = path.resolve(context.extensionPath, "..");
    const isWin = process.platform === "win32";
    const script = isWin
        ? path.join(root, "scripts", "install.ps1")
        : path.join(root, "scripts", "install.sh");

    if (!fs.existsSync(script)) {
        vscode.window.showErrorMessage(`Varn installer script not found: ${script}`);
        return;
    }

    const t = vscode.window.createTerminal({
        name: "Varn Install Runtime",
        cwd: root,
    });
    t.show(true);
    if (isWin) {
        t.sendText(`powershell -ExecutionPolicy Bypass -File "${script}"`);
    } else {
        t.sendText(`chmod +x "${script}" && "${script}"`);
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

/**
 * Build Varn arg list for a `run` invocation.
 */
function buildCliArgs(file, opts) {
    const args = [file];
    if (opts.verbose) args.push("--verbose");
    if (opts.noRun)   args.push("--no-run");
    if (opts.debugPhases && opts.debugPhases.length > 0) {
        args.push(`--debug=${opts.debugPhases.join(",")}`);
    }
    return args;
}

/**
 * Resolve the active .vn file path from a URI or the active editor.
 */
function resolveTargetFile(uri) {
    const fsPath = uri?.fsPath ?? vscode.window.activeTextEditor?.document.fileName;
    if (!fsPath || !fsPath.endsWith(".vn")) {
        vscode.window.showWarningMessage("No Warp file to run. Open a .vn file first.");
        return undefined;
    }
    return fsPath;
}

/**
 * Run a process and stream its output to the shared Varn Output channel.
 */
function runInOutputChannel(bin, args, cwd, label) {
    if (!runChannel) {
        runChannel = vscode.window.createOutputChannel("Varn Output");
    }
    runChannel.clear();
    runChannel.show(true);
    runChannel.appendLine(`▶  ${label}`);
    runChannel.appendLine(`  binary: ${bin}`);
    runChannel.appendLine("─".repeat(64));

    const proc = spawn(bin, args, { cwd });

    proc.stdout.on("data", (d) => runChannel.append(stripAnsi(d.toString())));
    proc.stderr.on("data", (d) => runChannel.append(stripAnsi(d.toString())));
    proc.on("error", (err) => {
        runChannel.appendLine(`\nError: ${err.message}`);
    });
    proc.on("close", (code) => {
        runChannel.appendLine("─".repeat(64));
        runChannel.appendLine(`Exited with code ${code}`);
    });
}

function registerVarnCodeLensProvider() {
    const selector = { language: "Varn", scheme: "file" };
    return vscode.languages.registerCodeLensProvider(selector, {
        provideCodeLenses(document) {
            const cfg = vscode.workspace.getConfiguration("Varn");
            const enabled = cfg.get("codeLens.enabled", true);
            if (!enabled) return [];

            const firstLine = new vscode.Range(0, 0, 0, 0);
            return [
                new vscode.CodeLens(firstLine, {
                    title: "Run",
                    command: "Varn.runFile",
                    arguments: [document.uri],
                }),
                new vscode.CodeLens(firstLine, {
                    title: "Bench",
                    command: "Varn.benchFile",
                    arguments: [document.uri],
                }),
                new vscode.CodeLens(firstLine, {
                    title: "Disasm",
                    command: "Varn.disasmFile",
                    arguments: [document.uri],
                }),
                new vscode.CodeLens(firstLine, {
                    title: "AST",
                    command: "Varn.showAst",
                    arguments: [document.uri],
                }),
            ];
        },
    });
}

module.exports = {
    runVarnFile,
    runWithDebugPhase,
    runDisasm,
    runBench,
    runWithPhases,
    runDoctor,
    runInstaller,
    toggleBuildMode,
    registerVarnCodeLensProvider
};
