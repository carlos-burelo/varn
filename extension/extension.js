// extension.js
"use strict";

const vscode = require("vscode");
const { State } = require("vscode-languageclient/node");
const { resolveLspPath } = require("./src/binary");
const { initStatusBar, setLspStatus, setStatusProvider } = require("./src/status");
const { createClient, stopClient, getClient } = require("./src/client");
const { VarnDebugAdapterFactory, VarnDebugConfigProvider } = require("./src/debug");

// View Providers
const { VarnStatusProvider } = require("./src/views/status_view");
const { VarnActionsProvider } = require("./src/views/actions_view");
const { VarnAstProvider } = require("./src/views/ast_view");

// Command Handlers
const {
    runVarnFile,
    runCheck,
    runBuild,
    runBytecode,
    runBench,
    runWithPhases,
    toggleBuildMode
} = require("./src/commands/execution");

const {
    runPkgInstall,
    runPkgUpdate,
    runPkgAdd,
    runPkgRemove
} = require("./src/commands/packages");

const {
    runRepl,
    runDoctor,
    runCacheClean,
    runInitProject,
    runInstaller
} = require("./src/commands/utilities");

// CodeLens Provider
const { registerVarnCodeLensProvider } = require("./src/providers/codelens");

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

    // ── Native Tree Views ────────────────────────────────────────────────────
    const statusProvider = new VarnStatusProvider(context);
    setStatusProvider(statusProvider);
    const actionsProvider = new VarnActionsProvider(context);
    const astProvider = new VarnAstProvider(context);

    context.subscriptions.push(
        vscode.window.registerTreeDataProvider("varn-status", statusProvider),
        vscode.window.registerTreeDataProvider("varn-actions", actionsProvider),
        vscode.window.registerTreeDataProvider("varn-ast", astProvider)
    );

    // Sync AST Explorer with active editor changes
    vscode.window.onDidChangeActiveTextEditor(() => astProvider.refresh(), null, context.subscriptions);
    vscode.workspace.onDidSaveTextDocument((doc) => {
        if (doc.fileName.endsWith(".vn")) {
            astProvider.refresh();
        }
    }, null, context.subscriptions);

    // Initial AST population
    astProvider.refresh();

    // ── Commands Registration ────────────────────────────────────────────────
    context.subscriptions.push(
        registerVarnCodeLensProvider(),

        // LSP Operations
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

        // File Operations
        vscode.commands.registerCommand("Varn.runFile", (uri) => {
            runVarnFile(context, uri, { terminal: false });
        }),

        vscode.commands.registerCommand("Varn.runFileInTerminal", (uri) => {
            runVarnFile(context, uri, { terminal: true });
        }),

        vscode.commands.registerCommand("Varn.checkFile", (uri) => {
            runCheck(context, uri);
        }),

        vscode.commands.registerCommand("Varn.buildFile", (uri) => {
            runBuild(context, uri);
        }),

        vscode.commands.registerCommand("Varn.showBytecode", (uri) => {
            runBytecode(context, uri);
        }),

        vscode.commands.registerCommand("Varn.showAst", (uri) => {
            // Re-use runWithPhases flow or run direct debug query
            const { runWithDebugPhase } = require("./src/commands/execution");
            runWithDebugPhase(context, uri, ["ast"], "AST");
        }),

        vscode.commands.registerCommand("Varn.showTokens", (uri) => {
            const { runWithDebugPhase } = require("./src/commands/execution");
            runWithDebugPhase(context, uri, ["tokens"], "Tokens");
        }),

        vscode.commands.registerCommand("Varn.benchFile", (uri) => {
            runBench(context, uri);
        }),

        vscode.commands.registerCommand("Varn.runWithPhases", (uri) => {
            runWithPhases(context, uri);
        }),

        // Dependency Operations
        vscode.commands.registerCommand("Varn.pkgInstall", () => {
            runPkgInstall(context);
        }),

        vscode.commands.registerCommand("Varn.pkgUpdate", () => {
            runPkgUpdate(context);
        }),

        vscode.commands.registerCommand("Varn.pkgAdd", () => {
            runPkgAdd(context);
        }),

        vscode.commands.registerCommand("Varn.pkgRemove", () => {
            runPkgRemove(context);
        }),

        // Utilities
        vscode.commands.registerCommand("Varn.runRepl", () => {
            runRepl(context);
        }),

        vscode.commands.registerCommand("Varn.doctor", () => {
            runDoctor(context);
        }),

        vscode.commands.registerCommand("Varn.cacheClean", () => {
            runCacheClean(context);
        }),

        vscode.commands.registerCommand("Varn.initProject", () => {
            runInitProject(context);
        }),

        vscode.commands.registerCommand("Varn.installRuntime", () => {
            runInstaller(context);
        }),

        vscode.commands.registerCommand("Varn.toggleBuildMode", () => {
            toggleBuildMode();
        }),

        vscode.commands.registerCommand("Varn.selectEditorRange", (line) => {
            const editor = vscode.window.activeTextEditor;
            if (!editor) return;
            const range = editor.document.lineAt(line).range;
            editor.selection = new vscode.Selection(range.start, range.start);
            editor.revealRange(range, vscode.TextEditorRevealType.InCenterIfOutsideViewport);
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
