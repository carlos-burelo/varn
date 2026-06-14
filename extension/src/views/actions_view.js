"use strict";

const vscode = require("vscode");

class ActionCategoryItem extends vscode.TreeItem {
    constructor(label, collapsibleState) {
        super(label, collapsibleState);
        this.contextValue = "category";
    }
}

class ActionTreeItem extends vscode.TreeItem {
    constructor(label, iconName, commandId) {
        super(label, vscode.TreeItemCollapsibleState.None);
        this.iconPath = new vscode.ThemeIcon(iconName);
        this.command = {
            title: label,
            command: commandId
        };
        this.contextValue = "action";
    }
}

class VarnActionsProvider {
    constructor(context) {
        this.context = context;
    }

    getTreeItem(element) {
        return element;
    }

    getChildren(element) {
        if (!element) {
            // Root elements: Categories
            return [
                new ActionCategoryItem("File Operations", vscode.TreeItemCollapsibleState.Expanded),
                new ActionCategoryItem("Dependency Management", vscode.TreeItemCollapsibleState.Collapsed),
                new ActionCategoryItem("Utilities", vscode.TreeItemCollapsibleState.Collapsed)
            ];
        }

        // Child elements based on Category
        if (element instanceof ActionCategoryItem) {
            if (element.label === "File Operations") {
                return [
                    new ActionTreeItem("Run File", "play", "Varn.runFile"),
                    new ActionTreeItem("Run File in Terminal", "terminal", "Varn.runFileInTerminal"),
                    new ActionTreeItem("Check File (Typecheck)", "checklist", "Varn.checkFile"),
                    new ActionTreeItem("Build File (Compile)", "package", "Varn.buildFile"),
                    new ActionTreeItem("Show Bytecode", "file-binary", "Varn.showBytecode"),
                    new ActionTreeItem("Show AST", "symbol-structure", "Varn.showAst"),
                    new ActionTreeItem("Benchmark File", "dashboard", "Varn.benchFile")
                ];
            } else if (element.label === "Dependency Management") {
                return [
                    new ActionTreeItem("Add Dependency", "add", "Varn.pkgAdd"),
                    new ActionTreeItem("Remove Dependency", "trash", "Varn.pkgRemove"),
                    new ActionTreeItem("Install Dependencies", "cloud-download", "Varn.pkgInstall"),
                    new ActionTreeItem("Update Dependencies", "refresh", "Varn.pkgUpdate")
                ];
            } else if (element.label === "Utilities") {
                return [
                    new ActionTreeItem("Start Interactive REPL", "terminal", "Varn.runRepl"),
                    new ActionTreeItem("Doctor (Check Setup)", "pulse", "Varn.doctor"),
                    new ActionTreeItem("Init Project", "new-folder", "Varn.initProject"),
                    new ActionTreeItem("Clean Cache", "clear-all", "Varn.cacheClean"),
                    new ActionTreeItem("Install Runtime", "cloud-download", "Varn.installRuntime")
                ];
            }
        }

        return [];
    }
}

module.exports = {
    VarnActionsProvider
};
