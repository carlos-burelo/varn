"use strict";

const vscode = require("vscode");
const path = require("path");
const fs = require("fs");
const { runInOutputChannel, resolveCliPath, formatTerminalCommand } = require("./base");

/**
 * Get workspace root directory path.
 * @param {vscode.ExtensionContext} context
 * @returns {string}
 */
function getWorkspaceDir(context) {
    return vscode.workspace.workspaceFolders?.[0]?.uri?.fsPath ?? context.extensionPath;
}

/**
 * Start the interactive REPL in a VS Code terminal (via vn repl).
 * @param {vscode.ExtensionContext} context
 */
function runRepl(context) {
    const cliPath = resolveCliPath(context);
    if (!cliPath) {
        vscode.window.showErrorMessage("Varn binary not found.");
        return;
    }

    const t = vscode.window.createTerminal({
        name: "Varn REPL",
        cwd: getWorkspaceDir(context)
    });
    t.show(true);
    t.sendText(formatTerminalCommand(cliPath, ["repl"]));
}

/**
 * Run diagnostic checks (via vn doctor).
 * @param {vscode.ExtensionContext} context
 */
function runDoctor(context) {
    const cliPath = resolveCliPath(context);
    if (!cliPath) {
        vscode.window.showErrorMessage("Varn binary not found.");
        return;
    }
    const cwd = getWorkspaceDir(context);
    runInOutputChannel(cliPath, ["doctor"], cwd, "Varn Doctor");
}

/**
 * Clean compiler build cache (via vn cache clean).
 * @param {vscode.ExtensionContext} context
 */
function runCacheClean(context) {
    const cliPath = resolveCliPath(context);
    if (!cliPath) {
        vscode.window.showErrorMessage("Varn binary not found.");
        return;
    }
    const cwd = getWorkspaceDir(context);
    runInOutputChannel(cliPath, ["cache", "clean"], cwd, "Cache: Clean Cache");
}

/**
 * Initialize a new Varn project (via vn init [dir] [--name name]).
 * @param {vscode.ExtensionContext} context
 */
async function runInitProject(context) {
    const cliPath = resolveCliPath(context);
    if (!cliPath) {
        vscode.window.showErrorMessage("Varn binary not found.");
        return;
    }

    const dir = await vscode.window.showInputBox({
        prompt: "Directory for the new project (leave empty for current workspace root)",
        placeHolder: "my-project"
    });

    const name = await vscode.window.showInputBox({
        prompt: "Project name (optional, leave empty to use directory name)",
        placeHolder: "my-project"
    });

    const args = ["init"];
    if (dir && dir.trim()) {
        args.push(dir.trim());
    }
    if (name && name.trim()) {
        args.push("--name", name.trim());
    }

    const cwd = getWorkspaceDir(context);
    runInOutputChannel(cliPath, args, cwd, "Init: New Project");
}

/**
 * Run installer script to download the latest Varn runtime.
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

module.exports = {
    runRepl,
    runDoctor,
    runCacheClean,
    runInitProject,
    runInstaller
};
