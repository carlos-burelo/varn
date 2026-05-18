"use strict";

const path = require("path");
const fs = require("fs");

/**
 * @param {string} binFile
 * @returns {string | undefined}
 */
function varnHomeBinPath(binFile) {
    const home = process.env.VARN_HOME;
    if (!home) return undefined;
    return path.join(home, "bin", binFile);
}

/**
 * @param {string} executable
 * @returns {string | undefined}
 */
function findOnPath(executable) {
    const pathValue = process.env.PATH;
    if (!pathValue) return undefined;
    const dirs = pathValue.split(path.delimiter);
    for (const dir of dirs) {
        if (!dir) continue;
        const full = path.join(dir, executable);
        if (fs.existsSync(full)) return full;
    }
    return undefined;
}

module.exports = {
    varnHomeBinPath,
    findOnPath
};
