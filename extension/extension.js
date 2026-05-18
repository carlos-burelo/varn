// extension.js
"use strict";

const vscode = require("vscode");
const { State } = require("vscode-languageclient/node");
const { resolveLspPath } = require("./src/binary");
const { initStatusBar, setLspStatus } = require("./src/status");
const { createClient, stopClient, getClient } = require("./src/client");
const { VarnDebugAdapterFactory, VarnDebugConfigProvider } = require("./src/debug");
const { 
    runVarnFile, 
    runWithDebugPhase, 
    runDisasm, 
    runBench, 
    runWithPhases, 
    runDoctor, 
    runInstaller, 
    toggleBuildMode, 
    registerVarnCodeLensProvider 
} = require("./src/commands");

/**
 * @param {vscode.ExtensionContext} context
 */
async function activate(context) {
    // ── Status bar ───────────────────────────────────────────────────────────
    initStatusBar(context);

    // ── Language Server ──────────────────────────────────────────────────────
    const lspOutputChannel = vscode.window.createOutputChannel("Varn Language Server");
    const lspPath = resolveLspPath(context);
    
    if (!lspPath) {
        setLspStatus("error");
        vscode.window.showErrorMessage(
            "vn-lsp binary not found. " +
            "Build with `cargo build --release --bin vn-lsp` or set Varn.server.path."
        );
    } else {
        const client = createClient(lspPath, lspOutputChannel);
        client.onDidChangeState(({ newState }) => {
            if      (newState === State.Running)  setLspStatus("running");
            else if (newState === State.Starting) setLspStatus("starting");
            else                                  setLspStatus("stopped");
        });
    }

    // ── Debug adapter ────────────────────────────────────────────────────────
    context.subscriptions.push(
        vscode.debug.registerDebugAdapterDescriptorFactory("Varn", new VarnDebugAdapterFactory(context)),
        vscode.debug.registerDebugConfigurationProvider("Varn", new VarnDebugConfigProvider())
    );

    // ── Commands ─────────────────────────────────────────────────────────────
    context.subscriptions.push(
        registerVarnCodeLensProvider(),

        vscode.commands.registerCommand("Varn.restartServer", async () => {
            await stopClient();
            if (!lspPath) return;
            setLspStatus("starting");
            const client = createClient(lspPath, lspOutputChannel);
            client.onDidChangeState(({ newState }) => {
                if      (newState === State.Running)  setLspStatus("running");
                else if (newState === State.Starting) setLspStatus("starting");
                else                                  setLspStatus("stopped");
            });
            await client.start();
        }),

        vscode.commands.registerCommand("Varn.stopServer", async () => {
            await stopClient();
            setLspStatus("stopped");
        }),

        vscode.commands.registerCommand("Varn.showServerLog", () => {
            getClient()?.outputChannel.show();
        }),

        vscode.commands.registerCommand("Varn.runFile", (uri) => {
            runVarnFile(context, uri, { terminal: false });
        }),

        vscode.commands.registerCommand("Varn.runFileInTerminal", (uri) => {
            runVarnFile(context, uri, { terminal: true });
        }),

        vscode.commands.registerCommand("Varn.showAst", (uri) => {
            runWithDebugPhase(context, uri, ["ast"], "AST");
        }),

        vscode.commands.registerCommand("Varn.showTokens", (uri) => {
            runWithDebugPhase(context, uri, ["tokens"], "Tokens");
        }),

        vscode.commands.registerCommand("Varn.disasmFile", (uri) => {
            runDisasm(context, uri);
        }),

        vscode.commands.registerCommand("Varn.benchFile", (uri) => {
            runBench(context, uri);
        }),

        vscode.commands.registerCommand("Varn.doctor", () => {
            runDoctor(context);
        }),

        vscode.commands.registerCommand("Varn.installRuntime", () => {
            runInstaller(context);
        }),

        vscode.commands.registerCommand("Varn.runWithPhases", (uri) => {
            runWithPhases(context, uri);
        }),

        vscode.commands.registerCommand("Varn.toggleBuildMode", () => {
            toggleBuildMode();
        }),

        { dispose: () => stopClient() }
    );

    // Start LSP
    const client = getClient();
    if (client) {
        await client.start().catch((err) => {
            setLspStatus("stopped");
            vscode.window.showErrorMessage(`Varn Language Server failed to start: ${err?.message ?? err}`);
        });
    }
}

async function deactivate() {
    await stopClient();
}

module.exports = { activate, deactivate };
