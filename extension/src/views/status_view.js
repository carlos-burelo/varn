"use strict";

const vscode = require("vscode");
const { getLspStatus } = require("../status");
const { resolveCliPath, resolveLspPath } = require("../binary");

class StatusTreeItem extends vscode.TreeItem {
    constructor(label, description, iconName, command) {
        super(label, vscode.TreeItemCollapsibleState.None);
        this.description = description;
        if (iconName) {
            this.iconPath = new vscode.ThemeIcon(iconName);
        }
        if (command) {
            this.command = command;
        }
    }
}

class VarnStatusProvider {
    constructor(context) {
        this.context = context;
        this._onDidChangeTreeData = new vscode.EventEmitter();
        this.onDidChangeTreeData = this._onDidChangeTreeData.event;
    }

    refresh() {
        this._onDidChangeTreeData.fire();
    }

    getTreeItem(element) {
        return element;
    }

    getChildren() {
        const items = [];

        // 1. LSP Server Status
        const lspStatus = getLspStatus();
        let lspDesc = "Stopped";
        let lspIcon = "circle-slash";
        if (lspStatus === "running") {
            lspDesc = "Running";
            lspIcon = "check";
        } else if (lspStatus === "starting") {
            lspDesc = "Starting...";
            lspIcon = "sync~spin";
        } else if (lspStatus === "error") {
            lspDesc = "Error (LSP not found)";
            lspIcon = "error";
        }
        items.push(
            new StatusTreeItem(
                "LSP Server",
                lspDesc,
                lspIcon,
                { title: "Restart LSP Server", command: "Varn.restartServer" }
            )
        );

        // 2. Build Mode (Debug/Release)
        const config = vscode.workspace.getConfiguration("Varn");
        const mode = config.get("buildMode", "debug");
        const modeDesc = mode === "release" ? "Release" : "Debug";
        const modeIcon = mode === "release" ? "zap" : "beaker";
        items.push(
            new StatusTreeItem(
                "Build Mode",
                modeDesc,
                modeIcon,
                { title: "Toggle Build Mode", command: "Varn.toggleBuildMode" }
            )
        );

        // 3. CLI Binary Path
        const cliPath = resolveCliPath(this.context);
        const cliDesc = cliPath ? cliPath : "Not found";
        items.push(
            new StatusTreeItem(
                "CLI path",
                cliDesc,
                "terminal"
            )
        );

        // 4. LSP Binary Path
        const lspPath = resolveLspPath(this.context);
        const lspPathDesc = lspPath ? lspPath : "Not found";
        items.push(
            new StatusTreeItem(
                "LSP path",
                lspPathDesc,
                "server"
            )
        );

        return items;
    }
}

module.exports = {
    VarnStatusProvider
};
