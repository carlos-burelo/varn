"use strict";

const vscode = require("vscode");
const path = require("path");
const { spawn } = require("child_process");
const { resolveCliPath } = require("./binary");
const { stripAnsi } = require("./utils");

class VarnDebugAdapterFactory {
    /** @param {vscode.ExtensionContext} context */
    constructor(context) { this._context = context; }

    /** @param {vscode.DebugSession} session */
    createDebugAdapterDescriptor(session) {
        const cliPath = resolveCliPath(this._context);
        if (!cliPath) {
            vscode.window.showErrorMessage(
                "Varn binary not found. " +
                "Build with `cargo build --release --bin vn` or set Varn.cli.path."
            );
            return undefined;
        }
        return new vscode.DebugAdapterInlineImplementation(
            new VarnDebugAdapter(cliPath, session.configuration)
        );
    }
}

class VarnDebugAdapter {
    /**
     * @param {string} cliPath
     * @param {Record<string, unknown>} launchConfig
     */
    constructor(cliPath, launchConfig) {
        this._cliPath = cliPath;
        this._launchConfig = launchConfig;
        this._seq = 1;
        this._proc = null;
        this._emitter = new vscode.EventEmitter();
        this.onDidSendMessage = this._emitter.event;
    }

    handleMessage(msg) {
        switch (msg.command) {
            case "initialize":
                this._respond(msg, {
                    supportsConfigurationDoneRequest: true,
                    supportsTerminateRequest:         true,
                });
                this._event("initialized");
                break;
            case "launch":
                this._respond(msg);
                this._launch(msg.arguments ?? this._launchConfig);
                break;
            case "configurationDone":
                this._respond(msg);
                break;
            case "threads":
                this._respond(msg, { threads: [{ id: 1, name: "main" }] });
                break;
            case "stackTrace":
                this._respond(msg, { stackFrames: [], totalFrames: 0 });
                break;
            case "scopes":
                this._respond(msg, { scopes: [] });
                break;
            case "variables":
                this._respond(msg, { variables: [] });
                break;
            case "setBreakpoints":
                this._respond(msg, {
                    breakpoints: (msg.arguments?.breakpoints ?? []).map(() => ({ verified: false }))
                });
                break;
            case "setFunctionBreakpoints":
                this._respond(msg, { breakpoints: [] });
                break;
            case "setExceptionBreakpoints":
                this._respond(msg, { filters: [] });
                break;
            case "terminate":
            case "disconnect":
                if (this._proc) { this._proc.kill(); this._proc = null; }
                this._respond(msg);
                break;
            default:
                this._emitter.fire({
                    type:        "response",
                    seq:         this._seq++,
                    request_seq: msg.seq,
                    command:     msg.command,
                    success:     false,
                    message:     `unsupported request: ${msg.command}`,
                });
        }
    }

    _launch(cfg) {
        const file = /** @type {string} */ (cfg.program);
        if (!file) {
            this._output("stderr", "Varn: 'program' not set in launch configuration.\n");
            this._event("terminated");
            return;
        }

        const phases  = /** @type {string[]} */ (cfg.debugPhases ?? []);
        const verbose = /** @type {boolean}  */ (cfg.verbose ?? false);
        const noRun   = /** @type {boolean}  */ (cfg.noRun   ?? false);
        const extra   = /** @type {string[]} */ (cfg.args    ?? []);

        const args = [file];
        if (verbose) args.push("--verbose");
        if (noRun)   args.push("--noRun");
        if (phases.length > 0) args.push(`--debug=${phases.join(",")}`);
        if (extra.length  > 0) args.push("--", ...extra);

        const cwd = /** @type {string | undefined} */ (cfg.cwd) ?? path.dirname(file);

        this._event("process", {
            name:           path.basename(file),
            isLocalProcess: true,
            startMethod:    "launch",
        });

        const proc = spawn(this._cliPath, args, { cwd });
        this._proc = proc;

        proc.stdout.on("data", (d) => this._output("stdout", stripAnsi(d.toString())));
        proc.stderr.on("data", (d) => this._output("stderr", stripAnsi(d.toString())));

        proc.on("error", (err) => {
            this._output("stderr", `Error spawning Varn: ${err.message}\n`);
            this._event("exited",     { exitCode: 1 });
            this._event("terminated", {});
            this._proc = null;
        });

        proc.on("close", (code) => {
            this._output("console", `\nProcess exited with code ${code ?? 0}\n`);
            this._event("exited",     { exitCode: code ?? 0 });
            this._event("terminated", {});
            this._proc = null;
        });
    }

    _respond(req, body = {}) {
        this._emitter.fire({
            type:        "response",
            seq:         this._seq++,
            request_seq: req.seq,
            command:     req.command,
            success:     true,
            body,
        });
    }

    _event(event, body = {}) {
        this._emitter.fire({
            type:  "event",
            seq:   this._seq++,
            event,
            body,
        });
    }

    _output(category, text) {
        this._event("output", { category, output: text });
    }

    dispose() {
        if (this._proc) { this._proc.kill(); this._proc = null; }
        this._emitter.dispose();
    }
}

class VarnDebugConfigProvider {
    provideDebugConfigurations() {
        return [
            {
                type:        "Varn",
                request:     "launch",
                name:        "Run Varn File",
                program:     "${file}",
                args:         [],
                cwd:         "${workspaceFolder}",
                verbose:     false,
                debugPhases: [],
            },
        ];
    }

    resolveDebugConfiguration(_folder, config) {
        if (!config.type && !config.request && !config.name) {
            const editor = vscode.window.activeTextEditor;
            if (editor && editor.document.languageId === "Varn") {
                return {
                    type:        "Varn",
                    request:     "launch",
                    name:        "Run Varn File",
                    program:     editor.document.fileName,
                    args:         [],
                    cwd:         path.dirname(editor.document.fileName),
                    verbose:     false,
                    debugPhases: [],
                };
            }
            return null;
        }

        if (!config.program) {
            vscode.window.showErrorMessage(
                "Varn debug: 'program' is required in the launch configuration."
            );
            return null;
        }

        return config;
    }
}

module.exports = {
    VarnDebugAdapterFactory,
    VarnDebugConfigProvider
};
