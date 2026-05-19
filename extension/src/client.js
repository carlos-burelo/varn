"use strict";

const vscode = require("vscode");
const { LanguageClient, TransportKind } = require("vscode-languageclient/node");

/** @type {LanguageClient | undefined} */
let client;

/**
 * Create (but do not start) an LSP client.
 */
function createClient(binaryPath, outputChannel, args = []) {
    const serverOptions = {
        command:   binaryPath,
        args:      args,
        transport: TransportKind.stdio,
    };
    
    const clientOptions = {
        documentSelector: [{ scheme: "file", language: "Varn" }],
        synchronize: {
            fileEvents: vscode.workspace.createFileSystemWatcher("**/*.vn"),
        },
        outputChannel: outputChannel,
    };

    client = new LanguageClient("Varn", "Varn Language Server", serverOptions, clientOptions);
    return client;
}

async function stopClient() {
    if (client) {
        await client.stop().catch(() => {});
        await client.dispose().catch(() => {});
        client = undefined;
    }
}

function getClient() {
    return client;
}

module.exports = {
    createClient,
    stopClient,
    getClient
};
