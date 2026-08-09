//! GLSL preprocessor: transforms OptiFine/Iris legacy GLSL (120–430) into
//! Vulkan-compatible GLSL 450 with UBO wrapping and legacy-builtin replacement.
//!
//! Invoked by `compile_stage()` in `optifine.rs` after `pre_expand_includes`
//! has inlined the whole include tree, so every uniform/attribute/varying in
//! the program is visible in a single source string.
//!
//! Key rules:
//! - Non-opaque `uniform` declarations are hoisted (deduplicated by name)
//!   into one `FrameUniforms` UBO at `set=0, binding=0`. Hoisting them out of
//!   `#if` blocks is safe: an unreferenced UBO field only wastes space.
//! - Opaque uniforms (samplers/images) are rewritten **in place** with a
//!   per-name binding so mutually exclusive `#if` branches that declare the
//!   same sampler with different types (e.g. `shadowtex0` as `sampler2D` vs
//!   `sampler2DShadow`) keep exactly one live declaration after
//!   preprocessing, at a stable binding.
//! - `attribute`/`varying` declarations are hoisted to the preamble,
//!   deduplicated by name. OptiFine builtin attribute replacements
//!   (`gl_Vertex` → `vaPosition`, ...) get fixed locations 0–6; custom
//!   attributes (`mc_Entity`, `at_tangent`, ...) start at location 8.

use renderer_api::rhi::types::ShaderBindingType;
use std::collections::BTreeMap;

/// Fixed vertex-input locations for the replacements of legacy GL builtins,
/// with the types Iris 1.17+ shaders expect when using these names natively
/// (`vaPosition` is vec3 — legacy `gl_Vertex` uses wrap it in `vec4(…, 1.0)`).
/// Location 7 is left free; custom pack attributes start at 8.
const BUILTIN_ATTRIBUTES: &[(&str, &str, u32)] = &[
    ("vaPosition", "vec3", 0),
    ("vaColor", "vec4", 1),
    ("vaNormal", "vec3", 2),
    ("vaUV0", "vec2", 3),
    ("vaUV1", "ivec2", 4),
    ("vaUV2", "ivec2", 5),
    ("vaUV3", "vec2", 6),
];
const CUSTOM_ATTRIBUTE_BASE_LOCATION: u32 = 8;
const MAX_FRAG_DATA: usize = 8;

/// UBO fields injected when the source references them (via a legacy-builtin
/// replacement or Iris 1.17+ core-profile names) without declaring them.
/// The bool marks fog fields that are only injected when `gl_Fog` was used,
/// since packs may define identically named globals of their own.
const INJECTED_UNIFORMS: &[(&str, &str, bool)] = &[
    ("gbufferModelView", "mat4", false),
    ("gbufferModelViewInverse", "mat4", false),
    ("gbufferProjection", "mat4", false),
    ("gbufferProjectionInverse", "mat4", false),
    ("modelViewMatrix", "mat4", false),
    ("modelViewMatrixInverse", "mat4", false),
    ("projectionMatrix", "mat4", false),
    ("projectionMatrixInverse", "mat4", false),
    ("normalMatrix", "mat3", false),
    ("textureMatrix", "mat4", false),
    ("chunkOffset", "vec3", false),
    ("fogStart", "float", true),
    ("fogEnd", "float", true),
    ("fogDensity", "float", true),
    ("fogColor", "vec3", true),
];

/// Strip precision qualifiers, `const`, and initializers from a UBO field
/// declaration — Vulkan GLSL does not allow these inside uniform blocks.
fn clean_ubo_field(decl: &str) -> String {
    let mut d = decl.to_string();
    for q in &["highp ", "mediump ", "lowp "] {
        d = d.replace(q, "");
    }
    d = d.replacen("const ", "", 1);
    if let Some(eq) = d.find('=') {
        d = d[..eq].trim().to_string();
    }
    d
}

/// Split a GLSL declaration into one declaration per variable.
///
/// OptiFine packs commonly group interface variables (`float near, far` or
/// `varying vec3 sunVec, upVec`). Vulkan assigns a location to every variable,
/// while the old patcher counted the whole line as one slot and keyed it only
/// by the final name. Normalising the declaration first keeps UBO members and
/// interface locations unambiguous.
fn split_declaration(decl: &str) -> Vec<(String, String)> {
    fn top_level_parts(source: &str, delimiter: u8) -> Vec<&str> {
        let bytes = source.as_bytes();
        let mut parts = Vec::new();
        let mut start = 0usize;
        let mut parens = 0u32;
        let mut brackets = 0u32;
        let mut braces = 0u32;
        for (index, byte) in bytes.iter().copied().enumerate() {
            match byte {
                b'(' => parens += 1,
                b')' => parens = parens.saturating_sub(1),
                b'[' => brackets += 1,
                b']' => brackets = brackets.saturating_sub(1),
                b'{' => braces += 1,
                b'}' => braces = braces.saturating_sub(1),
                _ => {}
            }
            if byte == delimiter && parens == 0 && brackets == 0 && braces == 0 {
                parts.push(source[start..index].trim());
                start = index + 1;
            }
        }
        parts.push(source[start..].trim());
        parts
    }

    fn without_initializer(source: &str) -> &str {
        top_level_parts(source, b'=')
            .into_iter()
            .next()
            .unwrap_or(source)
            .trim()
    }

    let cleaned = decl.trim().trim_end_matches(';').trim();
    let parts = top_level_parts(cleaned, b',');
    let Some(first) = parts.first().copied() else {
        return Vec::new();
    };
    let first = without_initializer(first);
    let Some(split_at) = first.rfind(char::is_whitespace) else {
        return Vec::new();
    };
    let type_prefix = first[..split_at]
        .split_whitespace()
        .filter(|token| !matches!(*token, "const" | "highp" | "mediump" | "lowp"))
        .collect::<Vec<_>>()
        .join(" ");
    let first_declarator = first[split_at..].trim();
    if type_prefix.is_empty() || first_declarator.is_empty() {
        return Vec::new();
    }

    parts
        .into_iter()
        .enumerate()
        .filter_map(|(index, part)| {
            let declarator = if index == 0 {
                first_declarator
            } else {
                without_initializer(part)
            };
            let name = declarator
                .trim_start()
                .bytes()
                .take_while(|byte| is_ident_char(*byte))
                .map(char::from)
                .collect::<String>();
            (!name.is_empty()).then(|| (name, format!("{type_prefix} {declarator}")))
        })
        .collect()
}

/// Split an optionally-qualified uniform declaration.
///
/// Image declarations emitted by OptiFine/Iris frequently put memory
/// qualifiers before `uniform` (`writeonly uniform image3D target`). Keep the
/// qualifiers that affect image access, while dropping precision qualifiers
/// that are meaningless in desktop Vulkan GLSL and illegal on UBO members.
fn split_qualified_uniform(trimmed: &str) -> Option<(String, &str)> {
    let bytes = trimmed.as_bytes();
    let mut cursor = 0usize;
    let mut retained = Vec::new();

    loop {
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        let start = cursor;
        while cursor < bytes.len() && is_ident_char(bytes[cursor]) {
            cursor += 1;
        }
        if start == cursor {
            return None;
        }
        let word = &trimmed[start..cursor];
        match word {
            "uniform" => {
                let declaration = trimmed[cursor..].trim_start();
                return (!declaration.is_empty()).then(|| (retained.join(" "), declaration));
            }
            "coherent" | "volatile" | "restrict" | "readonly" | "writeonly" | "precise" => {
                retained.push(word);
            }
            "highp" | "mediump" | "lowp" => {}
            _ => return None,
        }
    }
}

/// First non-whitespace byte in code after `cursor`, skipping comments too.
/// This lets identifier rewriting recognise calls such as
/// `texture /* compatibility */ (sampler, uv)`.
fn next_code_byte(source: &str, mut cursor: usize) -> Option<u8> {
    let bytes = source.as_bytes();
    loop {
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }
        if source[cursor..].starts_with("//") {
            cursor = source[cursor..]
                .find('\n')
                .map(|offset| cursor + offset + 1)
                .unwrap_or(bytes.len());
            continue;
        }
        if source[cursor..].starts_with("/*") {
            cursor = source[cursor + 2..]
                .find("*/")
                .map(|offset| cursor + 2 + offset + 2)
                .unwrap_or(bytes.len());
            continue;
        }
        return bytes.get(cursor).copied();
    }
}

/// Rewrite identifier tokens in GLSL code while leaving comments and quoted
/// preprocessor text byte-for-byte intact.
fn rewrite_code_identifiers<'a>(
    source: &str,
    mut replacement: impl FnMut(&str, bool, bool) -> Option<&'a str>,
) -> String {
    let bytes = source.as_bytes();
    let mut output = String::with_capacity(source.len() + 32);
    let mut cursor = 0usize;

    while cursor < bytes.len() {
        if source[cursor..].starts_with("//") {
            let end = source[cursor..]
                .find('\n')
                .map(|offset| cursor + offset + 1)
                .unwrap_or(bytes.len());
            output.push_str(&source[cursor..end]);
            cursor = end;
            continue;
        }
        if source[cursor..].starts_with("/*") {
            let end = source[cursor + 2..]
                .find("*/")
                .map(|offset| cursor + 2 + offset + 2)
                .unwrap_or(bytes.len());
            output.push_str(&source[cursor..end]);
            cursor = end;
            continue;
        }
        if matches!(bytes[cursor], b'\"' | b'\'') {
            let quote = bytes[cursor];
            let start = cursor;
            cursor += 1;
            let mut escaped = false;
            while cursor < bytes.len() {
                let byte = bytes[cursor];
                cursor += 1;
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == quote {
                    break;
                }
            }
            output.push_str(&source[start..cursor]);
            continue;
        }
        if is_ident_char(bytes[cursor]) {
            let start = cursor;
            cursor += 1;
            while cursor < bytes.len() && is_ident_char(bytes[cursor]) {
                cursor += 1;
            }
            let token = &source[start..cursor];
            let mut previous = start;
            while previous > 0 && bytes[previous - 1].is_ascii_whitespace() {
                previous -= 1;
            }
            let member_access = previous > 0 && bytes[previous - 1] == b'.';
            let function_call = next_code_byte(source, cursor) == Some(b'(');
            output.push_str(replacement(token, function_call, member_access).unwrap_or(token));
            continue;
        }

        let ch = source[cursor..]
            .chars()
            .next()
            .expect("valid UTF-8 boundary");
        output.push(ch);
        cursor += ch.len_utf8();
    }

    output
}

/// Rename references to an identifier without touching a same-named function
/// call. BSL declares `uniform sampler2D texture`; after `texture2D` is adapted
/// to the core `texture()` function, the variable would otherwise shadow that
/// builtin and shaderc rejects every call.
fn rename_identifier_references(source: &str, old: &str, new: &str) -> String {
    rewrite_code_identifiers(source, |token, function_call, member_access| {
        (token == old && !function_call && !member_access).then_some(new)
    })
}

/// Replace legacy texture/shadow function *tokens*, not arbitrary substrings.
/// Token-aware matching also accepts whitespace between a function name and
/// its opening parenthesis (`texture2D (sampler, uv)`).
fn replace_legacy_function_calls(source: &str) -> String {
    rewrite_code_identifiers(source, |token, function_call, _| {
        if !function_call {
            return None;
        }
        match token {
            "texture2DGradARB" | "texture2DGrad" => Some("textureGrad"),
            "texture2DProjLod" => Some("textureProjLod"),
            "texture2DProj" => Some("textureProj"),
            "texture2DLod" | "texture3DLod" => Some("textureLod"),
            "texelFetch2D" => Some("texelFetch"),
            "texture2D" | "texture3D" | "texture1D" => Some("texture"),
            "shadow2DLod" => Some("rc_shadow2DLod"),
            "shadow2D" => Some("rc_shadow2D"),
            _ => None,
        }
    })
}

/// Variable name of a declaration like `vec3 offsets[9]` → `offsets`.
fn decl_name(decl: &str) -> String {
    decl.split_whitespace()
        .last()
        .unwrap_or("")
        .trim_end_matches(';')
        .split('[')
        .next()
        .unwrap_or("")
        .trim()
        .to_string()
}

fn is_ident_char(c: u8) -> bool {
    c == b'_' || c.is_ascii_alphanumeric()
}

/// Whole-word occurrence check (`vaUV0` must not match `vaUV01`).
fn contains_token(source: &str, token: &str) -> bool {
    let bytes = source.as_bytes();
    let mut start = 0;
    while let Some(pos) = source[start..].find(token) {
        let begin = start + pos;
        let end = begin + token.len();
        let ok_before = begin == 0 || !is_ident_char(bytes[begin - 1]);
        let ok_after = end >= bytes.len() || !is_ident_char(bytes[end]);
        if ok_before && ok_after {
            return true;
        }
        start = begin + token.len().max(1);
    }
    false
}

/// Number of interface-location slots a varying declaration consumes.
fn location_slots(decl: &str) -> u32 {
    let array_len = decl
        .split_once('[')
        .and_then(|(_, rest)| rest.split(']').next())
        .and_then(|n| n.trim().parse::<u32>().ok())
        .unwrap_or(1);
    let base = if decl.contains("mat4") {
        4
    } else if decl.contains("mat3") {
        3
    } else if decl.contains("mat2") {
        2
    } else {
        1
    };
    base * array_len
}

/// Result of patching a GLSL source.
pub struct PatchedGlsl {
    pub source: String,
    pub uniform_ubo_fields: Vec<String>,
    /// Opaque resource name → `(set=0 binding, RHI resource kind)` chosen by
    /// the patcher. Keeping the kind here is important for Iris image
    /// declarations: an `image3D` must not be treated as a sampled texture.
    pub sampler_bindings: Vec<(String, u32, ShaderBindingType)>,
    pub sampler_count: usize,
    pub attribute_count: usize,
}

/// Cross-stage interface seed. The patcher runs once per stage; without a
/// seed each stage would assign UBO field offsets and sampler bindings in
/// its own declaration order, and the stages of one program would disagree
/// about what lives at `set 0, binding N`. Seeding the later stage with the
/// earlier stage's interface keeps the shared prefix identical: seeded UBO
/// fields are always emitted (even when unused) so std140 offsets match, and
/// seeded sampler names keep their binding when redeclared.
#[derive(Clone, Default)]
pub struct PatchSeed {
    pub ubo_fields: Vec<String>,
    pub sampler_bindings: Vec<(String, u32, ShaderBindingType)>,
    /// Composite/deferred/final draw fullscreen quads with the fixed-function
    /// matrices set to `ortho(0..1)`, not the world camera. In this mode the
    /// legacy matrix builtins map to `rc_fullscreenOrtho`/identity instead of
    /// the gbuffer camera uniforms.
    pub fullscreen_pass: bool,
}

/// Lightweight patch for included files: only applies text replacements
/// (legacy functions, built-ins) without UBO wrapping or version changes.
/// Only used by the shaderc include callback; the main path pre-expands
/// includes before `patch_optifine_glsl` so this rarely runs.
pub fn patch_include_glsl(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("#version ") || trimmed.starts_with("#extension ") {
            continue;
        }
        if let Some((qualifiers, rest)) = split_qualified_uniform(trimmed) {
            let cleaned = clean_ubo_field(rest.trim_end_matches(';').trim());
            let qualifiers = if qualifiers.is_empty() {
                String::new()
            } else {
                format!("{qualifiers} ")
            };
            out.push_str(&format!("{qualifiers}uniform {};\n", cleaned));
            continue;
        }
        let mut patched = line.to_string();
        let t = patched.trim();
        if t.starts_with("varying ") {
            let rest = t.strip_prefix("varying ").unwrap_or("");
            patched = format!("    {};", rest.trim_end_matches(';'));
        } else if t.starts_with("attribute ") {
            patched = format!("    // attribute dropped: {}", &t[10..]);
        }
        patched = replace_legacy_tokens(&patched, false);
        out.push_str(&patched);
        out.push('\n');
    }
    out
}

/// Text replacements shared by the include patch and the full patch.
/// Order matters: longer tokens must be replaced before their prefixes
/// (`gl_VertexID` before `gl_Vertex`, `gl_NormalMatrix` before `gl_Normal`).
fn replace_legacy_tokens(source: &str, fullscreen_pass: bool) -> String {
    let mut s = replace_legacy_function_calls(source)
        // gl_FragColor / gl_FragData[N]
        .replace("gl_FragColor", "fragData0");
    for i in (0..MAX_FRAG_DATA).rev() {
        s = s.replace(&format!("gl_FragData[{i}]"), &format!("fragData{i}"));
    }
    for i in 0..8 {
        s = s.replace(&format!("gl_TextureMatrix[{i}]"), "mat4(1.0)");
    }
    s = if fullscreen_pass {
        // Composite-style passes draw a fullscreen quad with the GL
        // fixed-function matrices set to ortho(0..1); the gbuffer* uniforms
        // separately hold the world camera. The quad vertex buffer supplies
        // positions in [0,1]², so the ortho stand-in maps them to NDC.
        s.replace(
            "ftransform()",
            "(rc_fullscreenOrtho * vec4(vaPosition, 1.0))",
        )
        .replace("gl_ModelViewProjectionMatrix", "rc_fullscreenOrtho")
        .replace("gl_ModelViewMatrixInverse", "mat4(1.0)")
        .replace("gl_ModelViewMatrix", "mat4(1.0)")
        .replace("gl_ProjectionMatrixInverse", "inverse(rc_fullscreenOrtho)")
        .replace("gl_ProjectionMatrix", "rc_fullscreenOrtho")
        .replace("gl_NormalMatrix", "mat3(1.0)")
    } else {
        // ftransform() — world passes transform by the camera matrices.
        s.replace(
            "ftransform()",
            "(gbufferProjection * gbufferModelView * vec4(vaPosition, 1.0))",
        )
        // Legacy matrix builtins (inverse variants first: prefix overlap)
        .replace(
            "gl_ModelViewProjectionMatrix",
            "(gbufferProjection * gbufferModelView)",
        )
        .replace("gl_ModelViewMatrixInverse", "gbufferModelViewInverse")
        .replace("gl_ModelViewMatrix", "gbufferModelView")
        .replace("gl_ProjectionMatrixInverse", "gbufferProjectionInverse")
        .replace("gl_ProjectionMatrix", "gbufferProjection")
        .replace("gl_NormalMatrix", "mat3(gbufferModelView)")
    };
    s = s
        // Fog state struct
        .replace("gl_Fog.scale", "(1.0 / max(fogEnd - fogStart, 0.0001))")
        .replace("gl_Fog.start", "fogStart")
        .replace("gl_Fog.end", "fogEnd")
        .replace("gl_Fog.density", "fogDensity")
        .replace("gl_Fog.color", "vec4(fogColor, 1.0)")
        .replace("gl_FogFragCoord", "rc_fogFragCoord")
        // Vulkan renames. `gl_Vertex` is a prefix of both the legacy and the
        // Vulkan spelling, so park them in placeholders until after the
        // attribute replacement below.
        .replace("gl_VertexIndex", "RC_GL_VERTEX_INDEX")
        .replace("gl_VertexID", "RC_GL_VERTEX_INDEX")
        .replace("gl_InstanceIndex", "RC_GL_INSTANCE_INDEX")
        .replace("gl_InstanceID", "RC_GL_INSTANCE_INDEX")
        // Vertex attributes (legacy builtins are vec4; the Iris-style
        // replacements are narrower, so wrap them back up to vec4)
        .replace("gl_Vertex", "vec4(vaPosition, 1.0)")
        .replace("RC_GL_VERTEX_INDEX", "gl_VertexIndex")
        .replace("RC_GL_INSTANCE_INDEX", "gl_InstanceIndex")
        .replace("gl_Color", "vaColor")
        .replace("gl_Normal", "vaNormal");
    for i in 0..4 {
        s = s.replace(
            &format!("gl_MultiTexCoord{i}"),
            &format!("vec4(vaUV{i}, 0.0, 1.0)"),
        );
    }
    s
}

/// Split leading interpolation/auxiliary qualifiers off a `varying` line.
/// `flat varying vec4 x` → `("flat", "vec4 x")`; returns None when the line
/// is not a varying declaration.
fn split_varying(trimmed: &str) -> Option<(String, String)> {
    let mut qualifiers = Vec::new();
    let mut rest = trimmed;
    loop {
        rest = rest.trim_start();
        let word = rest.split_whitespace().next()?;
        match word {
            "flat" | "noperspective" | "smooth" | "centroid" | "invariant" => {
                qualifiers.push(word);
                rest = &rest[word.len()..];
            }
            "varying" => {
                let decl = rest["varying".len()..].trim().trim_end_matches(';').trim();
                return Some((qualifiers.join(" "), decl.to_string()));
            }
            _ => return None,
        }
    }
}

/// True if the opaque-uniform declaration declares a sampler or image.
fn is_opaque_type(decl: &str) -> bool {
    let Some((name, declaration)) = split_declaration(decl).into_iter().next() else {
        return false;
    };
    declaration
        .split_whitespace()
        .take_while(|token| token.split('[').next().unwrap_or("") != name)
        .any(|token| {
            matches!(
                token,
                "atomic_uint" | "subpassInput" | "isubpassInput" | "usubpassInput"
            ) || token.starts_with("sampler")
                || token.starts_with("isampler")
                || token.starts_with("usampler")
                || token.starts_with("image")
                || token.starts_with("iimage")
                || token.starts_with("uimage")
        })
}

fn opaque_binding_type(decl: &str) -> ShaderBindingType {
    let Some((_, declaration)) = split_declaration(decl).into_iter().next() else {
        return ShaderBindingType::CombinedImageSampler;
    };
    if declaration.split_whitespace().any(|token| {
        let token = token.split('[').next().unwrap_or("");
        token.starts_with("image") || token.starts_with("iimage") || token.starts_with("uimage")
    }) {
        ShaderBindingType::StorageTexture
    } else {
        ShaderBindingType::CombinedImageSampler
    }
}

/// Transform legacy OptiFine GLSL → Vulkan GLSL 450.
///
/// `stage_kind` is one of `vertex`, `fragment`, `geometry`, `tess_control`,
/// `tess_evaluation`, `compute`.
pub fn patch_optifine_glsl(source: &str, stage_kind: &str) -> PatchedGlsl {
    patch_optifine_glsl_seeded(source, stage_kind, &PatchSeed::default())
}

/// [`patch_optifine_glsl`] with a cross-stage interface seed; see [`PatchSeed`].
pub fn patch_optifine_glsl_seeded(source: &str, stage_kind: &str, seed: &PatchSeed) -> PatchedGlsl {
    let is_vertex = stage_kind == "vertex";
    let is_fragment = stage_kind == "fragment";

    // Comments that hide `#define` from OptiFine option scanning must not
    // resurface as directives after other replacements.
    let source = source.replace("//#define", "//_define");
    let uses_fog = source.contains("gl_Fog.");
    let source = replace_legacy_tokens(&source, seed.fullscreen_pass);

    let mut body = String::with_capacity(source.len());
    // First-seen order preserved; keyed by variable name for dedup. Seeded
    // entries come first so the shared interface prefix stays identical
    // across the stages of one program.
    let mut ubo_fields: Vec<String> = Vec::new();
    let mut ubo_names: BTreeMap<String, String> = BTreeMap::new();
    for seed_field in &seed.ubo_fields {
        let split = split_declaration(seed_field);
        if split.is_empty() {
            let name = decl_name(seed_field);
            if !name.is_empty() && !ubo_names.contains_key(&name) {
                ubo_names.insert(name, seed_field.clone());
                ubo_fields.push(seed_field.clone());
            }
        } else {
            for (name, field) in split {
                if !ubo_names.contains_key(&name) {
                    ubo_names.insert(name, field.clone());
                    ubo_fields.push(field);
                }
            }
        }
    }
    let mut sampler_bindings: Vec<(String, u32, ShaderBindingType)> = seed.sampler_bindings.clone();
    let mut attributes: Vec<(String, String)> = Vec::new(); // (name, decl)
    let mut varyings: Vec<(String, String, String)> = Vec::new(); // (name, quals, decl)
    let mut opaque_renames: BTreeMap<String, String> = BTreeMap::new();
    let mut in_block_comment = false;

    for line in source.lines() {
        let started_in_comment = in_block_comment;
        // Track /* ... */ state so declarations inside block comments are
        // not hoisted. GLSL has no string literals, so a plain scan works.
        {
            let mut rest = line;
            loop {
                if in_block_comment {
                    match rest.find("*/") {
                        Some(pos) => {
                            in_block_comment = false;
                            rest = &rest[pos + 2..];
                        }
                        None => break,
                    }
                } else {
                    let line_comment = rest.find("//");
                    match rest.find("/*") {
                        Some(pos) if line_comment.is_none_or(|lc| pos < lc) => {
                            in_block_comment = true;
                            rest = &rest[pos + 2..];
                        }
                        _ => break,
                    }
                }
            }
        }
        if started_in_comment {
            body.push_str(line);
            body.push('\n');
            continue;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("//") || trimmed.starts_with("/*") {
            body.push_str(line);
            body.push('\n');
            continue;
        }
        if trimmed.starts_with("#version ") || trimmed.starts_with("#extension ") {
            continue;
        }

        // Opaque uniforms: rebind in place. Non-opaque: hoist into the UBO.
        if let Some((uniform_qualifiers, rest)) = split_qualified_uniform(trimmed) {
            let decl = rest.trim_end_matches(';').trim();
            if is_opaque_type(decl) {
                let declarations = split_declaration(decl);
                for (name, declaration) in declarations {
                    let binding = match sampler_bindings.iter().find(|(n, _, _)| *n == name) {
                        Some((_, binding, _)) => *binding,
                        None => {
                            // Binding 0 is the FrameUniforms UBO.
                            let binding = sampler_bindings.len() as u32 + 1;
                            sampler_bindings.push((
                                name.clone(),
                                binding,
                                opaque_binding_type(decl),
                            ));
                            binding
                        }
                    };
                    let shader_name = if name == "texture" {
                        let renamed = "rc_sampler_texture".to_string();
                        opaque_renames.insert(name.clone(), renamed.clone());
                        renamed
                    } else {
                        name.clone()
                    };
                    let declaration = if shader_name == name {
                        declaration
                    } else {
                        rename_identifier_references(&declaration, &name, &shader_name)
                    };
                    let uniform_qualifiers = if uniform_qualifiers.is_empty() {
                        String::new()
                    } else {
                        format!("{uniform_qualifiers} ")
                    };
                    body.push_str(&format!(
                        "layout(set = 0, binding = {binding}) {uniform_qualifiers}uniform {declaration};\n"
                    ));
                }
            } else {
                for (name, field) in split_declaration(decl) {
                    match ubo_names.get(&name) {
                        Some(existing) if *existing != field => log::warn!(
                            "shader pack redeclares uniform '{name}' as '{field}' (keeping '{existing}')"
                        ),
                        Some(_) => {}
                        None => {
                            ubo_names.insert(name, field.clone());
                            ubo_fields.push(field);
                        }
                    }
                }
            }
            continue;
        }

        // Vertex attributes: hoist (dedup by name) for vertex stages,
        // drop everywhere else.
        if let Some(rest) = trimmed.strip_prefix("attribute ") {
            if is_vertex {
                for (name, decl) in split_declaration(rest) {
                    if !attributes.iter().any(|(n, _)| *n == name) {
                        attributes.push((name, decl));
                    }
                }
            }
            continue;
        }

        // Varyings: hoist with qualifiers, dedup by name.
        if let Some((quals, decl)) = split_varying(trimmed) {
            for (name, declaration) in split_declaration(&decl) {
                if !varyings.iter().any(|(n, _, _)| *n == name) {
                    varyings.push((name, quals.clone(), declaration));
                }
            }
            continue;
        }

        body.push_str(line);
        body.push('\n');
    }

    for (old, new) in opaque_renames {
        body = rename_identifier_references(&body, &old, &new);
    }

    // Inject uniforms that replacements introduced (gbuffer matrices, fog)
    // or that Iris 1.17+ sources reference natively (modelViewMatrix, ...).
    for (name, ty, fog_gated) in INJECTED_UNIFORMS {
        if *fog_gated && !uses_fog {
            continue;
        }
        if !ubo_names.contains_key(*name) && contains_token(&body, name) {
            let field = format!("{ty} {name}");
            ubo_names.insert((*name).to_string(), field.clone());
            ubo_fields.push(field);
        }
    }

    // ---- Preamble assembly -------------------------------------------------
    let mut preamble = String::with_capacity(4096);
    preamble.push_str("#version 450\n");
    preamble.push_str("#extension GL_ARB_separate_shader_objects : enable\n");
    preamble.push_str("#extension GL_ARB_shading_language_420pack : enable\n\n");

    // Fragment outputs for gl_FragColor / gl_FragData[N] replacements.
    if is_fragment {
        for i in 0..MAX_FRAG_DATA {
            let name = format!("fragData{i}");
            if contains_token(&body, &name) {
                preamble.push_str(&format!("layout(location = {i}) out vec4 {name};\n"));
            }
        }
    }

    // Vertex inputs: fixed locations for builtin replacements, then custom
    // pack attributes.
    let mut attribute_count = 0usize;
    if is_vertex {
        for (name, ty, location) in BUILTIN_ATTRIBUTES {
            if contains_token(&body, name) {
                preamble.push_str(&format!("layout(location = {location}) in {ty} {name};\n"));
                attribute_count += 1;
            }
        }
        let mut location = CUSTOM_ATTRIBUTE_BASE_LOCATION;
        for (_, decl) in &attributes {
            preamble.push_str(&format!("layout(location = {location}) in {decl};\n"));
            location += location_slots(decl);
            attribute_count += 1;
        }
    }

    // Hoisted varyings. Locations are assigned in sorted-name order so both
    // stages of a program agree whenever they declare the same set.
    if !varyings.is_empty() {
        let direction = if is_vertex {
            Some("out")
        } else if is_fragment {
            Some("in")
        } else {
            None
        };
        let mut sorted = varyings.clone();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        let mut location = 0u32;
        for (_, quals, decl) in &sorted {
            let quals = if quals.is_empty() {
                String::new()
            } else {
                format!("{quals} ")
            };
            match direction {
                Some(direction) => preamble.push_str(&format!(
                    "layout(location = {location}) {quals}{direction} {decl};\n"
                )),
                // Geometry/tessellation/compute: declare as plain globals so
                // references still resolve.
                None => preamble.push_str(&format!("{decl};\n")),
            }
            location += location_slots(decl);
        }
    }

    // FrameUniforms UBO with every hoisted non-opaque uniform.
    if !ubo_fields.is_empty() {
        preamble.push_str("layout(set = 0, binding = 0) uniform FrameUniforms {\n");
        for field in &ubo_fields {
            preamble.push_str(&format!("    {field};\n"));
        }
        preamble.push_str("};\n\n");
    }

    // Compatibility helpers.
    if contains_token(&body, "rc_fullscreenOrtho") {
        // Column-major ortho mapping [0,1]² → NDC, replacing the
        // fixed-function ortho projection OptiFine sets for composite quads.
        preamble.push_str(
            "const mat4 rc_fullscreenOrtho = mat4(\n    2.0, 0.0, 0.0, 0.0,\n    0.0, 2.0, 0.0, 0.0,\n    0.0, 0.0, 1.0, 0.0,\n    -1.0, -1.0, 0.0, 1.0);\n",
        );
    }
    if contains_token(&body, "rc_shadow2D") {
        preamble.push_str(concat!(
            "vec4 rc_shadow2D(sampler2DShadow s, vec3 p) { return vec4(texture(s, p)); }\n",
            "vec4 rc_shadow2D(sampler2D s, vec3 p) { return vec4(step(p.z, texture(s, p.xy).r)); }\n",
        ));
    }
    if contains_token(&body, "rc_shadow2DLod") {
        preamble.push_str(concat!(
            "vec4 rc_shadow2DLod(sampler2DShadow s, vec3 p, float lod) { return vec4(textureLod(s, p, lod)); }\n",
            "vec4 rc_shadow2DLod(sampler2D s, vec3 p, float lod) { return vec4(step(p.z, textureLod(s, p.xy, lod).r)); }\n",
        ));
    }
    if contains_token(&body, "rc_fogFragCoord") {
        preamble.push_str("float rc_fogFragCoord = 0.0;\n");
    }
    preamble.push('\n');

    let mut final_source = preamble;
    final_source.push_str(&body);

    PatchedGlsl {
        source: final_source,
        uniform_ubo_fields: ubo_fields,
        sampler_count: sampler_bindings.len(),
        sampler_bindings,
        attribute_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_vertex_builtins_are_wrapped_and_declared() {
        let source = "#version 120\nvoid main() {\n    gl_Position = ftransform();\n    vec2 uv = (gl_TextureMatrix[0] * gl_MultiTexCoord0).xy;\n}\n";
        let patched = patch_optifine_glsl(source, "vertex");
        assert!(patched
            .source
            .contains("layout(location = 0) in vec3 vaPosition;"));
        assert!(patched
            .source
            .contains("layout(location = 3) in vec2 vaUV0;"));
        assert!(patched
            .source
            .contains("gbufferProjection * gbufferModelView * vec4(vaPosition, 1.0)"));
        assert!(patched.source.contains("mat4(1.0) * vec4(vaUV0, 0.0, 1.0)"));
        // Replacements pull the gbuffer matrices into the UBO.
        assert!(patched
            .uniform_ubo_fields
            .contains(&"mat4 gbufferModelView".to_string()));
    }

    #[test]
    fn vertex_index_survives_gl_vertex_replacement() {
        let patched = patch_optifine_glsl(
            "#version 130\nvoid main() { int i = gl_VertexID; vec4 p = gl_Vertex; }\n",
            "vertex",
        );
        assert!(patched.source.contains("gl_VertexIndex"));
        assert!(!patched.source.contains("vec4(vaPosition, 1.0)Index"));
    }

    #[test]
    fn samplers_keep_one_binding_per_name_across_branches() {
        let source = "#version 130\n#ifdef A\nuniform sampler2D shadowtex0;\n#else\nuniform sampler2DShadow shadowtex0;\n#endif\nuniform sampler2D tex;\nvoid main() {}\n";
        let patched = patch_optifine_glsl(source, "fragment");
        assert_eq!(
            patched.sampler_bindings,
            vec![
                (
                    "shadowtex0".to_string(),
                    1,
                    ShaderBindingType::CombinedImageSampler
                ),
                (
                    "tex".to_string(),
                    2,
                    ShaderBindingType::CombinedImageSampler
                )
            ]
        );
        assert!(patched
            .source
            .contains("layout(set = 0, binding = 1) uniform sampler2D shadowtex0;"));
        assert!(patched
            .source
            .contains("layout(set = 0, binding = 1) uniform sampler2DShadow shadowtex0;"));
    }

    #[test]
    fn duplicate_uniforms_are_deduplicated_and_initializers_stripped() {
        let source = "#version 130\nuniform float rainStrength;\nuniform bool heavyFog = false;\nuniform float rainStrength;\nvoid main() {}\n";
        let patched = patch_optifine_glsl(source, "fragment");
        assert_eq!(
            patched.uniform_ubo_fields,
            vec![
                "float rainStrength".to_string(),
                "bool heavyFog".to_string()
            ]
        );
    }

    #[test]
    fn comma_separated_uniforms_become_distinct_ubo_members() {
        let source = "#version 120\nuniform float near, far;\nuniform float timeAngle, timeBrightness;\nuniform float timeAngle;\nuniform mat4 gbufferModelView, gbufferModelViewInverse;\nvoid main() { gl_Position = gbufferModelView * vec4(1.0); }\n";
        let patched = patch_optifine_glsl(source, "vertex");
        assert_eq!(
            patched.uniform_ubo_fields,
            vec![
                "float near".to_string(),
                "float far".to_string(),
                "float timeAngle".to_string(),
                "float timeBrightness".to_string(),
                "mat4 gbufferModelView".to_string(),
                "mat4 gbufferModelViewInverse".to_string(),
            ]
        );
        assert_eq!(patched.source.matches("float timeAngle;").count(), 1);
        assert_eq!(patched.source.matches("mat4 gbufferModelView;").count(), 1);
    }

    #[test]
    fn frag_data_outputs_declared_only_when_referenced() {
        let source =
            "#version 130\nvoid main() { gl_FragData[0] = vec4(1.0); gl_FragData[7] = vec4(0.0); }\n";
        let patched = patch_optifine_glsl(source, "fragment");
        assert!(patched
            .source
            .contains("layout(location = 0) out vec4 fragData0;"));
        assert!(patched
            .source
            .contains("layout(location = 7) out vec4 fragData7;"));
        assert!(!patched.source.contains("out vec4 fragData3;"));
    }

    #[test]
    fn varyings_are_hoisted_with_qualifiers_and_direction() {
        let source =
            "#version 120\nflat varying vec3 sunVec;\nvarying vec2 texCoord;\nvoid main() {}\n";
        let vertex = patch_optifine_glsl(source, "vertex");
        assert!(vertex
            .source
            .contains("layout(location = 0) flat out vec3 sunVec;"));
        assert!(vertex
            .source
            .contains("layout(location = 1) out vec2 texCoord;"));
        let fragment = patch_optifine_glsl(source, "fragment");
        assert!(fragment
            .source
            .contains("layout(location = 0) flat in vec3 sunVec;"));
    }

    #[test]
    fn comma_separated_and_reordered_varyings_have_unique_locations() {
        let source = "#version 120\nvarying vec2 texCoord, lmCoord;\nvarying vec3 sunVec, upVec, eastVec;\nvarying vec3 upVec, sunVec;\nvoid main() {}\n";
        let patched = patch_optifine_glsl(source, "vertex");
        for (location, name) in [
            (0, "eastVec"),
            (1, "lmCoord"),
            (2, "sunVec"),
            (3, "texCoord"),
            (4, "upVec"),
        ] {
            assert!(patched
                .source
                .contains(&format!("layout(location = {location}) out vec")));
            assert_eq!(patched.source.matches(&format!(" {name};")).count(), 1);
        }
    }

    #[test]
    fn declarations_inside_block_comments_are_ignored() {
        let source =
            "#version 130\n/*\nuniform float ghost;\n*/\nuniform float real;\nvoid main() {}\n";
        let patched = patch_optifine_glsl(source, "fragment");
        assert_eq!(patched.uniform_ubo_fields, vec!["float real".to_string()]);
    }

    #[test]
    fn custom_attributes_start_after_builtin_locations() {
        let source = "#version 120\nattribute vec4 mc_Entity;\nattribute vec4 at_tangent;\nattribute vec4 mc_Entity;\nvoid main() { gl_Position = gl_Vertex; }\n";
        let patched = patch_optifine_glsl(source, "vertex");
        assert!(patched
            .source
            .contains("layout(location = 8) in vec4 mc_Entity;"));
        assert!(patched
            .source
            .contains("layout(location = 9) in vec4 at_tangent;"));
        assert_eq!(patched.attribute_count, 3); // vaPosition + 2 custom
    }

    #[test]
    fn comma_separated_attributes_each_consume_a_location() {
        let source = "#version 120\nattribute vec4 mc_Entity, at_tangent;\nattribute mat2 custom_matrix;\nvoid main() {}\n";
        let patched = patch_optifine_glsl(source, "vertex");
        assert!(patched
            .source
            .contains("layout(location = 8) in vec4 mc_Entity;"));
        assert!(patched
            .source
            .contains("layout(location = 9) in vec4 at_tangent;"));
        assert!(patched
            .source
            .contains("layout(location = 10) in mat2 custom_matrix;"));
    }

    #[test]
    fn texture_named_sampler_does_not_shadow_core_texture_function() {
        let source = "#version 120\nuniform sampler2D texture;\nvarying vec2 texCoord;\nvoid main() { gl_FragColor = texture2D(texture, texCoord); }\n";
        let patched = patch_optifine_glsl(source, "fragment");
        assert_eq!(
            patched.sampler_bindings,
            vec![(
                "texture".to_string(),
                1,
                ShaderBindingType::CombinedImageSampler
            )]
        );
        assert!(patched
            .source
            .contains("uniform sampler2D rc_sampler_texture;"));
        assert!(patched
            .source
            .contains("texture(rc_sampler_texture, texCoord)"));
        assert!(!patched.source.contains("texture(texture,"));
    }

    #[test]
    fn qualified_uniforms_are_rebound_and_precision_is_removed() {
        let source = "#version 430\nwriteonly uniform uimage3D voxelimg;\ncoherent volatile restrict readonly uniform image3D lightimg;\nhighp uniform sampler2D texture;\nmediump uniform float exposure;\nvoid main() { vec4 value = texture2D ( texture, vec2(0.5)); }\n";
        let patched = patch_optifine_glsl(source, "fragment");
        assert_eq!(
            patched.sampler_bindings,
            vec![
                ("voxelimg".to_string(), 1, ShaderBindingType::StorageTexture),
                ("lightimg".to_string(), 2, ShaderBindingType::StorageTexture),
                (
                    "texture".to_string(),
                    3,
                    ShaderBindingType::CombinedImageSampler
                ),
            ]
        );
        assert!(patched
            .source
            .contains("layout(set = 0, binding = 1) writeonly uniform uimage3D voxelimg;"));
        assert!(patched.source.contains(
            "layout(set = 0, binding = 2) coherent volatile restrict readonly uniform image3D lightimg;"
        ));
        assert!(patched
            .source
            .contains("layout(set = 0, binding = 3) uniform sampler2D rc_sampler_texture;"));
        assert_eq!(
            patched.uniform_ubo_fields,
            vec!["float exposure".to_string()]
        );
        assert!(patched
            .source
            .contains("texture ( rc_sampler_texture, vec2(0.5))"));
    }

    #[test]
    fn opaque_detection_uses_the_type_not_the_variable_name() {
        let source = "#version 120\nuniform int imageCount;\nuniform vec2 samplerOffset;\nuniform sampler2D actualSampler;\nvoid main() {}\n";
        let patched = patch_optifine_glsl(source, "fragment");
        assert_eq!(
            patched.uniform_ubo_fields,
            vec![
                "int imageCount".to_string(),
                "vec2 samplerOffset".to_string()
            ]
        );
        assert_eq!(
            patched.sampler_bindings,
            vec![(
                "actualSampler".to_string(),
                1,
                ShaderBindingType::CombinedImageSampler
            )]
        );
    }

    #[test]
    fn texture_aliasing_respects_calls_members_comments_and_whitespace() {
        let source = "#version 120\nuniform sampler2D texture;\nvoid main() {\n vec4 a = texture2D /* legacy */ (texture, vec2(0.0));\n vec4 b = texture /* core */ (texture, vec2(1.0));\n vec4 c = material.texture;\n // texture2D(texture, vec2(0.5))\n}\n";
        let patched = patch_optifine_glsl(source, "fragment");
        assert!(patched
            .source
            .contains("texture /* legacy */ (rc_sampler_texture, vec2(0.0))"));
        assert!(patched
            .source
            .contains("texture /* core */ (rc_sampler_texture, vec2(1.0))"));
        assert!(patched.source.contains("material.texture"));
        assert!(patched.source.contains("// texture2D(texture, vec2(0.5))"));
    }

    #[test]
    fn include_patch_preserves_memory_qualifiers_before_uniform() {
        let patched = patch_include_glsl(
            "writeonly uniform highp image3D target;\nhighp uniform float exposure = 1.0;\n",
        );
        assert!(patched.contains("writeonly uniform image3D target;"));
        assert!(patched.contains("uniform float exposure;"));
    }

    #[test]
    fn shadow2d_helper_injected_when_used() {
        let source = "#version 120\nuniform sampler2DShadow shadowtex0;\nvoid main() { float s = shadow2D(shadowtex0, vec3(0.5)).x; }\n";
        let patched = patch_optifine_glsl(source, "fragment");
        assert!(patched.source.contains("rc_shadow2D(shadowtex0"));
        assert!(patched
            .source
            .contains("vec4 rc_shadow2D(sampler2DShadow s, vec3 p)"));
    }
}
