"use strict";

const vscode = require("vscode");
const path = require("path");
const fs = require("fs");
const { BIN_EXT } = require("./constants");
const { varnHomeBinPath, findOnPath } = require("./utils");

/**
 * Generate a list of candidate paths for a binary.
 * @param {vscode.ExtensionContext} context
 * @param {string} binName
 * @returns {string[]}
 */
function getBinaryCandidates(context, binName) {
    const candidates = [];
    const mode = vscode.workspace.getConfiguration("Varn").get("buildMode", "debug");

    // 1. warp_HOME
    const homeBin = varnHomeBinPath(binName);
    if (homeBin) candidates.push(homeBin);

    // 2. Workspace folders (check target/release and target/debug)
    if (vscode.workspace.workspaceFolders) {
        for (const folder of vscode.workspace.workspaceFolders) {
            const root = folder.uri.fsPath;
            if (mode === "release") {
                candidates.push(path.join(root, "target", "release", binName));
                candidates.push(path.join(root, "target", "debug",   binName));
            } else {
                candidates.push(path.join(root, "target", "debug",   binName));
                candidates.push(path.join(root, "target", "release", binName));
            }
        }
    }

    // 3. Extension root (for development)
    const extRoot = path.resolve(context.extensionPath, "..");
    if (mode === "release") {
        candidates.push(path.join(extRoot, "target", "release", binName));
        candidates.push(path.join(extRoot, "target", "debug",   binName));
    } else {
        candidates.push(path.join(extRoot, "target", "debug",   binName));
        candidates.push(path.join(extRoot, "target", "release", binName));
    }

    return candidates;
}

/**
 * Resolve the Varn binary.
 * @param {vscode.ExtensionContext} context
 * @returns {string | undefined}
 */
function resolveCliPath(context) {
    const override = vscode.workspace.getConfiguration("Varn").get("cli.path");
    if (override && fs.existsSync(override)) return override;

    const binName = `vn${BIN_EXT}`;
    const candidates = getBinaryCandidates(context, binName);
    
    // Add legacy name as fallback
    const legacyName = `vn${BIN_EXT}`;
    candidates.push(...getBinaryCandidates(context, legacyName));

    const found = candidates.find((p) => fs.existsSync(p));
    if (found) return found;

    return findOnPath(binName) ?? findOnPath("vn") ?? findOnPath("vn");
}

/**
 * Resolve the Varn LSP binary.
 * @param {vscode.ExtensionContext} context
 * @returns {string | undefined}
 */
function resolveLspPath(context) {
    const override = vscode.workspace.getConfiguration("Varn").get("server.path");
    if (override && fs.existsSync(override)) return override;

    const binName = `vn-lsp${BIN_EXT}`;
    const candidates = getBinaryCandidates(context, binName);

    const found = candidates.find((p) => fs.existsSync(p));
    if (found) return found;

    return findOnPath(binName) ?? findOnPath("vn-lsp") ?? findOnPath("vn-lsp");
}

module.exports = {
    resolveCliPath,
    resolveLspPath,
};
