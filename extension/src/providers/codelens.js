"use strict";

const vscode = require("vscode");

function registerVarnCodeLensProvider() {
    const selector = { language: "varn", scheme: "file" };
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
                    title: "Bytecode",
                    command: "Varn.showBytecode",
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
    registerVarnCodeLensProvider
};
