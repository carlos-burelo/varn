use rustc_hash::FxHashMap;
use std::rc::Rc;
use varn_compiler::FunctionProto;
use varn_core::ModuleId;
use varn_types::{ModuleGraphArtifact, TestEntry};

use crate::{DiagnosticReport, Failure};

pub struct Executable {
    pub entry_proto: FunctionProto,
    pub precompiled: Rc<FxHashMap<ModuleId, Rc<FunctionProto>>>,
    pub graph_artifact: ModuleGraphArtifact,
    pub deps: FxHashMap<String, Vec<String>>,
    pub warnings: Vec<DiagnosticReport>,
    pub tests: Vec<TestEntry>,
}

pub(crate) fn precompiled_modules(
    graph: &ModuleGraphArtifact,
) -> Rc<FxHashMap<ModuleId, Rc<FunctionProto>>> {
    let mut precompiled: FxHashMap<ModuleId, Rc<FunctionProto>> = FxHashMap::default();
    for (path, proto) in &graph.modules {
        if *path != graph.entry_path {
            precompiled.insert(ModuleId::from_canonical_str(path), Rc::new(proto.clone()));
        }
    }
    Rc::new(precompiled)
}

impl Executable {
    pub fn from_graph(graph: ModuleGraphArtifact) -> Result<Executable, Failure> {
        let entry_proto = graph
            .entry_proto()
            .cloned()
            .ok_or_else(|| Failure::internal("entry module not found in graph"))?;

        Ok(Executable {
            entry_proto,
            precompiled: precompiled_modules(&graph),
            tests: graph.tests.clone(),
            graph_artifact: graph,
            deps: FxHashMap::default(),
            warnings: Vec::new(),
        })
    }
}
