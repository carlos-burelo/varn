use super::args::Opts;
use super::stats::{get_result_signature, RunResult, RuntimeInfo};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

pub(super) fn discover_runtimes(opts: &Opts, target_vn: &Path) -> Vec<RuntimeInfo> {
    let mut runtimes = Vec::new();

    runtimes.push(RuntimeInfo {
        name: "varn".to_string(),
        bin: target_vn.to_path_buf(),
        args_prefix: vec!["run".to_string()],
        empty_ext: ".vn",
        empty_body: "print(1)",
    });

    if let Some(ref base_path) = opts.baseline {
        runtimes.push(RuntimeInfo {
            name: "varn-base".to_string(),
            bin: base_path.clone(),
            args_prefix: vec!["run".to_string()],
            empty_ext: ".vn",
            empty_body: "print(1)",
        });
    }

    if let Some(bun_path) = find_binary("bun") {
        runtimes.push(RuntimeInfo {
            name: "bun".to_string(),
            bin: bun_path,
            args_prefix: vec!["run".to_string()],
            empty_ext: ".ts",
            empty_body: "console.log(1)",
        });
    }

    if let Some(node_path) = find_binary("node") {
        runtimes.push(RuntimeInfo {
            name: "node".to_string(),
            bin: node_path,
            args_prefix: vec![],
            empty_ext: ".ts",
            empty_body: "console.log(1)",
        });
    }

    if !opts.skip_python {
        if let Some(py_path) = find_binary("python").or_else(|| find_binary("python3")) {
            runtimes.push(RuntimeInfo {
                name: "python".to_string(),
                bin: py_path,
                args_prefix: vec![],
                empty_ext: ".py",
                empty_body: "print(1)",
            });
        }
    }

    runtimes
}

pub(super) fn find_binary(cmd: &str) -> Option<PathBuf> {
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(cmd);
            if candidate.is_file() {
                return Some(candidate);
            }
            #[cfg(windows)]
            {
                let candidate_exe = dir.join(format!("{cmd}.exe"));
                if candidate_exe.is_file() {
                    return Some(candidate_exe);
                }
                let candidate_cmd = dir.join(format!("{cmd}.cmd"));
                if candidate_cmd.is_file() {
                    return Some(candidate_cmd);
                }
            }
        }
    }
    None
}

pub(super) fn invoke_once(
    rt: &RuntimeInfo,
    file: &Path,
    cache_dir: &Path,
) -> Result<RunResult, Box<dyn std::error::Error>> {
    let mut cmd = Command::new(&rt.bin);
    for arg in &rt.args_prefix {
        cmd.arg(arg);
    }
    cmd.arg(file);
    cmd.env("VARN_CACHE_DIR", cache_dir);

    let start = Instant::now();
    let output = cmd.output()?;
    let duration = start.elapsed();
    let ms = duration.as_secs_f64() * 1000.0;

    let mut raw = String::from_utf8_lossy(&output.stdout).to_string();
    if !output.stderr.is_empty() {
        if !raw.is_empty() {
            raw.push('\n');
        }
        raw.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    let trimmed = raw.trim();
    let signature = get_result_signature(trimmed);

    Ok(RunResult {
        ms,
        signature,
        output: trimmed.to_string(),
    })
}

pub(super) fn get_cpu_info() -> String {
    const UNKNOWN_CPU: &str = "unknown";
    #[cfg(windows)]
    {
        let out = Command::new("reg")
            .args([
                "query",
                r"HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0",
                "/v",
                "ProcessorNameString",
            ])
            .output()
            .ok();
        if let Some(o) = out {
            let text = String::from_utf8_lossy(&o.stdout);
            for line in text.lines() {
                if let Some(pos) = line.find("REG_SZ") {
                    let name = line[pos + 6..].trim();
                    if !name.is_empty() {
                        return name.to_string();
                    }
                }
            }
        }
        if let Ok(id) = std::env::var("PROCESSOR_IDENTIFIER") {
            return id;
        }
    }
    UNKNOWN_CPU.to_string()
}
