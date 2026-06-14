"use strict";

const vscode = require("vscode");
const path = require("path");
const { runInOutputChannel, resolveCliPath } = require("./base");

/**
 * Get workspace root directory path.
 * @param {vscode.ExtensionContext} context
 * @returns {string}
 */
function getWorkspaceDir(context) {
    return vscode.workspace.workspaceFolders?.[0]?.uri?.fsPath ?? context.extensionPath;
}

/**
 * Install project dependencies (via vn pkg install).
 * @param {vscode.ExtensionContext} context
 */
function runPkgInstall(context) {
    const cliPath = resolveCliPath(context);
    if (!cliPath) {
        vscode.window.showErrorMessage("Varn binary not found.");
        return;
    }
    const cwd = getWorkspaceDir(context);
    runInOutputChannel(cliPath, ["pkg", "install"], cwd, "Pkg: Install Dependencies");
}

/**
 * Update project dependencies (via vn pkg update).
 * @param {vscode.ExtensionContext} context
 */
function runPkgUpdate(context) {
    const cliPath = resolveCliPath(context);
    if (!cliPath) {
        vscode.window.showErrorMessage("Varn binary not found.");
        return;
    }
    const cwd = getWorkspaceDir(context);
    runInOutputChannel(cliPath, ["pkg", "update"], cwd, "Pkg: Update Dependencies");
}

/**
 * Add a dependency to the project (via vn pkg add <alias> <origin>).
 * @param {vscode.ExtensionContext} context
 */
async function runPkgAdd(context) {
    const cliPath = resolveCliPath(context);
    if (!cliPath) {
        vscode.window.showErrorMessage("Varn binary not found.");
        return;
    }

    const alias = await vscode.window.showInputBox({
        prompt: "Enter the dependency alias (e.g., math)",
        placeHolder: "alias"
    });
    if (!alias) return;

    const origin = await vscode.window.showInputBox({
        prompt: "Enter the dependency origin (Git URL or local path)",
        placeHolder: "https://github.com/username/repo or /path/to/folder"
    });
    if (!origin) return;

    const cwd = getWorkspaceDir(context);
    runInOutputChannel(cliPath, ["pkg", "add", alias, origin], cwd, `Pkg: Add ${alias}`);
}

/**
 * Remove a dependency from the project (via vn pkg remove <alias>).
 * @param {vscode.ExtensionContext} context
 */
async function runPkgRemove(context) {
    const cliPath = resolveCliPath(context);
    if (!cliPath) {
        vscode.window.showErrorMessage("Varn binary not found.");
        return;
    }

    const alias = await vscode.window.showInputBox({
        prompt: "Enter the dependency alias to remove",
        placeHolder: "alias"
    });
    if (!alias) return;

    const cwd = getWorkspaceDir(context);
    runInOutputChannel(cliPath, ["pkg", "remove", alias], cwd, `Pkg: Remove ${alias}`);
}

module.exports = {
    runPkgInstall,
    runPkgUpdate,
    runPkgAdd,
    runPkgRemove
};
