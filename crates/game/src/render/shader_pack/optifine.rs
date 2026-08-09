//! OptiFine/Iris shader-pack discovery and backend-neutral source preparation.
//!
//! This module deliberately stops before render-graph and descriptor creation.
//! It produces an explicit, deterministic manifest and prepares GLSL that is
//! ready for a backend compiler. Legacy OpenGL built-ins are diagnosed instead
//! of being silently translated incorrectly.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};

use renderer_api::rhi::types::{
    ShaderBinding, ShaderBindingType, ShaderInterface, ShaderLanguage, ShaderSource,
    ShaderSourceOrigin, ShaderStage as RhiShaderStage,
};

pub const PROPERTIES_PATH: &str = "shaders/shaders.properties";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OptifinePass {
    Setup,
    Begin,
    Shadow,
    ShadowComposite,
    Prepare,
    GbuffersOpaque,
    Deferred,
    GbuffersTranslucent,
    Composite,
    Final,
}

impl OptifinePass {
    pub fn execution_order(self) -> u8 {
        match self {
            Self::Setup => 0,
            Self::Begin => 1,
            Self::Shadow => 2,
            Self::ShadowComposite => 3,
            Self::Prepare => 4,
            Self::GbuffersOpaque => 5,
            Self::Deferred => 6,
            Self::GbuffersTranslucent => 7,
            Self::Composite => 8,
            Self::Final => 9,
        }
    }

    pub fn is_gbuffers_style(self) -> bool {
        matches!(
            self,
            Self::Shadow | Self::GbuffersOpaque | Self::GbuffersTranslucent
        )
    }

    fn is_compute_only(self) -> bool {
        self == Self::Setup
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OptifineStage {
    Compute,
    Vertex,
    TessellationControl,
    TessellationEvaluation,
    Geometry,
    Fragment,
}

impl OptifineStage {
    fn from_extension(extension: &str) -> Option<Self> {
        Some(match extension {
            "csh" => Self::Compute,
            "vsh" => Self::Vertex,
            "tcs" => Self::TessellationControl,
            "tes" => Self::TessellationEvaluation,
            "gsh" => Self::Geometry,
            "fsh" => Self::Fragment,
            _ => return None,
        })
    }

    pub const fn rhi_stage(self) -> RhiShaderStage {
        match self {
            Self::Compute => RhiShaderStage::Compute,
            Self::Vertex => RhiShaderStage::Vertex,
            Self::TessellationControl => RhiShaderStage::TessellationControl,
            Self::TessellationEvaluation => RhiShaderStage::TessellationEvaluation,
            Self::Geometry => RhiShaderStage::Geometry,
            Self::Fragment => RhiShaderStage::Fragment,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptifineStageSource {
    pub stage: OptifineStage,
    pub path: String,
    /// Compute shaders run suffixless first, followed by `_a` through `_z`.
    pub sequence: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OptifineProgramActivation {
    Enabled,
    Disabled { reason: String },
    Conditional { expression: String },
}

impl OptifineProgramActivation {
    pub fn is_unconditionally_disabled(&self) -> bool {
        matches!(self, Self::Disabled { .. })
    }

    /// Evaluate `program.<name>.enabled` against effective shader-option
    /// values. Undefined identifiers follow GLSL/OptiFine boolean semantics
    /// and evaluate to false.
    pub fn evaluate(&self, values: &BTreeMap<String, String>) -> Result<bool, String> {
        match self {
            Self::Enabled => Ok(true),
            Self::Disabled { .. } => Ok(false),
            Self::Conditional { expression } => evaluate_boolean_expression(expression, values),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum BooleanToken {
    Value(String),
    Not,
    And,
    Or,
    Equal,
    NotEqual,
    LeftParen,
    RightParen,
}

fn tokenize_boolean_expression(expression: &str) -> Result<Vec<BooleanToken>, String> {
    let bytes = expression.as_bytes();
    let mut tokens = Vec::new();
    let mut cursor = 0usize;
    while cursor < bytes.len() {
        if bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
            continue;
        }
        let (token, consumed) = match bytes[cursor..] {
            [b'&', b'&', ..] => (Some(BooleanToken::And), 2),
            [b'|', b'|', ..] => (Some(BooleanToken::Or), 2),
            [b'=', b'=', ..] => (Some(BooleanToken::Equal), 2),
            [b'!', b'=', ..] => (Some(BooleanToken::NotEqual), 2),
            [b'!', ..] => (Some(BooleanToken::Not), 1),
            [b'(', ..] => (Some(BooleanToken::LeftParen), 1),
            [b')', ..] => (Some(BooleanToken::RightParen), 1),
            [b'&' | b'|' | b'=', ..] => {
                return Err(format!(
                    "invalid boolean operator near '{}'",
                    &expression[cursor..]
                ));
            }
            _ => (None, 0),
        };
        if let Some(token) = token {
            tokens.push(token);
            cursor += consumed;
            continue;
        }

        let start = cursor;
        while cursor < bytes.len()
            && !bytes[cursor].is_ascii_whitespace()
            && !matches!(bytes[cursor], b'!' | b'&' | b'|' | b'=' | b'(' | b')')
        {
            cursor += 1;
        }
        if start == cursor {
            return Err(format!("invalid token near '{}'", &expression[cursor..]));
        }
        tokens.push(BooleanToken::Value(expression[start..cursor].to_string()));
    }
    Ok(tokens)
}

struct BooleanExpressionParser<'a> {
    tokens: &'a [BooleanToken],
    values: &'a BTreeMap<String, String>,
    cursor: usize,
}

impl BooleanExpressionParser<'_> {
    fn parse(mut self) -> Result<bool, String> {
        if self.tokens.is_empty() {
            return Err("empty boolean expression".to_string());
        }
        let result = self.parse_or()?;
        if self.cursor != self.tokens.len() {
            return Err(format!("unexpected token {:?}", self.tokens[self.cursor]));
        }
        Ok(result)
    }

    fn parse_or(&mut self) -> Result<bool, String> {
        let mut value = self.parse_and()?;
        while self.consume(&BooleanToken::Or) {
            value |= self.parse_and()?;
        }
        Ok(value)
    }

    fn parse_and(&mut self) -> Result<bool, String> {
        let mut value = self.parse_not()?;
        while self.consume(&BooleanToken::And) {
            value &= self.parse_not()?;
        }
        Ok(value)
    }

    fn parse_not(&mut self) -> Result<bool, String> {
        if self.consume(&BooleanToken::Not) {
            return Ok(!self.parse_not()?);
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Result<bool, String> {
        if self.consume(&BooleanToken::LeftParen) {
            let value = self.parse_or()?;
            if !self.consume(&BooleanToken::RightParen) {
                return Err("missing ')' in boolean expression".to_string());
            }
            return Ok(value);
        }

        let left = self.parse_value(false)?;
        if self.consume(&BooleanToken::Equal) {
            let right = self.parse_value(true)?;
            return Ok(left.eq_ignore_ascii_case(&right));
        }
        if self.consume(&BooleanToken::NotEqual) {
            let right = self.parse_value(true)?;
            return Ok(!left.eq_ignore_ascii_case(&right));
        }
        Ok(boolean_value(&left))
    }

    fn parse_value(&mut self, unknown_is_literal: bool) -> Result<String, String> {
        let Some(BooleanToken::Value(token)) = self.tokens.get(self.cursor) else {
            return Err("expected option name or value".to_string());
        };
        self.cursor += 1;
        if token.eq_ignore_ascii_case("true") || token.eq_ignore_ascii_case("false") {
            return Ok(token.clone());
        }
        if let Some(value) = self.values.get(token) {
            return Ok(value.clone());
        }
        if token.parse::<f64>().is_ok()
            || (token.starts_with('"') && token.ends_with('"'))
            || (token.starts_with('\'') && token.ends_with('\''))
        {
            return Ok(token.trim_matches(['"', '\'']).to_string());
        }
        Ok(if unknown_is_literal {
            token.clone()
        } else {
            "false".to_string()
        })
    }

    fn consume(&mut self, expected: &BooleanToken) -> bool {
        if self.tokens.get(self.cursor) == Some(expected) {
            self.cursor += 1;
            true
        } else {
            false
        }
    }
}

fn boolean_value(value: &str) -> bool {
    if value.eq_ignore_ascii_case("true")
        || value.eq_ignore_ascii_case("on")
        || value.eq_ignore_ascii_case("yes")
    {
        return true;
    }
    if value.eq_ignore_ascii_case("false")
        || value.eq_ignore_ascii_case("off")
        || value.eq_ignore_ascii_case("no")
        || value.is_empty()
    {
        return false;
    }
    value.parse::<f64>().is_ok_and(|number| number != 0.0)
}

fn evaluate_boolean_expression(
    expression: &str,
    values: &BTreeMap<String, String>,
) -> Result<bool, String> {
    let tokens = tokenize_boolean_expression(expression)?;
    BooleanExpressionParser {
        tokens: &tokens,
        values,
        cursor: 0,
    }
    .parse()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptifineProgram {
    /// Name used by `program.<name>.enabled`, including a dimension directory.
    pub name: String,
    pub pass: OptifinePass,
    pub index: u8,
    pub fallback: Option<String>,
    pub activation: OptifineProgramActivation,
    pub stages: Vec<OptifineStageSource>,
    /// Whether the discovered stages form a program that can be compiled.
    pub complete: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptifineOptionKind {
    Boolean,
    Value,
    Constant,
    /// A screen referenced an option for which no source definition was found.
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptifineOptionSource {
    pub path: String,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptifineOption {
    pub name: String,
    pub kind: OptifineOptionKind,
    pub default_value: String,
    pub values: Vec<String>,
    pub description: String,
    pub slider: bool,
    pub screens: Vec<String>,
    pub sources: Vec<OptifineOptionSource>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OptifineScreenEntry {
    Option(String),
    Screen(String),
    Profile,
    Empty,
    Remaining,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptifineScreen {
    /// Empty for the root screen.
    pub name: String,
    pub columns: Option<u8>,
    pub entries: Vec<OptifineScreenEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OptifineProfileAction {
    SetOption { name: String, value: String },
    IncludeProfile(String),
    DisableProgram(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptifineProfile {
    pub name: String,
    pub actions: Vec<OptifineProfileAction>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptifineDiagnosticSeverity {
    Warning,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OptifineDiagnosticCode {
    InvalidPropertiesEncoding,
    DuplicateProperty,
    UnknownProgramFile,
    DuplicateStage,
    IncompleteStagePair,
    InvalidStageForPass,
    ConflictingOption,
    UndefinedScreenOption,
    InvalidOptionOverride,
    LegacyOpenGlSemantics,
    IncludeError,
    CompileError,
    ProgramDisabled,
    ProgramNotFound,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptifineDiagnostic {
    pub severity: OptifineDiagnosticSeverity,
    pub code: OptifineDiagnosticCode,
    pub message: String,
    pub path: Option<String>,
    pub line: Option<usize>,
}

impl OptifineDiagnostic {
    fn warning(code: OptifineDiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            severity: OptifineDiagnosticSeverity::Warning,
            code,
            message: message.into(),
            path: None,
            line: None,
        }
    }

    fn error(code: OptifineDiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            severity: OptifineDiagnosticSeverity::Error,
            code,
            message: message.into(),
            path: None,
            line: None,
        }
    }

    fn at(mut self, path: impl Into<String>, line: Option<usize>) -> Self {
        self.path = Some(path.into());
        self.line = line;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptifineManifest {
    pub programs: Vec<OptifineProgram>,
    pub options: Vec<OptifineOption>,
    pub screens: Vec<OptifineScreen>,
    pub profiles: Vec<OptifineProfile>,
    pub required_iris_features: Vec<String>,
    pub optional_iris_features: Vec<String>,
    pub properties: BTreeMap<String, String>,
    pub diagnostics: Vec<OptifineDiagnostic>,
}

impl OptifineManifest {
    pub fn program(&self, name: &str) -> Option<&OptifineProgram> {
        self.programs.iter().find(|program| program.name == name)
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == OptifineDiagnosticSeverity::Error)
    }

    /// Whether `value` is an acceptable override for the named option.
    /// Mirrors [`validate_option_overrides`], for callers that need to
    /// sanitise persisted overrides against a (possibly updated) pack.
    pub fn is_valid_override(&self, name: &str, value: &str) -> bool {
        let Some(option) = self.options.iter().find(|option| option.name == name) else {
            return false;
        };
        match option.kind {
            OptifineOptionKind::Boolean => {
                value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false")
            }
            OptifineOptionKind::Value | OptifineOptionKind::Constant => {
                option.values.is_empty() || option.values.iter().any(|allowed| allowed == value)
            }
            OptifineOptionKind::Unknown => false,
        }
    }

    pub fn effective_option_values(
        &self,
        overrides: &BTreeMap<String, String>,
    ) -> BTreeMap<String, String> {
        self.options
            .iter()
            .map(|option| {
                let value = overrides
                    .get(&option.name)
                    .filter(|value| self.is_valid_override(&option.name, value))
                    .cloned()
                    .unwrap_or_else(|| option.default_value.clone());
                (option.name.clone(), value)
            })
            .collect()
    }

    pub fn program_is_active(
        &self,
        program: &OptifineProgram,
        values: &BTreeMap<String, String>,
    ) -> Result<bool, String> {
        program.activation.evaluate(values).map_err(|error| {
            format!(
                "invalid program.{}.enabled expression: {error}",
                program.name
            )
        })
    }
}

#[derive(Clone, Debug)]
pub struct OptifineShaderPack {
    manifest: OptifineManifest,
    files: HashMap<String, Vec<u8>>,
}

impl OptifineShaderPack {
    pub(super) fn from_files(files: HashMap<String, Vec<u8>>) -> Option<Self> {
        let manifest = manifest_from_files(&files)?;
        Some(Self { manifest, files })
    }

    pub fn manifest(&self) -> &OptifineManifest {
        &self.manifest
    }

    pub fn compile_program(
        &self,
        name: &str,
        option_overrides: &BTreeMap<String, String>,
    ) -> OptifineCompileReport {
        compile_program(&self.files, &self.manifest, name, option_overrides)
    }

    /// Localized labels from `shaders/lang/<code>.lang` (`option.NAME=…`,
    /// `screen.NAME=…`, `value.NAME.<v>=…`). Falls back to `en_US` entries
    /// for keys the requested language does not define.
    pub fn lang_entries(&self, language: &str) -> BTreeMap<String, String> {
        let mut entries = BTreeMap::new();
        for code in ["en_US", language] {
            let path = format!("shaders/lang/{code}.lang");
            let Some(bytes) = self.files.get(&path) else {
                continue;
            };
            let Ok(source) = std::str::from_utf8(bytes) else {
                continue;
            };
            for line in source.lines() {
                let line = line.trim_start_matches('\u{feff}').trim();
                if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    entries.insert(key.trim().to_string(), value.trim().to_string());
                }
            }
        }
        entries
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptifineCompiledStage {
    pub stage: OptifineStage,
    pub source_path: String,
    pub sequence: u8,
    /// Patched GLSL stays backend-neutral until the active RHI creates the
    /// native shader module.
    pub source: ShaderSource,
    /// Colour targets written by this stage, parsed from the OptiFine
    /// `/* DRAWBUFFERS:0364 */` or Iris `/* RENDERTARGETS: 0,5,19 */`
    /// comment. When branches declare different sets, the longest list is
    /// reported so the executor can allocate enough attachments.
    /// Empty for non-fragment stages or when no directive is present
    /// (OptiFine semantics: defaults to `{0}`).
    pub draw_buffers: Vec<u8>,
    /// Field declarations of the `FrameUniforms` UBO the patcher generated
    /// at `set = 0, binding = 0`, in declaration order.
    pub ubo_fields: Vec<String>,
    /// Sampler/image name → `set = 0` binding assigned by the patcher.
    pub sampler_bindings: Vec<(String, u32, ShaderBindingType)>,
}

impl OptifineCompiledStage {
    /// Convert the pack patcher's interface metadata into the renderer RHI
    /// contract. This keeps OptiFine/Iris naming and pass parsing in Game,
    /// while the selected backend remains responsible for native layouts.
    pub fn rhi_interface(&self) -> ShaderInterface {
        let stages = self.stage.rhi_stage().mask();
        let mut interface = ShaderInterface::default();
        if !self.ubo_fields.is_empty() {
            interface = interface.with_binding(ShaderBinding::new(
                "FrameUniforms",
                0,
                0,
                ShaderBindingType::UniformBuffer,
                stages,
            ));
        }
        for (name, binding, ty) in &self.sampler_bindings {
            interface = interface.with_binding(ShaderBinding::new(name, 0, *binding, *ty, stages));
        }
        interface
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptifineCompileReport {
    pub program: String,
    pub stages: Vec<OptifineCompiledStage>,
    pub diagnostics: Vec<OptifineDiagnostic>,
}

impl OptifineCompileReport {
    pub fn succeeded(&self) -> bool {
        !self.stages.is_empty()
            && !self
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == OptifineDiagnosticSeverity::Error)
    }
}

pub(super) fn manifest_from_files(files: &HashMap<String, Vec<u8>>) -> Option<OptifineManifest> {
    let has_properties = files.contains_key(PROPERTIES_PATH);
    let has_program = files
        .keys()
        .any(|path| program_stage_candidate(path).is_some());
    if !has_properties && !has_program {
        return None;
    }

    let mut diagnostics = Vec::new();
    let properties = parse_properties(files.get(PROPERTIES_PATH), &mut diagnostics);
    let screens = parse_screens(&properties);
    let profiles = parse_profiles(&properties);
    let required_iris_features = property_words(&properties, "iris.features.required");
    let optional_iris_features = property_words(&properties, "iris.features.optional");
    let mut programs = discover_programs(files, &properties, &mut diagnostics);
    programs.sort_by_key(program_sort_key);
    let options = discover_options(files, &properties, &screens, &mut diagnostics);

    Some(OptifineManifest {
        programs,
        options,
        screens,
        profiles,
        required_iris_features,
        optional_iris_features,
        properties,
        diagnostics,
    })
}

fn parse_properties(
    bytes: Option<&Vec<u8>>,
    diagnostics: &mut Vec<OptifineDiagnostic>,
) -> BTreeMap<String, String> {
    let Some(bytes) = bytes else {
        return BTreeMap::new();
    };
    let Ok(source) = std::str::from_utf8(bytes) else {
        diagnostics.push(
            OptifineDiagnostic::error(
                OptifineDiagnosticCode::InvalidPropertiesEncoding,
                "shaders.properties is not valid UTF-8",
            )
            .at(PROPERTIES_PATH, None),
        );
        return BTreeMap::new();
    };

    let mut properties = BTreeMap::new();
    let mut logical_line = String::new();
    let mut logical_start = 1usize;
    for (index, physical_line) in source.lines().enumerate() {
        let physical_line_number = index + 1;
        if logical_line.is_empty() {
            logical_start = physical_line_number;
        }
        let trimmed_end = physical_line.trim_end();
        let continued = has_unescaped_trailing_backslash(trimmed_end);
        let segment = if continued {
            &trimmed_end[..trimmed_end.len() - 1]
        } else {
            physical_line
        };
        logical_line.push_str(segment);
        if continued {
            continue;
        }

        if let Some((key, value)) = split_property(&logical_line) {
            if properties.insert(key.clone(), value).is_some() {
                diagnostics.push(
                    OptifineDiagnostic::warning(
                        OptifineDiagnosticCode::DuplicateProperty,
                        format!("property '{key}' is defined more than once; the last value wins"),
                    )
                    .at(PROPERTIES_PATH, Some(logical_start)),
                );
            }
        }
        logical_line.clear();
    }
    if !logical_line.is_empty() {
        if let Some((key, value)) = split_property(&logical_line) {
            properties.insert(key, value);
        }
    }
    properties
}

fn has_unescaped_trailing_backslash(line: &str) -> bool {
    line.as_bytes()
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        == 1
}

fn split_property(line: &str) -> Option<(String, String)> {
    let line = line.trim_start();
    if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
        return None;
    }
    let mut separator = None;
    let mut escaped = false;
    for (index, character) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
        } else if character == '=' || character == ':' || character.is_whitespace() {
            separator = Some(index);
            break;
        }
    }
    let (raw_key, raw_value) = match separator {
        Some(index) => {
            let mut value = line[index..].trim_start();
            if value.starts_with('=') || value.starts_with(':') {
                value = value[1..].trim_start();
            }
            (&line[..index], value)
        }
        None => (line, ""),
    };
    let key = unescape_property(raw_key.trim());
    (!key.is_empty()).then(|| (key, unescape_property(raw_value.trim())))
}

fn unescape_property(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        match characters.next() {
            Some('n') => output.push('\n'),
            Some('r') => output.push('\r'),
            Some('t') => output.push('\t'),
            Some(character) => output.push(character),
            None => output.push('\\'),
        }
    }
    output
}

fn property_words(properties: &BTreeMap<String, String>, key: &str) -> Vec<String> {
    properties
        .get(key)
        .map(|value| value.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default()
}

fn parse_screens(properties: &BTreeMap<String, String>) -> Vec<OptifineScreen> {
    let mut screens = Vec::new();
    for (key, value) in properties {
        let name = if key == "screen" {
            ""
        } else if let Some(name) = key.strip_prefix("screen.") {
            if name.ends_with(".columns") || name == "columns" {
                continue;
            }
            name
        } else {
            continue;
        };
        let columns_key = if name.is_empty() {
            "screen.columns".to_string()
        } else {
            format!("screen.{name}.columns")
        };
        let columns = properties
            .get(&columns_key)
            .and_then(|value| value.parse::<u8>().ok());
        let entries = value
            .split_whitespace()
            .map(|entry| {
                if entry == "<profile>" {
                    OptifineScreenEntry::Profile
                } else if entry == "<empty>" {
                    OptifineScreenEntry::Empty
                } else if entry == "*" {
                    OptifineScreenEntry::Remaining
                } else if entry.starts_with('[') && entry.ends_with(']') && entry.len() > 2 {
                    OptifineScreenEntry::Screen(entry[1..entry.len() - 1].to_string())
                } else {
                    OptifineScreenEntry::Option(entry.to_string())
                }
            })
            .collect();
        screens.push(OptifineScreen {
            name: name.to_string(),
            columns,
            entries,
        });
    }
    screens.sort_by(|left, right| left.name.cmp(&right.name));
    screens
}

fn parse_profiles(properties: &BTreeMap<String, String>) -> Vec<OptifineProfile> {
    let mut profiles = Vec::new();
    for (key, value) in properties {
        let Some(name) = key.strip_prefix("profile.") else {
            continue;
        };
        let actions = value
            .split_whitespace()
            .filter_map(|token| {
                if let Some(program) = token.strip_prefix("!program.") {
                    Some(OptifineProfileAction::DisableProgram(program.to_string()))
                } else if let Some(profile) = token.strip_prefix("profile.") {
                    Some(OptifineProfileAction::IncludeProfile(profile.to_string()))
                } else if let Some(option) = token.strip_prefix('!') {
                    Some(OptifineProfileAction::SetOption {
                        name: option.to_string(),
                        value: "false".to_string(),
                    })
                } else if let Some((option, value)) =
                    token.split_once('=').or_else(|| token.split_once(':'))
                {
                    Some(OptifineProfileAction::SetOption {
                        name: option.to_string(),
                        value: value.to_string(),
                    })
                } else if !token.is_empty() {
                    Some(OptifineProfileAction::SetOption {
                        name: token.to_string(),
                        value: "true".to_string(),
                    })
                } else {
                    None
                }
            })
            .collect();
        profiles.push(OptifineProfile {
            name: name.to_string(),
            actions,
        });
    }
    profiles.sort_by(|left, right| left.name.cmp(&right.name));
    profiles
}

#[derive(Clone, Debug)]
struct ProgramStageCandidate {
    name: String,
    pass: OptifinePass,
    index: u8,
    stage: OptifineStage,
    sequence: u8,
}

fn program_stage_candidate(path: &str) -> Option<ProgramStageCandidate> {
    let relative = path.strip_prefix("shaders/")?;
    let extension = Path::new(relative).extension()?.to_str()?;
    let stage = OptifineStage::from_extension(extension)?;
    let without_extension = relative.strip_suffix(&format!(".{extension}"))?;
    let (program_path, sequence) = if stage == OptifineStage::Compute {
        match without_extension.rsplit_once('_') {
            Some((base, suffix))
                if suffix.len() == 1
                    && suffix.as_bytes()[0].is_ascii_lowercase()
                    && classify_program_leaf(Path::new(base).file_name()?.to_str()?).is_some() =>
            {
                (base, suffix.as_bytes()[0] - b'a' + 1)
            }
            _ => (without_extension, 0),
        }
    } else {
        (without_extension, 0)
    };
    let leaf = Path::new(program_path).file_name()?.to_str()?;
    let (pass, index) = classify_program_leaf(leaf)?;
    Some(ProgramStageCandidate {
        name: program_path.replace('\\', "/"),
        pass,
        index,
        stage,
        sequence,
    })
}

fn classify_program_leaf(name: &str) -> Option<(OptifinePass, u8)> {
    if name == "final" {
        return Some((OptifinePass::Final, 0));
    }
    if matches!(name, "shadow" | "shadow_solid" | "shadow_cutout") {
        return Some((OptifinePass::Shadow, 0));
    }
    if name.starts_with("gbuffers_") && is_identifier(name) {
        let pass = if is_translucent_gbuffers(name) {
            OptifinePass::GbuffersTranslucent
        } else {
            OptifinePass::GbuffersOpaque
        };
        return Some((pass, 0));
    }
    for (prefix, pass) in [
        ("setup", OptifinePass::Setup),
        ("begin", OptifinePass::Begin),
        ("shadowcomp", OptifinePass::ShadowComposite),
        ("prepare", OptifinePass::Prepare),
        ("deferred", OptifinePass::Deferred),
        ("composite", OptifinePass::Composite),
    ] {
        if let Some(index) = indexed_program(name, prefix) {
            return Some((pass, index));
        }
    }
    None
}

fn indexed_program(name: &str, prefix: &str) -> Option<u8> {
    let suffix = name.strip_prefix(prefix)?;
    if suffix.is_empty() {
        return Some(0);
    }
    if suffix.starts_with('0') {
        return None;
    }
    suffix
        .parse::<u8>()
        .ok()
        .filter(|index| (1..=99).contains(index))
}

fn is_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn is_translucent_gbuffers(name: &str) -> bool {
    matches!(
        name,
        "gbuffers_water"
            | "gbuffers_hand_water"
            | "gbuffers_particles_translucent"
            | "gbuffers_block_translucent"
            | "gbuffers_entities_translucent"
    )
}

fn discover_programs(
    files: &HashMap<String, Vec<u8>>,
    properties: &BTreeMap<String, String>,
    diagnostics: &mut Vec<OptifineDiagnostic>,
) -> Vec<OptifineProgram> {
    let mut grouped = BTreeMap::<String, (OptifinePass, u8, Vec<OptifineStageSource>)>::new();
    let mut paths: Vec<_> = files.keys().collect();
    paths.sort();
    for path in paths {
        let extension = Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str());
        let Some(extension) = extension else {
            continue;
        };
        if OptifineStage::from_extension(extension).is_none() || !path.starts_with("shaders/") {
            continue;
        }
        let Some(candidate) = program_stage_candidate(path) else {
            diagnostics.push(
                OptifineDiagnostic::warning(
                    OptifineDiagnosticCode::UnknownProgramFile,
                    "shader stage filename does not name a recognized OptiFine/Iris program",
                )
                .at(path, None),
            );
            continue;
        };
        let entry = grouped.entry(candidate.name.clone()).or_insert_with(|| {
            (
                candidate.pass,
                candidate.index,
                Vec::<OptifineStageSource>::new(),
            )
        });
        if entry
            .2
            .iter()
            .any(|source| source.stage == candidate.stage && source.sequence == candidate.sequence)
        {
            diagnostics.push(
                OptifineDiagnostic::error(
                    OptifineDiagnosticCode::DuplicateStage,
                    format!(
                        "program '{}' contains duplicate {:?} stage sequence {}",
                        candidate.name, candidate.stage, candidate.sequence
                    ),
                )
                .at(path, None),
            );
            continue;
        }
        entry.2.push(OptifineStageSource {
            stage: candidate.stage,
            path: path.clone(),
            sequence: candidate.sequence,
        });
    }

    grouped
        .into_iter()
        .map(|(name, (pass, index, mut stages))| {
            stages.sort_by_key(|stage| (stage.stage, stage.sequence));
            let activation = program_activation(&name, pass, properties);
            let complete = validate_stage_set(&name, pass, &stages, diagnostics);
            OptifineProgram {
                fallback: program_fallback(&name),
                name,
                pass,
                index,
                activation,
                stages,
                complete,
            }
        })
        .collect()
}

fn program_activation(
    name: &str,
    pass: OptifinePass,
    properties: &BTreeMap<String, String>,
) -> OptifineProgramActivation {
    if pass == OptifinePass::Shadow
        && properties
            .get("shadow.enabled")
            .is_some_and(|value| value.eq_ignore_ascii_case("false"))
    {
        return OptifineProgramActivation::Disabled {
            reason: "shadow.enabled=false".to_string(),
        };
    }
    let Some(expression) = properties.get(&format!("program.{name}.enabled")) else {
        return OptifineProgramActivation::Enabled;
    };
    if expression.eq_ignore_ascii_case("true") {
        OptifineProgramActivation::Enabled
    } else if expression.eq_ignore_ascii_case("false") {
        OptifineProgramActivation::Disabled {
            reason: format!("program.{name}.enabled=false"),
        }
    } else {
        OptifineProgramActivation::Conditional {
            expression: expression.clone(),
        }
    }
}

fn validate_stage_set(
    name: &str,
    pass: OptifinePass,
    stages: &[OptifineStageSource],
    diagnostics: &mut Vec<OptifineDiagnostic>,
) -> bool {
    let has = |stage| stages.iter().any(|source| source.stage == stage);
    let has_compute = has(OptifineStage::Compute);
    let has_vertex = has(OptifineStage::Vertex);
    let has_fragment = has(OptifineStage::Fragment);
    let has_tess_control = has(OptifineStage::TessellationControl);
    let has_tess_evaluation = has(OptifineStage::TessellationEvaluation);
    let mut complete = true;

    if pass.is_compute_only() {
        if !has_compute {
            complete = false;
            diagnostics.push(OptifineDiagnostic::error(
                OptifineDiagnosticCode::IncompleteStagePair,
                format!("setup program '{name}' requires a compute stage"),
            ));
        }
        if stages
            .iter()
            .any(|source| source.stage != OptifineStage::Compute)
        {
            complete = false;
            diagnostics.push(OptifineDiagnostic::error(
                OptifineDiagnosticCode::InvalidStageForPass,
                format!("setup program '{name}' only supports compute stages"),
            ));
        }
        return complete;
    }

    if pass.is_gbuffers_style() && has_compute {
        complete = false;
        diagnostics.push(OptifineDiagnostic::error(
            OptifineDiagnosticCode::InvalidStageForPass,
            format!("gbuffers-style program '{name}' does not support compute stages"),
        ));
    }
    if pass.is_gbuffers_style() || !has_compute {
        if !has_vertex || !has_fragment {
            complete = false;
            diagnostics.push(OptifineDiagnostic::error(
                OptifineDiagnosticCode::IncompleteStagePair,
                format!("program '{name}' requires both vertex and fragment stages"),
            ));
        }
    } else if has_vertex != has_fragment {
        complete = false;
        diagnostics.push(OptifineDiagnostic::error(
            OptifineDiagnosticCode::IncompleteStagePair,
            format!("compute-backed program '{name}' must provide both graphics stages or neither"),
        ));
    }
    if has_tess_control != has_tess_evaluation {
        complete = false;
        diagnostics.push(OptifineDiagnostic::error(
            OptifineDiagnosticCode::IncompleteStagePair,
            format!(
                "program '{name}' must provide both tessellation control and evaluation stages"
            ),
        ));
    }
    if (has_tess_control || has_tess_evaluation) && !pass.is_gbuffers_style() {
        complete = false;
        diagnostics.push(OptifineDiagnostic::error(
            OptifineDiagnosticCode::InvalidStageForPass,
            format!("composite-style program '{name}' does not support tessellation stages"),
        ));
    }
    complete
}

fn program_fallback(name: &str) -> Option<String> {
    let path = Path::new(name);
    let leaf = path.file_name()?.to_str()?;
    let fallback_leaf = match leaf {
        "shadow_solid" | "shadow_cutout" => "shadow",
        "gbuffers_line" => "gbuffers_basic",
        "gbuffers_textured" | "gbuffers_skybasic" => "gbuffers_basic",
        "gbuffers_textured_lit" => "gbuffers_textured",
        "gbuffers_skytextured"
        | "gbuffers_clouds"
        | "gbuffers_beaconbeam"
        | "gbuffers_armor_glint"
        | "gbuffers_spidereyes" => "gbuffers_textured",
        "gbuffers_terrain" | "gbuffers_entities" | "gbuffers_hand" | "gbuffers_weather" => {
            "gbuffers_textured_lit"
        }
        "gbuffers_damagedblock" | "gbuffers_block" | "gbuffers_water" => "gbuffers_terrain",
        "gbuffers_hand_water" => "gbuffers_hand",
        "gbuffers_entities_glowing" | "gbuffers_entities_translucent" => "gbuffers_entities",
        "gbuffers_particles" => "gbuffers_textured_lit",
        "gbuffers_particles_translucent" => "gbuffers_particles",
        "gbuffers_block_translucent" => "gbuffers_block",
        _ => return None,
    };
    Some(match path.parent().and_then(Path::to_str) {
        Some(parent) if !parent.is_empty() => {
            format!("{}/{}", parent.replace('\\', "/"), fallback_leaf)
        }
        _ => fallback_leaf.to_string(),
    })
}

fn program_sort_key(program: &OptifineProgram) -> (String, u8, u8, String) {
    let dimension = Path::new(&program.name)
        .parent()
        .and_then(Path::to_str)
        .unwrap_or("")
        .replace('\\', "/");
    (
        dimension,
        program.pass.execution_order(),
        program.index,
        program.name.clone(),
    )
}

#[derive(Clone, Debug)]
struct ParsedOption {
    name: String,
    kind: OptifineOptionKind,
    default_value: String,
    values: Vec<String>,
    description: String,
    source: OptifineOptionSource,
}

fn discover_options(
    files: &HashMap<String, Vec<u8>>,
    properties: &BTreeMap<String, String>,
    screens: &[OptifineScreen],
    diagnostics: &mut Vec<OptifineDiagnostic>,
) -> Vec<OptifineOption> {
    let mut screen_membership = BTreeMap::<String, BTreeSet<String>>::new();
    for screen in screens {
        for entry in &screen.entries {
            if let OptifineScreenEntry::Option(option) = entry {
                screen_membership
                    .entry(option.clone())
                    .or_default()
                    .insert(screen.name.clone());
            }
        }
    }
    let sliders: BTreeSet<_> = property_words(properties, "sliders").into_iter().collect();
    let mut options = BTreeMap::<String, OptifineOption>::new();
    let mut paths: Vec<_> = files
        .keys()
        .filter(|path| is_shader_source_path(path))
        .collect();
    paths.sort();
    for path in paths {
        let Ok(source) = std::str::from_utf8(&files[path]) else {
            continue;
        };
        for (index, line) in source.lines().enumerate() {
            let Some(parsed) = parse_option_line(path, index + 1, line, &screen_membership) else {
                continue;
            };
            match options.get_mut(&parsed.name) {
                Some(existing) => {
                    if existing.kind != parsed.kind
                        || existing.default_value != parsed.default_value
                        || existing.values != parsed.values
                    {
                        diagnostics.push(
                            OptifineDiagnostic::warning(
                                OptifineDiagnosticCode::ConflictingOption,
                                format!(
                                    "option '{}' has conflicting definitions; the first definition is used",
                                    parsed.name
                                ),
                            )
                            .at(path, Some(index + 1)),
                        );
                    }
                    if !existing.sources.contains(&parsed.source) {
                        existing.sources.push(parsed.source);
                    }
                }
                None => {
                    let option_name = parsed.name.clone();
                    let screens = screen_membership
                        .get(&option_name)
                        .map(|names| names.iter().cloned().collect())
                        .unwrap_or_default();
                    options.insert(
                        option_name.clone(),
                        OptifineOption {
                            name: parsed.name,
                            kind: parsed.kind,
                            default_value: parsed.default_value,
                            values: parsed.values,
                            description: parsed.description,
                            slider: sliders.contains(&option_name),
                            screens,
                            sources: vec![parsed.source],
                        },
                    );
                }
            }
        }
    }

    for (name, screen_names) in screen_membership {
        if options.contains_key(&name) {
            continue;
        }
        diagnostics.push(OptifineDiagnostic::warning(
            OptifineDiagnosticCode::UndefinedScreenOption,
            format!("screen references option '{name}' but no shader definition was found"),
        ));
        options.insert(
            name.clone(),
            OptifineOption {
                name: name.clone(),
                kind: OptifineOptionKind::Unknown,
                default_value: String::new(),
                values: Vec::new(),
                description: String::new(),
                slider: sliders.contains(&name),
                screens: screen_names.into_iter().collect(),
                sources: Vec::new(),
            },
        );
    }
    options.into_values().collect()
}

fn is_shader_source_path(path: &str) -> bool {
    path.starts_with("shaders/")
        && Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(
                    extension,
                    "vsh" | "fsh" | "gsh" | "csh" | "tcs" | "tes" | "glsl"
                )
            })
}

fn parse_option_line(
    path: &str,
    line_number: usize,
    line: &str,
    screen_membership: &BTreeMap<String, BTreeSet<String>>,
) -> Option<ParsedOption> {
    if let Some(parsed) = parse_define_option(path, line_number, line, screen_membership) {
        return Some(parsed);
    }
    parse_constant_option(path, line_number, line, screen_membership)
}

fn parse_define_option(
    path: &str,
    line_number: usize,
    line: &str,
    screen_membership: &BTreeMap<String, BTreeSet<String>>,
) -> Option<ParsedOption> {
    let trimmed = line.trim_start();
    let (enabled, definition) = if let Some(definition) = trimmed.strip_prefix("#define") {
        (true, definition)
    } else {
        let commented = trimmed.strip_prefix("//")?.trim_start();
        (false, commented.strip_prefix("#define")?)
    };
    let (code, comment) = split_line_comment(definition);
    let mut tokens = code.split_whitespace();
    let name = tokens.next()?;
    if !is_identifier(name) || name.starts_with("MC_") {
        return None;
    }
    let raw_value = tokens.collect::<Vec<_>>().join(" ");
    let (values, description) = parse_value_list(comment);
    let is_screen_option = screen_membership.contains_key(name);
    let (kind, default_value) = if raw_value.is_empty() {
        if comment.trim().is_empty() && !is_screen_option {
            return None;
        }
        (
            OptifineOptionKind::Boolean,
            if enabled { "true" } else { "false" }.to_string(),
        )
    } else {
        if values.is_empty() && !is_screen_option {
            return None;
        }
        (OptifineOptionKind::Value, raw_value)
    };
    Some(ParsedOption {
        name: name.to_string(),
        kind,
        default_value,
        values,
        description,
        source: OptifineOptionSource {
            path: path.to_string(),
            line: line_number,
        },
    })
}

fn parse_constant_option(
    path: &str,
    line_number: usize,
    line: &str,
    screen_membership: &BTreeMap<String, BTreeSet<String>>,
) -> Option<ParsedOption> {
    let (code, comment) = split_line_comment(line.trim_start());
    let code = code.strip_prefix("const ")?.trim_start();
    let mut tokens = code.split_whitespace();
    let value_type = tokens.next()?;
    if !matches!(value_type, "bool" | "int" | "float") {
        return None;
    }
    let name = tokens.next()?;
    if !is_identifier(name) {
        return None;
    }
    let (_, value) = code.split_once('=')?;
    let default_value = value.trim().trim_end_matches(';').trim().to_string();
    let (values, description) = parse_value_list(comment);
    if values.is_empty() && comment.trim().is_empty() && !screen_membership.contains_key(name) {
        return None;
    }
    Some(ParsedOption {
        name: name.to_string(),
        kind: OptifineOptionKind::Constant,
        default_value,
        values,
        description,
        source: OptifineOptionSource {
            path: path.to_string(),
            line: line_number,
        },
    })
}

fn split_line_comment(line: &str) -> (&str, &str) {
    line.split_once("//").unwrap_or((line, ""))
}

fn parse_value_list(comment: &str) -> (Vec<String>, String) {
    let comment = comment.trim();
    let Some(rest) = comment.strip_prefix('[') else {
        return (Vec::new(), comment.to_string());
    };
    let Some(end) = rest.find(']') else {
        return (Vec::new(), comment.to_string());
    };
    let values = rest[..end].split_whitespace().map(str::to_string).collect();
    (values, rest[end + 1..].trim().to_string())
}

fn compile_program(
    files: &HashMap<String, Vec<u8>>,
    manifest: &OptifineManifest,
    name: &str,
    option_overrides: &BTreeMap<String, String>,
) -> OptifineCompileReport {
    let mut report = OptifineCompileReport {
        program: name.to_string(),
        stages: Vec::new(),
        diagnostics: Vec::new(),
    };
    let Some(program) = manifest.program(name) else {
        report.diagnostics.push(OptifineDiagnostic::error(
            OptifineDiagnosticCode::ProgramNotFound,
            format!("program '{name}' is not present in the shader pack"),
        ));
        return report;
    };
    if let OptifineProgramActivation::Disabled { reason } = &program.activation {
        report.diagnostics.push(OptifineDiagnostic::error(
            OptifineDiagnosticCode::ProgramDisabled,
            format!("program '{name}' is disabled: {reason}"),
        ));
        return report;
    }
    if !program.complete {
        report.diagnostics.push(OptifineDiagnostic::error(
            OptifineDiagnosticCode::IncompleteStagePair,
            format!("program '{name}' does not have a complete, valid stage set"),
        ));
        return report;
    }
    let override_values =
        validate_option_overrides(manifest, option_overrides, &mut report.diagnostics);
    if report
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == OptifineDiagnosticSeverity::Error)
    {
        return report;
    }
    // Thread the patched interface through the stages (Vertex sorts before
    // Fragment): the later stage is seeded with the earlier stage's UBO
    // fields and sampler bindings so both agree on `set 0` layout. Native
    // compilation is deliberately deferred to the active RHI backend.
    let mut seed = super::glsl_patcher::PatchSeed {
        // Composite-style programs (deferred/composite/final/…) draw
        // fullscreen quads with ortho fixed-function matrices in OptiFine.
        fullscreen_pass: !program.pass.is_gbuffers_style(),
        ..Default::default()
    };
    for stage in &program.stages {
        match compile_stage(
            files,
            stage,
            &override_values,
            &seed,
            &mut report.diagnostics,
        ) {
            Some(compiled) => {
                seed.ubo_fields = compiled.ubo_fields.clone();
                seed.sampler_bindings = compiled.sampler_bindings.clone();
                report.stages.push(compiled);
            }
            None => break,
        }
    }
    report
}

/// Determine the colour targets the *active* preprocessor branch writes.
///
/// `DRAWBUFFERS`/`RENDERTARGETS` directives live in comments and may differ
/// per `#if` branch (e.g. Complementary's composite4 writes `3` normally but
/// `30` with motion blur). Taking the wrong branch binds an attachment the
/// shader never writes, which clears that colortex to black and destroys its
/// contents. The directives are therefore rewritten into plain tokens that
/// survive as source comments until the backend compiler runs. The source
/// adapter reports the longest declaration conservatively; a backend-specific
/// executor can later evaluate active preprocessor branches when it builds a
/// full shader program.
fn resolve_draw_buffers(expanded: &str) -> Vec<u8> {
    parse_draw_buffers(expanded)
}

/// Rewrite `/* DRAWBUFFERS:XY */` and `/* RENDERTARGETS: a,b */` comments
/// into bare `rc_ofdb_XY` / `rc_ofrt_a_b` tokens so the preprocessor keeps
/// (and branch-filters) them. The output is only ever preprocessed, never
/// compiled, so stray identifiers are harmless.
fn mark_draw_buffer_directives(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut rest = source;
    while let Some(start) = rest.find("/*") {
        let Some(end) = rest[start + 2..].find("*/") else {
            break;
        };
        let inner = rest[start + 2..start + 2 + end].trim();
        let replacement = if let Some(token) = inner.strip_prefix("DRAWBUFFERS:") {
            let digits: String = token
                .trim()
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            (!digits.is_empty()).then(|| format!("\nrc_ofdb_{digits}\n"))
        } else if let Some(list) = inner.strip_prefix("RENDERTARGETS:") {
            let parts: Vec<String> = list
                .split(',')
                .map(|part| part.trim().to_string())
                .filter(|part| part.chars().all(|c| c.is_ascii_digit()) && !part.is_empty())
                .collect();
            (!parts.is_empty()).then(|| format!("\nrc_ofrt_{}\n", parts.join("_")))
        } else {
            None
        };
        out.push_str(&rest[..start]);
        match replacement {
            Some(marker) => out.push_str(&marker),
            None => out.push_str(&rest[start..start + 2 + end + 2]),
        }
        rest = &rest[start + 2 + end + 2..];
    }
    out.push_str(rest);
    out
}

/// Parse `/* DRAWBUFFERS:0364 */` (OptiFine) and `/* RENDERTARGETS: 0,5,19 */`
/// (Iris) directives. Directives may occur once per preprocessor branch; the
/// longest target list wins so callers can size attachments conservatively.
fn parse_draw_buffers(source: &str) -> Vec<u8> {
    let mut best: Vec<u8> = Vec::new();
    let mut consider = |candidate: Vec<u8>| {
        if candidate.len() > best.len() {
            best = candidate;
        }
    };
    for (start, _) in source.match_indices("DRAWBUFFERS:") {
        let rest = &source[start + "DRAWBUFFERS:".len()..];
        let token = rest
            .split(|c: char| c.is_whitespace() || c == '*')
            .next()
            .unwrap_or("");
        consider(
            token
                .chars()
                .filter_map(|c| c.to_digit(10).map(|d| d as u8))
                .collect(),
        );
    }
    for (start, _) in source.match_indices("RENDERTARGETS:") {
        let rest = &source[start + "RENDERTARGETS:".len()..];
        let list = rest.split("*/").next().unwrap_or("");
        consider(
            list.split(',')
                .filter_map(|part| part.trim().parse::<u8>().ok())
                .collect(),
        );
    }
    best
}

fn validate_option_overrides(
    manifest: &OptifineManifest,
    overrides: &BTreeMap<String, String>,
    diagnostics: &mut Vec<OptifineDiagnostic>,
) -> BTreeMap<String, String> {
    let mut accepted = BTreeMap::new();
    for (name, value) in overrides {
        let Some(option) = manifest.options.iter().find(|option| option.name == *name) else {
            diagnostics.push(OptifineDiagnostic::error(
                OptifineDiagnosticCode::InvalidOptionOverride,
                format!("option override '{name}' does not name a discovered shader option"),
            ));
            continue;
        };
        let valid = match option.kind {
            OptifineOptionKind::Boolean => {
                value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("false")
            }
            OptifineOptionKind::Value | OptifineOptionKind::Constant => {
                option.values.is_empty() || option.values.contains(value)
            }
            OptifineOptionKind::Unknown => false,
        };
        if valid {
            accepted.insert(name.clone(), value.clone());
        } else {
            diagnostics.push(OptifineDiagnostic::error(
                OptifineDiagnosticCode::InvalidOptionOverride,
                format!("'{value}' is not a valid value for shader option '{name}'"),
            ));
        }
    }
    accepted
}

fn compile_stage(
    files: &HashMap<String, Vec<u8>>,
    stage: &OptifineStageSource,
    overrides: &BTreeMap<String, String>,
    seed: &super::glsl_patcher::PatchSeed,
    diagnostics: &mut Vec<OptifineDiagnostic>,
) -> Option<OptifineCompiledStage> {
    let bytes = files.get(&stage.path)?;
    let source = match std::str::from_utf8(bytes) {
        Ok(source) => source,
        Err(_) => {
            diagnostics.push(
                OptifineDiagnostic::error(
                    OptifineDiagnosticCode::CompileError,
                    "shader source is not valid UTF-8",
                )
                .at(&stage.path, None),
            );
            return None;
        }
    };
    diagnose_legacy_semantics(source, &stage.path, diagnostics);
    let source = apply_option_overrides(source, overrides);

    // Recursively expand #include directives so the GLSL patcher sees all
    // uniforms from the entire include tree and can wrap them into one UBO.
    let expanded = match pre_expand_includes(&source, files, &stage.path, overrides, 0) {
        Ok(s) => s,
        Err(e) => {
            diagnostics.push(
                OptifineDiagnostic::error(OptifineDiagnosticCode::IncludeError, e)
                    .at(&stage.path, None),
            );
            return None;
        }
    };

    // Legacy GLSL → 450 compatibility patcher (sees ALL uniforms from includes).
    let stage_kind = match stage.stage {
        OptifineStage::Vertex => "vertex",
        OptifineStage::Fragment => "fragment",
        OptifineStage::Geometry => "geometry",
        OptifineStage::TessellationControl => "tess_control",
        OptifineStage::TessellationEvaluation => "tess_evaluation",
        OptifineStage::Compute => "compute",
    };
    let draw_buffers = if stage.stage == OptifineStage::Fragment {
        resolve_draw_buffers(&expanded)
    } else {
        Vec::new()
    };
    let patched = super::glsl_patcher::patch_optifine_glsl_seeded(&expanded, stage_kind, seed);
    let source = ShaderSource::glsl(patched.source, stage.path.clone())
        .with_origin(ShaderSourceOrigin::Optifine);

    Some(OptifineCompiledStage {
        stage: stage.stage,
        source_path: stage.path.clone(),
        sequence: stage.sequence,
        source,
        draw_buffers,
        ubo_fields: patched.uniform_ubo_fields,
        sampler_bindings: patched.sampler_bindings,
    })
}

/// Recursively resolve `#include` directives so the GLSL patcher sees
/// all uniforms from the entire include tree.
fn pre_expand_includes(
    source: &str,
    files: &HashMap<String, Vec<u8>>,
    base_path: &str,
    overrides: &BTreeMap<String, String>,
    depth: usize,
) -> Result<String, String> {
    if depth > 32 {
        return Err("include depth limit exceeded".to_string());
    }
    let mut out = String::with_capacity(source.len());
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("#include ") {
            let path = rest
                .trim()
                .trim_matches('"')
                .trim_matches('<')
                .trim_matches('>')
                .trim_matches('"');
            let resolved = resolve_include_path(path, true, base_path, base_path)
                .map_err(|e| format!("include resolution failed for '{path}': {e}"))?;
            let bytes = files
                .get(&resolved)
                .ok_or_else(|| format!("'{resolved}' not found"))?;
            let text = std::str::from_utf8(bytes)
                .map_err(|_| format!("'{resolved}' is not valid UTF-8"))?;
            let text = apply_option_overrides(text, overrides);
            // Recursively expand nested includes
            let nested = pre_expand_includes(&text, files, &resolved, overrides, depth + 1)?;
            out.push_str(&nested);
            out.push('\n');
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    Ok(out)
}

fn diagnose_legacy_semantics(source: &str, path: &str, diagnostics: &mut Vec<OptifineDiagnostic>) {
    let legacy_symbols = [
        "attribute ",
        "varying ",
        "ftransform(",
        "gl_Vertex",
        "gl_Normal",
        "gl_Color",
        "gl_MultiTexCoord",
        "gl_ModelViewMatrix",
        "gl_ModelViewProjectionMatrix",
        "gl_ProjectionMatrix",
        "gl_NormalMatrix",
        "gl_FragColor",
        "gl_FragData",
    ];
    let used: Vec<_> = legacy_symbols
        .iter()
        .filter(|symbol| source.contains(**symbol))
        .copied()
        .collect();
    if !used.is_empty() {
        diagnostics.push(
            OptifineDiagnostic::warning(
                OptifineDiagnosticCode::LegacyOpenGlSemantics,
                format!(
                    "legacy OpenGL symbols require a future ABI translation layer: {}",
                    used.join(", ")
                ),
            )
            .at(path, None),
        );
    }
}

fn apply_option_overrides(source: &str, overrides: &BTreeMap<String, String>) -> String {
    if overrides.is_empty() {
        return source.to_string();
    }
    let mut output = String::with_capacity(source.len());
    for line in source.lines() {
        let replacement = rewrite_option_line(line, overrides);
        output.push_str(replacement.as_deref().unwrap_or(line));
        output.push('\n');
    }
    output
}

fn rewrite_option_line(line: &str, overrides: &BTreeMap<String, String>) -> Option<String> {
    let leading_len = line.len() - line.trim_start().len();
    let leading = &line[..leading_len];
    let trimmed = line.trim_start();
    if let Some(rewritten) = rewrite_define_option(leading, trimmed, overrides) {
        return Some(rewritten);
    }
    rewrite_constant_option(leading, trimmed, overrides)
}

fn rewrite_define_option(
    leading: &str,
    trimmed: &str,
    overrides: &BTreeMap<String, String>,
) -> Option<String> {
    let (commented, definition) = if let Some(definition) = trimmed.strip_prefix("#define") {
        (false, definition)
    } else {
        let after_comment = trimmed.strip_prefix("//")?.trim_start();
        (true, after_comment.strip_prefix("#define")?)
    };
    let (code, comment) = split_line_comment(definition);
    let mut tokens = code.split_whitespace();
    let name = tokens.next()?;
    let value = overrides.get(name)?;
    let old_value = tokens.collect::<Vec<_>>().join(" ");
    let comment_suffix = if comment.is_empty() {
        String::new()
    } else {
        format!(" //{comment}")
    };
    if old_value.is_empty() {
        let enabled = value.eq_ignore_ascii_case("true");
        return Some(format!(
            "{leading}{}#define {name}{comment_suffix}",
            if enabled { "" } else { "//" }
        ));
    }
    if commented {
        return None;
    }
    Some(format!("{leading}#define {name} {value}{comment_suffix}"))
}

fn rewrite_constant_option(
    leading: &str,
    trimmed: &str,
    overrides: &BTreeMap<String, String>,
) -> Option<String> {
    let (code, comment) = split_line_comment(trimmed);
    let declaration = code.strip_prefix("const ")?.trim_start();
    let (left, _) = declaration.split_once('=')?;
    let mut tokens = left.split_whitespace();
    let value_type = tokens.next()?;
    if !matches!(value_type, "bool" | "int" | "float") {
        return None;
    }
    let name = tokens.next()?;
    let value = overrides.get(name)?;
    let comment_suffix = if comment.is_empty() {
        String::new()
    } else {
        format!(" //{comment}")
    };
    Some(format!(
        "{leading}const {value_type} {name} = {value};{comment_suffix}"
    ))
}

fn resolve_include_path(
    requested: &str,
    relative: bool,
    requesting: &str,
    root_path: &str,
) -> Result<String, String> {
    let requested = requested.replace('\\', "/");
    let candidate = if requested.starts_with('/') {
        PathBuf::from("shaders").join(requested.trim_start_matches('/'))
    } else if relative {
        Path::new(if requesting.is_empty() {
            root_path
        } else {
            requesting
        })
        .parent()
        .unwrap_or_else(|| Path::new("shaders"))
        .join(requested)
    } else {
        PathBuf::from("shaders").join(requested)
    };
    normalize_include_path(&candidate)
}

fn normalize_include_path(path: &Path) -> Result<String, String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.pop().is_none() {
                    return Err("include escapes the shader pack".to_string());
                }
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err("absolute include paths are not allowed".to_string())
            }
        }
    }
    let normalized = parts.join("/");
    if !normalized.starts_with("shaders/") {
        return Err("include escapes the shaders directory".to_string());
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage_source(version: u16, body: &str) -> Vec<u8> {
        format!("#version {version}\n{body}\n").into_bytes()
    }

    #[test]
    fn draw_buffers_use_conservative_longest_declaration() {
        let source = "#version 120\n#define MOTION_BLUR 0\nvoid main() {\n/* DRAWBUFFERS:3 */\n#if MOTION_BLUR == 1\n/* DRAWBUFFERS:30 */\n#endif\n}\n";
        assert_eq!(
            resolve_draw_buffers(source),
            vec![3, 0],
            "the source adapter must preserve enough targets for every branch"
        );
    }

    #[test]
    fn draw_buffer_markers_survive_only_in_comments_with_directives() {
        let marked = mark_draw_buffer_directives(
            "/* DRAWBUFFERS:05 */\n/* keep this comment */\n/* RENDERTARGETS: 0,15,3 */\n",
        );
        assert!(marked.contains("rc_ofdb_05"));
        assert!(marked.contains("/* keep this comment */"));
        assert!(marked.contains("rc_ofrt_0_15_3"));
    }

    #[test]
    fn builds_ordered_manifest_with_stage_pairs_disables_and_options() {
        let files = HashMap::from([
            (
                PROPERTIES_PATH.to_string(),
                br#"
screen = SHADOWS [POST] <profile> *
screen.POST = BLOOM EXPOSURE
screen.POST.columns = 3
sliders = EXPOSURE
profile.LOW = !BLOOM EXPOSURE=0.5 !program.composite1
program.composite1.enabled = false
iris.features.required = CUSTOM_IMAGES SSBO
"#
                .to_vec(),
            ),
            (
                "shaders/gbuffers_basic.vsh".to_string(),
                stage_source(450, "#define SHADOWS // shadows\nvoid main() {}"),
            ),
            (
                "shaders/gbuffers_basic.fsh".to_string(),
                stage_source(450, "void main() {}"),
            ),
            (
                "shaders/shadow.vsh".to_string(),
                stage_source(450, "void main() {}"),
            ),
            (
                "shaders/shadow.fsh".to_string(),
                stage_source(450, "void main() {}"),
            ),
            (
                "shaders/composite1.vsh".to_string(),
                stage_source(
                    450,
                    "#define BLOOM // bloom\n#define EXPOSURE 1.0 // [0.5 1.0 2.0] exposure\nvoid main() {}",
                ),
            ),
            (
                "shaders/composite1.fsh".to_string(),
                stage_source(450, "void main() {}"),
            ),
            (
                "shaders/final.vsh".to_string(),
                stage_source(450, "void main() {}"),
            ),
            (
                "shaders/final.fsh".to_string(),
                stage_source(450, "void main() {}"),
            ),
        ]);

        let manifest = manifest_from_files(&files).unwrap();
        assert_eq!(
            manifest
                .programs
                .iter()
                .map(|program| program.name.as_str())
                .collect::<Vec<_>>(),
            ["shadow", "gbuffers_basic", "composite1", "final"]
        );
        assert!(manifest
            .program("composite1")
            .unwrap()
            .activation
            .is_unconditionally_disabled());
        assert_eq!(manifest.program("gbuffers_basic").unwrap().stages.len(), 2);
        assert_eq!(manifest.required_iris_features, ["CUSTOM_IMAGES", "SSBO"]);
        assert_eq!(manifest.options.len(), 3);
        let exposure = manifest
            .options
            .iter()
            .find(|option| option.name == "EXPOSURE")
            .unwrap();
        assert!(exposure.slider);
        assert_eq!(exposure.values, ["0.5", "1.0", "2.0"]);
        assert_eq!(manifest.screens[1].columns, Some(3));
        assert!(manifest.profiles[0]
            .actions
            .contains(&OptifineProfileAction::DisableProgram(
                "composite1".to_string()
            )));
    }

    #[test]
    fn groups_compute_suffixes_and_reports_incomplete_pairs() {
        let files = HashMap::from([
            (
                "shaders/setup.csh".to_string(),
                stage_source(450, "layout(local_size_x=1) in; void main() {}"),
            ),
            (
                "shaders/composite.csh".to_string(),
                stage_source(450, "layout(local_size_x=1) in; void main() {}"),
            ),
            (
                "shaders/composite_a.csh".to_string(),
                stage_source(450, "layout(local_size_x=1) in; void main() {}"),
            ),
            (
                "shaders/gbuffers_entities.vsh".to_string(),
                stage_source(450, "void main() {}"),
            ),
        ]);
        let manifest = manifest_from_files(&files).unwrap();
        let composite = manifest.program("composite").unwrap();
        assert_eq!(composite.stages.len(), 2);
        assert_eq!(
            composite
                .stages
                .iter()
                .map(|stage| stage.sequence)
                .collect::<Vec<_>>(),
            [0, 1]
        );
        assert!(manifest.program("setup").unwrap().complete);
        assert!(!manifest.program("gbuffers_entities").unwrap().complete);
        assert!(manifest
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == OptifineDiagnosticCode::IncompleteStagePair }));
    }

    #[test]
    fn compiles_vulkan_style_program_with_includes_and_option_override() {
        let files = HashMap::from([
            (
                PROPERTIES_PATH.to_string(),
                b"screen=SCALE\nsliders=SCALE\n".to_vec(),
            ),
            (
                "shaders/gbuffers_basic.vsh".to_string(),
                stage_source(
                    450,
                    "#include \"/lib/options.glsl\"\nlayout(location=0) out vec2 uv;\nvoid main() { uv=vec2(SCALE); gl_Position=vec4(0.0); }",
                ),
            ),
            (
                "shaders/gbuffers_basic.fsh".to_string(),
                stage_source(
                    450,
                    "layout(location=0) in vec2 uv;\nlayout(location=0) out vec4 color;\nvoid main() { color=vec4(uv,0.0,1.0); }",
                ),
            ),
            (
                "shaders/lib/options.glsl".to_string(),
                b"#define SCALE 1.0 // [0.5 1.0] scale\n".to_vec(),
            ),
        ]);
        let pack = OptifineShaderPack::from_files(files).unwrap();
        let report = pack.compile_program(
            "gbuffers_basic",
            &BTreeMap::from([("SCALE".to_string(), "0.5".to_string())]),
        );
        assert!(
            report.succeeded(),
            "compile diagnostics: {:?}",
            report.diagnostics
        );
        assert_eq!(report.stages.len(), 2);
        assert!(report
            .stages
            .iter()
            .all(|stage| stage.source.language == ShaderLanguage::Glsl));
    }

    #[test]
    fn parses_continued_properties_and_keeps_conditional_activation() {
        let mut diagnostics = Vec::new();
        let properties = parse_properties(
            Some(
                &b"screen = BLOOM \\\n EXPOSURE\nprogram.final.enabled = BLOOM && !DEBUG\n"
                    .to_vec(),
            ),
            &mut diagnostics,
        );
        assert_eq!(properties["screen"], "BLOOM  EXPOSURE");
        let activation = program_activation("final", OptifinePass::Final, &properties);
        assert_eq!(
            activation,
            OptifineProgramActivation::Conditional {
                expression: "BLOOM && !DEBUG".to_string()
            }
        );
    }

    #[test]
    fn evaluates_program_activation_with_boolean_precedence_and_overrides() {
        let activation = OptifineProgramActivation::Conditional {
            expression: "SHADOW && MULTICOLORED_BLOCKLIGHT || FXAA && !RETRO_FILTER".to_string(),
        };
        let mut values = BTreeMap::from([
            ("SHADOW".to_string(), "true".to_string()),
            ("MULTICOLORED_BLOCKLIGHT".to_string(), "false".to_string()),
            ("FXAA".to_string(), "true".to_string()),
            ("RETRO_FILTER".to_string(), "false".to_string()),
        ]);
        assert!(activation.evaluate(&values).unwrap());
        values.insert("RETRO_FILTER".to_string(), "true".to_string());
        assert!(!activation.evaluate(&values).unwrap());
        values.insert("MULTICOLORED_BLOCKLIGHT".to_string(), "true".to_string());
        assert!(activation.evaluate(&values).unwrap());
    }

    #[test]
    fn evaluates_parentheses_comparisons_and_undefined_options() {
        let values = BTreeMap::from([
            ("QUALITY".to_string(), "HIGH".to_string()),
            ("COUNT".to_string(), "2".to_string()),
        ]);
        assert!(
            evaluate_boolean_expression("(QUALITY == HIGH && COUNT != 0) || UNKNOWN", &values)
                .unwrap()
        );
        assert!(!evaluate_boolean_expression("UNKNOWN || COUNT == 0", &values).unwrap());
        assert!(evaluate_boolean_expression("!UNKNOWN", &values).unwrap());
        assert!(evaluate_boolean_expression("COUNT", &values).unwrap());
        assert!(evaluate_boolean_expression("A && (B", &values).is_err());
    }

    #[test]
    fn rewrites_boolean_value_and_constant_options() {
        let source = r#"
//#define BLOOM // bloom
#define QUALITY 1 // [1 2 3] quality
const int SHADOW_SAMPLES = 4; // [2 4 8] samples
"#;
        let rewritten = apply_option_overrides(
            source,
            &BTreeMap::from([
                ("BLOOM".to_string(), "true".to_string()),
                ("QUALITY".to_string(), "3".to_string()),
                ("SHADOW_SAMPLES".to_string(), "8".to_string()),
            ]),
        );
        assert!(rewritten.contains("#define BLOOM // bloom"));
        assert!(rewritten.contains("#define QUALITY 3 // [1 2 3] quality"));
        assert!(rewritten.contains("const int SHADOW_SAMPLES = 8; // [2 4 8] samples"));
    }

    /// Compile-coverage harness against the bundled ComplementaryReimagined
    /// pack. Run manually:
    /// `cargo test complementary_reimagined_compile_coverage -- --ignored --nocapture`
    #[test]
    #[ignore = "requires shaderpacks/ComplementaryReimagined_r5.8.1.zip and is slow"]
    fn complementary_reimagined_compile_coverage() {
        let pack = crate::render::shader_pack::load_optifine_shader_pack(
            "ComplementaryReimagined_r5.8.1.zip",
        )
        .expect("pack should load");
        let manifest = pack.manifest();
        let values = manifest.effective_option_values(&BTreeMap::new());
        let candidates: Vec<_> = manifest
            .programs
            .iter()
            .filter(|program| {
                program.complete && manifest.program_is_active(program, &values).unwrap()
            })
            .collect();
        let mut ok = 0usize;
        let mut failures: Vec<(String, String)> = Vec::new();
        for program in &candidates {
            let report = pack.compile_program(&program.name, &BTreeMap::new());
            if report.succeeded() {
                ok += 1;
            } else {
                let error = report
                    .diagnostics
                    .iter()
                    .find(|d| d.severity == OptifineDiagnosticSeverity::Error)
                    .map(|d| {
                        format!(
                            "{}: {}",
                            d.path.as_deref().unwrap_or("?"),
                            d.message.chars().take(700).collect::<String>()
                        )
                    })
                    .unwrap_or_default();
                failures.push((program.name.clone(), error));
            }
        }
        println!("== compiled {ok}/{} programs ==", candidates.len());
        for (name, error) in &failures {
            println!("FAIL {name}\n    {error}\n");
        }
        assert!(
            failures.is_empty(),
            "{} of {} programs failed to compile",
            failures.len(),
            candidates.len()
        );
    }

    #[test]
    fn parses_drawbuffers_and_rendertargets_directives() {
        assert_eq!(
            parse_draw_buffers("/* DRAWBUFFERS:0364 */"),
            vec![0, 3, 6, 4]
        );
        assert_eq!(
            parse_draw_buffers("/* RENDERTARGETS: 0,5,4,19,18 */"),
            vec![0, 5, 4, 19, 18]
        );
        // Longest branch wins when directives are conditional.
        assert_eq!(
            parse_draw_buffers(
                "#ifdef A\n/* DRAWBUFFERS:06 */\n#else\n/* DRAWBUFFERS:0 */\n#endif"
            ),
            vec![0, 6]
        );
        assert_eq!(parse_draw_buffers("void main() {}"), Vec::<u8>::new());
    }

    #[test]
    fn include_resolution_stays_inside_shader_directory() {
        assert_eq!(
            resolve_include_path(
                "../lib/common.glsl",
                true,
                "shaders/world0/composite.vsh",
                "shaders/world0/composite.vsh",
            )
            .unwrap(),
            "shaders/lib/common.glsl"
        );
        assert!(resolve_include_path(
            "../../../secret",
            true,
            "shaders/composite.vsh",
            "shaders/composite.vsh",
        )
        .is_err());
    }
}
