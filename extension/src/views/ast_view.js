"use strict";

const vscode = require("vscode");
const { spawn } = require("child_process");
const { resolveCliPath } = require("../binary");
const { stripAnsi } = require("../utils");
const path = require("path");

class AstNodeItem extends vscode.TreeItem {
    constructor(label, line, children) {
        const hasChildren = children && children.length > 0;
        super(
            label,
            hasChildren
                ? vscode.TreeItemCollapsibleState.Collapsed
                : vscode.TreeItemCollapsibleState.None
        );
        this.children = children || [];
        this.line = line;

        // Set native icons based on the node category
        const cleanLabel = label.toLowerCase();
        if (cleanLabel.includes("functiondecl")) {
            this.iconPath = new vscode.ThemeIcon("symbol-function");
        } else if (cleanLabel.includes("variabledecl") || cleanLabel.startsWith("var ")) {
            this.iconPath = new vscode.ThemeIcon("symbol-variable");
        } else if (cleanLabel.includes("classdecl")) {
            this.iconPath = new vscode.ThemeIcon("symbol-class");
        } else if (cleanLabel.includes("interfacedecl")) {
            this.iconPath = new vscode.ThemeIcon("symbol-interface");
        } else if (cleanLabel.includes("enumdecl")) {
            this.iconPath = new vscode.ThemeIcon("symbol-enum");
        } else if (cleanLabel.includes("structdecl")) {
            this.iconPath = new vscode.ThemeIcon("symbol-struct");
        } else if (cleanLabel.includes("stmt")) {
            this.iconPath = new vscode.ThemeIcon("symbol-method");
        } else if (cleanLabel.includes("literal")) {
            this.iconPath = new vscode.ThemeIcon("symbol-constant");
        } else if (cleanLabel.includes("call")) {
            this.iconPath = new vscode.ThemeIcon("symbol-keyword");
        } else if (cleanLabel.includes("member")) {
            this.iconPath = new vscode.ThemeIcon("symbol-property");
        } else {
            this.iconPath = new vscode.ThemeIcon("symbol-field");
        }

        if (line !== undefined) {
            this.command = {
                title: "Select Editor Range",
                command: "Varn.selectEditorRange",
                arguments: [line]
            };
        }
    }
}

function getAstForFile(context, filePath) {
    return new Promise((resolve, reject) => {
        const cliPath = resolveCliPath(context);
        if (!cliPath) {
            return reject(new Error("Varn CLI not found"));
        }

        const args = ["debug", filePath, "-p", "ast"];
        const proc = spawn(cliPath, args, { cwd: path.dirname(filePath) });
        let stdout = "";
        let stderr = "";

        proc.stdout.on("data", (data) => {
            stdout += data.toString();
        });

        proc.stderr.on("data", (data) => {
            stderr += data.toString();
        });

        proc.on("close", (code) => {
            if (code !== 0) {
                return reject(new Error(stderr || `CLI exited with code ${code}`));
            }
            resolve(stdout);
        });

        proc.on("error", (err) => {
            reject(err);
        });
    });
}

function parseAst(stdout) {
    const lines = stdout.split(/\r?\n/);
    const rootNodes = [];
    const stack = []; // Stack of { node: AstNodeItem, depth: number }

    for (const rawLine of lines) {
        const lineText = stripAnsi(rawLine);
        if (!lineText.trim()) continue;
        if (lineText.includes("abstract syntax tree") || lineText.includes("top-level statements")) {
            continue;
        }

        // Match prefix containing tree markers
        const match = lineText.match(/^([│├└─\s]*)(.*)$/);
        if (!match) continue;

        const prefix = match[1];
        const content = match[2].trim();
        if (!content) continue;

        // Depth (1-indexed) determined by indent level of 4 characters
        const depth = Math.floor(prefix.length / 4);
        if (depth === 0) continue;

        // Parse line coordinate suffix "[L...]"
        const lineMatch = content.match(/^(.*?)\s*\[L(\d+)\]$/);
        let line = undefined;
        let label = content;
        if (lineMatch) {
            label = lineMatch[1].trim();
            line = parseInt(lineMatch[2], 10) - 1; // 0-indexed for VS Code
        }

        const node = new AstNodeItem(label, line);

        while (stack.length > 0 && stack[stack.length - 1].depth >= depth) {
            stack.pop();
        }

        if (stack.length === 0) {
            // Roots default to Expanded to show top level statements immediately
            node.collapsibleState = vscode.TreeItemCollapsibleState.Expanded;
            rootNodes.push(node);
        } else {
            const parent = stack[stack.length - 1].node;
            parent.children.push(node);
            parent.collapsibleState = vscode.TreeItemCollapsibleState.Collapsed;
        }

        stack.push({ node, depth });
    }

    return rootNodes;
}

class VarnAstProvider {
    constructor(context) {
        this.context = context;
        this._onDidChangeTreeData = new vscode.EventEmitter();
        this.onDidChangeTreeData = this._onDidChangeTreeData.event;
        this.rootNodes = [];
        this.errorItem = null;
        this.loading = false;
    }

    refresh() {
        const editor = vscode.window.activeTextEditor;
        if (!editor || !editor.document.fileName.endsWith(".vn")) {
            this.rootNodes = [];
            this.errorItem = null;
            this.loading = false;
            this._onDidChangeTreeData.fire();
            return;
        }

        this.loading = true;
        this.errorItem = null;
        this._onDidChangeTreeData.fire(); // Show loading hint

        const filePath = editor.document.fileName;
        getAstForFile(this.context, filePath)
            .then((stdout) => {
                this.rootNodes = parseAst(stdout);
                this.loading = false;
                this._onDidChangeTreeData.fire();
            })
            .catch((err) => {
                this.rootNodes = [];
                this.errorItem = new vscode.TreeItem(
                    `Error parsing AST: ${err.message || err}`,
                    vscode.TreeItemCollapsibleState.None
                );
                this.errorItem.iconPath = new vscode.ThemeIcon("error");
                this.loading = false;
                this._onDidChangeTreeData.fire();
            });
    }

    getTreeItem(element) {
        return element;
    }

    getChildren(element) {
        if (element) {
            return element.children;
        }
        if (this.errorItem) {
            return [this.errorItem];
        }
        if (this.loading) {
            const item = new vscode.TreeItem("Loading AST...", vscode.TreeItemCollapsibleState.None);
            item.iconPath = new vscode.ThemeIcon("sync~spin");
            return [item];
        }
        if (this.rootNodes.length === 0) {
            const editor = vscode.window.activeTextEditor;
            if (editor && editor.document.fileName.endsWith(".vn")) {
                const item = new vscode.TreeItem("Loading AST...", vscode.TreeItemCollapsibleState.None);
                item.iconPath = new vscode.ThemeIcon("sync~spin");
                return [item];
            } else {
                return [new vscode.TreeItem("No active Varn file open", vscode.TreeItemCollapsibleState.None)];
            }
        }
        return this.rootNodes;
    }
}

module.exports = {
    VarnAstProvider
};
