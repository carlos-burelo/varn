use crate::{cli::RunArgs, error::CliError, pipeline};
use std::path::PathBuf;
use varn_pipeline::{CapabilitySet, RunOpts};
use varn_types::capabilities::*;

pub fn execute(args: RunArgs) -> Result<(), CliError> {
    let (file_path, eval) = match (args.file.as_ref(), args.eval.as_ref()) {
        (_, Some(code)) => ("(eval)".to_owned(), Some(code.clone())),
        (Some(file), None) => (file.clone(), None),
        (None, None) => return Err(CliError::usage("Provide a file or inline code with --eval")),
    };

    let capabilities = build_capabilities(&args);

    pipeline::run(&RunOpts {
        file_path,
        eval,
        verbose: args.verbose,
        no_run: false,
        debug: Default::default(),
        trace: args.trace,
        strict: args.strict,
        capabilities,
    })
}

pub fn build_capabilities(args: &RunArgs) -> CapabilitySet {
    if args.sandbox {
        return CapabilitySet::sandbox();
    }

    let has_any_allow_flag = args.allow_read.is_some()
        || args.allow_write.is_some()
        || args.allow_net.is_some()
        || args.allow_env.is_some()
        || args.allow_ffi;

    if args.allow_all || !has_any_allow_flag {
        return CapabilitySet::allow_all();
    }

    let mut mask = 0u64;
    let mut fs_read_paths = None;
    let mut fs_write_paths = None;
    let mut net_hosts = None;
    let mut env_vars = None;

    if let Some(ref read_spec) = args.allow_read {
        mask |= CAP_FS_READ;
        if read_spec != "*" && !read_spec.is_empty() {
            fs_read_paths = Some(
                read_spec
                    .split(',')
                    .map(|s| PathBuf::from(s.trim()))
                    .collect(),
            );
        }
    }

    if let Some(ref write_spec) = args.allow_write {
        mask |= CAP_FS_WRITE;
        if write_spec != "*" && !write_spec.is_empty() {
            fs_write_paths = Some(
                write_spec
                    .split(',')
                    .map(|s| PathBuf::from(s.trim()))
                    .collect(),
            );
        }
    }

    if let Some(ref net_spec) = args.allow_net {
        mask |= CAP_NET_CLIENT | CAP_NET_SERVER;
        if net_spec != "*" && !net_spec.is_empty() {
            net_hosts = Some(net_spec.split(',').map(|s| s.trim().to_string()).collect());
        }
    }

    if let Some(ref env_spec) = args.allow_env {
        mask |= CAP_SYS_ENV;
        if env_spec != "*" && !env_spec.is_empty() {
            env_vars = Some(env_spec.split(',').map(|s| s.trim().to_string()).collect());
        }
    }

    if args.allow_ffi {
        mask |= CAP_SYS_FFI;
    }

    CapabilitySet {
        mask,
        fs_read_paths,
        fs_write_paths,
        net_hosts,
        net_ports: None,
        env_vars,
    }
}
