use varn_op_macros::varn_contract;
use varn_types::NativeCtx;

pub struct PathRuntime;

varn_contract! {
    module: "runtime:path",
    contract: "src/modules/host/path/path_runtime.vn",
    impl PathRuntime {
        fn normalize(_ctx: &mut dyn NativeCtx, path: &str) -> Result<String, String> {
            let p = std::path::Path::new(path);
            let mut comps: Vec<&str> = vec![];
            for comp in p.components() {
                use std::path::Component;
                match comp {
                    Component::ParentDir => {
                        comps.pop();
                    }
                    Component::CurDir => {}
                    Component::Normal(s) => comps.push(s.to_str().unwrap_or("")),
                    Component::RootDir => comps.push(""),
                    Component::Prefix(p) => comps.push(p.as_os_str().to_str().unwrap_or("")),
                }
            }
            Ok(comps.join(std::path::MAIN_SEPARATOR_STR))
        }

        fn dirname(_ctx: &mut dyn NativeCtx, path: &str) -> Result<String, String> {
            Ok(std::path::Path::new(path)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| ".".to_owned()))
        }

        fn basename(_ctx: &mut dyn NativeCtx, path: &str) -> Result<String, String> {
            Ok(std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default())
        }
    }
}
