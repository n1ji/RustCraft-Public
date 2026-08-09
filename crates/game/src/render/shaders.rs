//! Game-side shader-pack state.
//!
//! OptiFine/Iris programs are prepared as backend-neutral RHI shader sources
//! here. Native compilation and pipeline lifetime stay in the selected RHI
//! backend.

use std::collections::BTreeMap;

use crate::render::shader_pack::{
    load_optifine_shader_pack, OptifineCompileReport, OptifineCompiledStage, OptifineDiagnostic,
    OptifineDiagnosticSeverity, OptifinePass,
};
use renderer_api::rhi::types::ShaderHandle;

#[derive(Clone, Debug)]
pub struct ShaderStageRuntime {
    pub handle: Option<ShaderHandle>,
    pub stage: OptifineCompiledStage,
}

#[derive(Clone, Debug)]
pub struct ShaderProgramRuntime {
    pub name: String,
    pub pass: OptifinePass,
    pub stages: Vec<ShaderStageRuntime>,
    pub diagnostics: Vec<OptifineDiagnostic>,
}

#[derive(Clone, Debug, Default)]
pub struct ShaderRuntime {
    pub active_pack: Option<String>,
    pub option_values: BTreeMap<String, String>,
    pub programs: Vec<ShaderProgramRuntime>,
    pub diagnostics: Vec<String>,
    pub handles: Vec<ShaderHandle>,
    pub generation: u64,
}

impl ShaderRuntime {
    pub fn load(
        &mut self,
        source_name: &str,
        overrides: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        let pack = load_optifine_shader_pack(source_name)?;
        let manifest = pack.manifest();
        if manifest.has_errors() {
            let diagnostics = manifest
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.severity == OptifineDiagnosticSeverity::Error)
                .map(|diagnostic| diagnostic.message.clone())
                .collect::<Vec<_>>();
            if !diagnostics.is_empty() {
                return Err(diagnostics.join("; "));
            }
        }

        let option_values = manifest.effective_option_values(overrides);
        let mut programs = Vec::new();
        let mut diagnostics = Vec::new();
        for program in &manifest.programs {
            if !program.complete || !manifest.program_is_active(program, &option_values)? {
                continue;
            }
            let report = pack.compile_program(&program.name, overrides);
            for diagnostic in &report.diagnostics {
                if diagnostic.severity == OptifineDiagnosticSeverity::Error {
                    diagnostics.push(format!("{}: {}", report.program, diagnostic.message));
                }
            }
            let OptifineCompileReport {
                program: report_name,
                stages,
                diagnostics: report_diagnostics,
            } = report;
            programs.push(ShaderProgramRuntime {
                name: report_name,
                pass: program.pass,
                stages: stages
                    .into_iter()
                    .map(|stage| ShaderStageRuntime {
                        handle: None,
                        stage,
                    })
                    .collect(),
                diagnostics: report_diagnostics,
            });
        }
        if !diagnostics.is_empty() {
            return Err(diagnostics.join("; "));
        }

        self.active_pack = Some(source_name.to_string());
        self.option_values = option_values;
        self.programs = programs;
        self.diagnostics = manifest
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.message.clone())
            .collect();
        self.generation = self.generation.wrapping_add(1);
        Ok(())
    }

    pub fn unload(&mut self) {
        self.active_pack = None;
        self.option_values.clear();
        self.programs.clear();
        self.diagnostics.clear();
        self.handles.clear();
        self.generation = self.generation.wrapping_add(1);
    }
}
