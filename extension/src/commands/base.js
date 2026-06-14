"use strict";

const vscode = require("vscode");
const path = require("path");
const { spawn } = require("child_process");
const { resolveCliPath } = require("../binary");
const { stripAnsi } = require("../utils");

/** @type {vscode.OutputChannel | undefined} */
let runChannel;

/**
 * Resolve the active .vn file path from a URI or the active editor.
 * @param {vscode.Uri | undefined} uri
 * @returns {string | undefined}
 */
function resolveTargetFile(uri) {
    let fsPath = uri?.fsPath;
    if (!fsPath) {
        fsPath = vscode.window.activeTextEditor?.document.fileName;
    }
    if (!fsPath || !fsPath.endsWith(".vn")) {
        const vnEditor = vscode.window.visibleTextEditors.find(e => e.document.fileName.endsWith(".vn"));
        if (vnEditor) {
            fsPath = vnEditor.document.fileName;
        }
    }
    if (!fsPath || !fsPath.endsWith(".vn")) {
        vscode.window.showWarningMessage("No Varn file selected. Open a .vn file first.");
        return undefined;
    }
    return fsPath;
}

/**
 * Run a process and stream its output to the shared Varn Output channel.
 * @param {string} bin Path to the binary
 * @param {string[]} args CLI arguments
 * @param {string} cwd Working directory
 * @param {string} label Title to display in output channel
 */
function runInOutputChannel(bin, args, cwd, label) {
    if (!runChannel) {
        runChannel = vscode.window.createOutputChannel("Varn Output");
    }
    runChannel.clear();
    runChannel.show(true);
    runChannel.appendLine(`▶  ${label}`);
    runChannel.appendLine(`  command: ${path.basename(bin)} ${args.join(" ")}`);
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

/**
 * Format a command string suitable for execution in the VS Code terminal.
 * @param {string} cliPath
 * @param {string[]} args
 * @returns {string}
 */
function formatTerminalCommand(cliPath, args) {
    const isWin = process.platform === "win32";
    const escapedArgs = args.map(a => `"${a}"`).join(" ");

    if (!isWin) {
        return `"${cliPath}" ${escapedArgs}`;
    }

    const shell = (vscode.env.shell || "").toLowerCase();
    const isPowerShell = shell.includes("powershell") || shell.includes("pwsh") || shell.includes("powershell_ise") || !shell;

    if (isPowerShell) {
        return `& "${cliPath}" ${escapedArgs}`;
    } else {
        return `"${cliPath}" ${escapedArgs}`;
    }
}

module.exports = {
    resolveTargetFile,
    runInOutputChannel,
    resolveCliPath,
    formatTerminalCommand
};
