//! Builder API and implementation for `buffa-reflect-build`.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
};

use buffa::Message as _;
use buffa_descriptor::generated::descriptor::{DescriptorProto, FileDescriptorSet};

/// Default name for the descriptor-set artifact written into `OUT_DIR`.
const DEFAULT_FDS_NAME: &str = "file_descriptor_set.bin";

/// How to obtain a `FileDescriptorSet` to feed both buffa-build and the
/// reflection layer.
#[derive(Debug, Clone, Default)]
enum DescriptorSource {
    /// Run `protoc --include_imports --include_source_info` ourselves and
    /// hand the resulting binary to buffa-build via `descriptor_set`.
    #[default]
    Protoc,
    /// Run `buf build --as-file-descriptor-set -o <path>`.
    Buf,
    /// Re-use a pre-built `FileDescriptorSet` binary.
    Precompiled(PathBuf),
}

/// Builder for compiling `.proto` files with reflection metadata enabled.
///
/// Driving the build script:
///
/// ```rust,ignore
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     buffa_reflect_build::Builder::new()
///         .file_descriptor_set_bytes("crate::FILE_DESCRIPTOR_SET_BYTES")
///         .files(&["proto/acme/api/v1/user.proto"])
///         .includes(&["proto/"])
///         .compile()?;
///     Ok(())
/// }
/// ```
///
/// The downstream library then ships:
///
/// ```rust,ignore
/// pub const FILE_DESCRIPTOR_SET_BYTES: &[u8] =
///     include_bytes!(concat!(env!("OUT_DIR"), "/file_descriptor_set.bin"));
///
/// buffa::include_proto!("acme.api.v1");
/// ```
///
/// Every generated message is decorated with
/// `#[derive(::buffa_reflect::ReflectMessage)]` so that
/// `m.descriptor()` resolves the right [`MessageDescriptor`](buffa_reflect::MessageDescriptor)
/// at runtime.
#[derive(Debug, Default)]
pub struct Builder {
    descriptor_source: DescriptorSource,
    files: Vec<PathBuf>,
    includes: Vec<PathBuf>,
    out_dir: Option<PathBuf>,
    file_descriptor_set_path: Option<PathBuf>,
    descriptor_pool_expr: Option<String>,
    file_descriptor_set_bytes_expr: Option<String>,
    include_file: Option<String>,

    // user-supplied codegen passthroughs
    type_attributes: Vec<(String, String)>,
    field_attributes: Vec<(String, String)>,
    message_attributes: Vec<(String, String)>,
    enum_attributes: Vec<(String, String)>,
    extern_paths: Vec<(String, String)>,
    bytes_paths: Vec<String>,
    generate_views: Option<bool>,
    generate_json: Option<bool>,
    generate_text: Option<bool>,
    generate_arbitrary: Option<bool>,
    preserve_unknown_fields: Option<bool>,
    strict_utf8_mapping: Option<bool>,
    allow_message_set: Option<bool>,
}

/// Errors raised by [`Builder::compile`].
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Neither [`Builder::descriptor_pool`] nor
    /// [`Builder::file_descriptor_set_bytes`] was supplied.
    #[error(
        "buffa-reflect-build: descriptor binding not configured — call `.descriptor_pool(..)` or \
         `.file_descriptor_set_bytes(..)`"
    )]
    MissingDescriptorBinding,

    /// Both bindings were supplied; the macro accepts at most one.
    #[error(
        "buffa-reflect-build: cannot set both `descriptor_pool` and `file_descriptor_set_bytes` — \
         pick one"
    )]
    ConflictingDescriptorBindings,

    /// `OUT_DIR` was not set by cargo and no explicit
    /// [`Builder::out_dir`] was given.
    #[error(
        "buffa-reflect-build: OUT_DIR is not set and no out_dir() was configured (run from \
         build.rs or call .out_dir())"
    )]
    MissingOutDir,

    /// Spawning the descriptor-producing tool (`protoc`/`buf`) failed.
    #[error("buffa-reflect-build: failed to invoke {tool}: {source}")]
    DescriptorTool {
        /// The tool that failed to launch.
        tool: &'static str,
        /// Source `io::Error`.
        #[source]
        source: std::io::Error,
    },

    /// The descriptor-producing tool returned a non-zero exit status.
    #[error("buffa-reflect-build: {tool} exited with status {status}: {stderr}")]
    DescriptorToolFailed {
        /// The tool that failed.
        tool: &'static str,
        /// Exit status returned by the tool.
        status: std::process::ExitStatus,
        /// Captured stderr.
        stderr: String,
    },

    /// The descriptor-set bytes could not be parsed.
    #[error("buffa-reflect-build: failed to decode FileDescriptorSet: {0}")]
    DecodeFileDescriptorSet(#[source] buffa::DecodeError),

    /// `Buf` mode resolves imports through `buf.yaml`/`buf.work.yaml` and
    /// has no use for `protoc`-style `.includes(..)`. Configuring both is
    /// almost always a bug, so we error rather than silently dropping the
    /// include list.
    #[error(
        "buffa-reflect-build: .includes(..) is not supported in `use_buf` mode (buf reads import \
         paths from buf.yaml); drop the includes() call or switch to the protoc source"
    )]
    BufWithIncludes,

    /// `buffa-build` reported an error.
    ///
    /// `buffa-build`'s `compile()` returns a `Box<dyn Error>` (without
    /// `Send + Sync`), so we capture its rendered message and surface it
    /// here as a string. The original error chain is unfortunately lost on
    /// the `buffa-build` side; if more structured failure data is needed
    /// we can lift it from a future `buffa-build` release.
    #[error("buffa-reflect-build: buffa-build error: {0}")]
    BuffaBuild(String),

    /// I/O error while reading or writing artifacts.
    #[error("buffa-reflect-build: io error: {0}")]
    Io(#[from] std::io::Error),
}

impl Builder {
    /// Construct a new builder with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Use a user-managed `DescriptorPool` for runtime descriptor lookup.
    /// The argument is a Rust expression yielding
    /// `&buffa_reflect::DescriptorPool` (e.g. `"crate::DESCRIPTOR_POOL"`).
    #[must_use]
    pub fn descriptor_pool(mut self, expr: impl Into<String>) -> Self {
        self.descriptor_pool_expr = Some(expr.into());
        self
    }

    /// Embed the descriptor-set bytes directly. The argument is a Rust
    /// expression yielding `&[u8]`, typically
    /// `"crate::FILE_DESCRIPTOR_SET_BYTES"` paired with
    /// `include_bytes!(concat!(env!("OUT_DIR"), "/file_descriptor_set.bin"))`.
    #[must_use]
    pub fn file_descriptor_set_bytes(mut self, expr: impl Into<String>) -> Self {
        self.file_descriptor_set_bytes_expr = Some(expr.into());
        self
    }

    /// Override where the descriptor-set artifact is written. Defaults to
    /// `<OUT_DIR>/file_descriptor_set.bin`.
    #[must_use]
    pub fn file_descriptor_set_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.file_descriptor_set_path = Some(path.into());
        self
    }

    /// `.proto` files to compile. Same semantics as
    /// [`buffa_build::Config::files`].
    #[must_use]
    pub fn files(mut self, files: &[impl AsRef<Path>]) -> Self {
        self.files
            .extend(files.iter().map(|f| f.as_ref().to_path_buf()));
        self
    }

    /// Include directories searched by protoc.
    #[must_use]
    pub fn includes(mut self, includes: &[impl AsRef<Path>]) -> Self {
        self.includes
            .extend(includes.iter().map(|i| i.as_ref().to_path_buf()));
        self
    }

    /// Use `buf build --as-file-descriptor-set` instead of `protoc`.
    #[must_use]
    pub fn use_buf(mut self) -> Self {
        self.descriptor_source = DescriptorSource::Buf;
        self
    }

    /// Use a precompiled descriptor-set file. Skips invoking `protoc`/`buf`.
    /// The file must contain a serialized
    /// `google.protobuf.FileDescriptorSet`.
    #[must_use]
    pub fn descriptor_set(mut self, path: impl Into<PathBuf>) -> Self {
        self.descriptor_source = DescriptorSource::Precompiled(path.into());
        self
    }

    /// Override the output directory (defaults to `$OUT_DIR`).
    #[must_use]
    pub fn out_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.out_dir = Some(dir.into());
        self
    }

    /// Pass a `type_attribute` through to buffa-build.
    #[must_use]
    pub fn type_attribute(mut self, path: impl Into<String>, attr: impl Into<String>) -> Self {
        self.type_attributes.push((path.into(), attr.into()));
        self
    }

    /// Pass a `field_attribute` through to buffa-build.
    #[must_use]
    pub fn field_attribute(mut self, path: impl Into<String>, attr: impl Into<String>) -> Self {
        self.field_attributes.push((path.into(), attr.into()));
        self
    }

    /// Pass a `message_attribute` through to buffa-build (struct-only).
    #[must_use]
    pub fn message_attribute(mut self, path: impl Into<String>, attr: impl Into<String>) -> Self {
        self.message_attributes.push((path.into(), attr.into()));
        self
    }

    /// Pass an `enum_attribute` through to buffa-build (enum-only).
    #[must_use]
    pub fn enum_attribute(mut self, path: impl Into<String>, attr: impl Into<String>) -> Self {
        self.enum_attributes.push((path.into(), attr.into()));
        self
    }

    /// Map a proto path prefix to an external Rust module. Forwarded to
    /// [`buffa_build::Config::extern_path`].
    #[must_use]
    pub fn extern_path(
        mut self,
        proto_path: impl Into<String>,
        rust_path: impl Into<String>,
    ) -> Self {
        self.extern_paths
            .push((proto_path.into(), rust_path.into()));
        self
    }

    /// Mark `bytes` fields under the given proto-path prefixes as
    /// `bytes::Bytes`-typed.
    #[must_use]
    pub fn use_bytes_type_in(mut self, paths: &[impl AsRef<str>]) -> Self {
        self.bytes_paths
            .extend(paths.iter().map(|p| p.as_ref().to_string()));
        self
    }

    /// Toggle generation of borrowed view types. Defaults to buffa-build's
    /// own default (currently `true`).
    #[must_use]
    pub fn generate_views(mut self, enabled: bool) -> Self {
        self.generate_views = Some(enabled);
        self
    }

    /// Toggle proto3 JSON serde derives.
    #[must_use]
    pub fn generate_json(mut self, enabled: bool) -> Self {
        self.generate_json = Some(enabled);
        self
    }

    /// Toggle textproto support.
    #[must_use]
    pub fn generate_text(mut self, enabled: bool) -> Self {
        self.generate_text = Some(enabled);
        self
    }

    /// Toggle `arbitrary::Arbitrary` derives.
    #[must_use]
    pub fn generate_arbitrary(mut self, enabled: bool) -> Self {
        self.generate_arbitrary = Some(enabled);
        self
    }

    /// Toggle unknown-field preservation.
    #[must_use]
    pub fn preserve_unknown_fields(mut self, enabled: bool) -> Self {
        self.preserve_unknown_fields = Some(enabled);
        self
    }

    /// Honor `features.utf8_validation = NONE` by mapping such strings to
    /// bytes — see [`buffa_build::Config::strict_utf8_mapping`].
    #[must_use]
    pub fn strict_utf8_mapping(mut self, enabled: bool) -> Self {
        self.strict_utf8_mapping = Some(enabled);
        self
    }

    /// Permit `message_set_wire_format = true` on input messages.
    #[must_use]
    pub fn allow_message_set(mut self, enabled: bool) -> Self {
        self.allow_message_set = Some(enabled);
        self
    }

    /// Emit a per-package include-file alongside the per-proto outputs.
    #[must_use]
    pub fn include_file(mut self, name: impl Into<String>) -> Self {
        self.include_file = Some(name.into());
        self
    }

    /// Compile the configured `.proto` files.
    ///
    /// # Errors
    ///
    /// See [`Error`] for the failure modes — most user errors are an
    /// unconfigured descriptor binding, a missing `OUT_DIR`, or
    /// `protoc`/`buf` invocation failure.
    pub fn compile(self) -> Result<(), Error> {
        match (
            self.descriptor_pool_expr.is_some(),
            self.file_descriptor_set_bytes_expr.is_some(),
        ) {
            (false, false) => return Err(Error::MissingDescriptorBinding),
            (true, true) => return Err(Error::ConflictingDescriptorBindings),
            _ => {}
        }

        if matches!(self.descriptor_source, DescriptorSource::Buf) && !self.includes.is_empty() {
            return Err(Error::BufWithIncludes);
        }

        let out_dir = self
            .out_dir
            .clone()
            .or_else(|| std::env::var_os("OUT_DIR").map(PathBuf::from))
            .ok_or(Error::MissingOutDir)?;
        std::fs::create_dir_all(&out_dir)?;

        let fds_path = self
            .file_descriptor_set_path
            .clone()
            .unwrap_or_else(|| out_dir.join(DEFAULT_FDS_NAME));
        if let Some(parent) = fds_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        produce_descriptor_set(
            &self.descriptor_source,
            &self.files,
            &self.includes,
            &fds_path,
        )?;

        let fds_bytes = std::fs::read(&fds_path)?;
        let fds = FileDescriptorSet::decode_from_slice(&fds_bytes)
            .map_err(Error::DecodeFileDescriptorSet)?;

        // Emit reflection-attribute decorations.
        let mut cfg = buffa_build::Config::new();
        cfg = self.apply_reflection_attributes(cfg, &fds);
        cfg = self.apply_user_passthroughs(cfg);

        // The `Builder::compile` algorithm always feeds buffa-build via the
        // precompiled descriptor set we just wrote, regardless of how it
        // was obtained — that's the seam that lets us own the FDS bytes.
        cfg = cfg.descriptor_set(&fds_path);
        cfg = cfg.files(&proto_relative_files(
            &self.descriptor_source,
            &self.files,
            &self.includes,
            &fds,
        ));
        cfg = cfg.out_dir(&out_dir);
        if let Some(name) = &self.include_file {
            cfg = cfg.include_file(name);
        }

        cfg.compile()
            .map_err(|e| Error::BuffaBuild(e.to_string()))?;

        // Cargo dependency tracking. buffa-build's own emit only fires when
        // its descriptor source matches the cargo trigger style; since we
        // always feed it Precompiled, we re-emit the right ones ourselves.
        emit_cargo_directives(&self.descriptor_source, &self.files, &fds_path);

        Ok(())
    }

    fn apply_reflection_attributes(
        &self,
        mut cfg: buffa_build::Config,
        fds: &FileDescriptorSet,
    ) -> buffa_build::Config {
        let binding_attr = if let Some(expr) = &self.descriptor_pool_expr {
            format!(r#"#[buffa_reflect(descriptor_pool = "{expr}")]"#)
        } else if let Some(expr) = &self.file_descriptor_set_bytes_expr {
            format!(r#"#[buffa_reflect(file_descriptor_set_bytes = "{expr}")]"#)
        } else {
            unreachable!("descriptor binding presence enforced above")
        };

        // One ReflectMessage derive + one binding attribute on every
        // generated message struct. `message_attribute(".", ...)` lands on
        // every message exactly once and avoids decorating enums.
        cfg = cfg.message_attribute(".", "#[derive(::buffa_reflect::ReflectMessage)]");
        cfg = cfg.message_attribute(".", binding_attr);

        // Per-message FQN attribute. The codegen prefix-match means nested
        // messages also pick up parent attributes; the derive picks the
        // longest message_name to disambiguate.
        for full_name in collect_message_full_names(fds) {
            cfg = cfg.message_attribute(
                format!(".{full_name}"),
                format!(r#"#[buffa_reflect(message_name = "{full_name}")]"#),
            );
        }
        cfg
    }

    fn apply_user_passthroughs(&self, mut cfg: buffa_build::Config) -> buffa_build::Config {
        for (path, attr) in &self.type_attributes {
            cfg = cfg.type_attribute(path, attr);
        }
        for (path, attr) in &self.field_attributes {
            cfg = cfg.field_attribute(path, attr);
        }
        for (path, attr) in &self.message_attributes {
            cfg = cfg.message_attribute(path, attr);
        }
        for (path, attr) in &self.enum_attributes {
            cfg = cfg.enum_attribute(path, attr);
        }
        for (proto, rust) in &self.extern_paths {
            cfg = cfg.extern_path(proto.clone(), rust.clone());
        }
        if !self.bytes_paths.is_empty() {
            cfg = cfg.use_bytes_type_in(&self.bytes_paths);
        }
        if let Some(b) = self.generate_views {
            cfg = cfg.generate_views(b);
        }
        if let Some(b) = self.generate_json {
            cfg = cfg.generate_json(b);
        }
        if let Some(b) = self.generate_text {
            cfg = cfg.generate_text(b);
        }
        if let Some(b) = self.generate_arbitrary {
            cfg = cfg.generate_arbitrary(b);
        }
        if let Some(b) = self.preserve_unknown_fields {
            cfg = cfg.preserve_unknown_fields(b);
        }
        if let Some(b) = self.strict_utf8_mapping {
            cfg = cfg.strict_utf8_mapping(b);
        }
        if let Some(b) = self.allow_message_set {
            cfg = cfg.allow_message_set(b);
        }
        cfg
    }
}

fn produce_descriptor_set(
    source: &DescriptorSource,
    files: &[PathBuf],
    includes: &[PathBuf],
    fds_path: &Path,
) -> Result<(), Error> {
    match source {
        DescriptorSource::Protoc => invoke_protoc(files, includes, fds_path),
        DescriptorSource::Buf => invoke_buf(fds_path),
        DescriptorSource::Precompiled(src) => {
            std::fs::copy(src, fds_path).map(|_| ()).map_err(Error::Io)
        }
    }
}

fn invoke_protoc(files: &[PathBuf], includes: &[PathBuf], out: &Path) -> Result<(), Error> {
    let protoc: OsString = std::env::var_os("PROTOC").unwrap_or_else(|| "protoc".into());
    let mut cmd = Command::new(&protoc);
    cmd.arg("--include_imports");
    cmd.arg("--include_source_info");
    {
        let mut arg = OsString::from("--descriptor_set_out=");
        arg.push(out);
        cmd.arg(arg);
    }
    for include in includes {
        let mut arg = OsString::from("--proto_path=");
        arg.push(include);
        cmd.arg(arg);
    }
    for file in files {
        cmd.arg(file);
    }
    let output = cmd.output().map_err(|source| Error::DescriptorTool {
        tool: "protoc",
        source,
    })?;
    if !output.status.success() {
        return Err(Error::DescriptorToolFailed {
            tool: "protoc",
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(())
}

fn invoke_buf(out: &Path) -> Result<(), Error> {
    let mut cmd = Command::new("buf");
    cmd.arg("build")
        .arg("--as-file-descriptor-set")
        .arg("-o")
        .arg(out);
    let output = cmd.output().map_err(|source| Error::DescriptorTool {
        tool: "buf",
        source,
    })?;
    if !output.status.success() {
        return Err(Error::DescriptorToolFailed {
            tool: "buf",
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(())
}

fn proto_relative_files(
    source: &DescriptorSource,
    files: &[PathBuf],
    includes: &[PathBuf],
    fds: &FileDescriptorSet,
) -> Vec<String> {
    match source {
        DescriptorSource::Buf | DescriptorSource::Precompiled(_) => {
            // `.files()` already names entries; pass through.
            // For Precompiled with no `.files(...)` set, fall back to
            // every file in the FDS.
            if files.is_empty() {
                fds.file.iter().filter_map(|f| f.name.clone()).collect()
            } else {
                files
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect()
            }
        }
        DescriptorSource::Protoc => files
            .iter()
            .map(|f| proto_relative_name(f, includes))
            .filter(|s| !s.is_empty())
            .collect(),
    }
}

/// Strip the longest matching include prefix off `file`, mirroring
/// `buffa-build`'s own behavior.
fn proto_relative_name(file: &Path, includes: &[PathBuf]) -> String {
    // Longest matched prefix wins → produce the SHORTEST relative path.
    let mut best: Option<&Path> = None;
    for include in includes {
        if let Ok(rel) = file.strip_prefix(include) {
            match best {
                Some(prev) if prev.as_os_str().len() <= rel.as_os_str().len() => {}
                _ => best = Some(rel),
            }
        }
    }
    best.unwrap_or(file).to_string_lossy().into_owned()
}

/// Walk `fds` and return every message FQN, including nested messages and
/// synthetic map-entry messages (the latter just don't have a Rust struct,
/// so the attribute is harmlessly dropped).
fn collect_message_full_names(fds: &FileDescriptorSet) -> Vec<String> {
    let mut out = Vec::new();
    for file in &fds.file {
        let pkg = file.package.as_deref().unwrap_or("");
        for msg in &file.message_type {
            collect_messages(msg, pkg, &mut out);
        }
    }
    out
}

fn collect_messages(msg: &DescriptorProto, scope: &str, out: &mut Vec<String>) {
    let Some(name) = msg.name.as_deref() else {
        return;
    };
    let full_name = if scope.is_empty() {
        name.to_string()
    } else {
        format!("{scope}.{name}")
    };
    out.push(full_name.clone());
    for nested in &msg.nested_type {
        collect_messages(nested, &full_name, out);
    }
}

fn emit_cargo_directives(source: &DescriptorSource, files: &[PathBuf], fds_path: &Path) {
    println!("cargo:rerun-if-changed={}", fds_path.display());
    match source {
        DescriptorSource::Protoc => {
            println!("cargo:rerun-if-env-changed=PROTOC");
            for f in files {
                println!("cargo:rerun-if-changed={}", f.display());
            }
        }
        DescriptorSource::Buf => {
            println!("cargo:rerun-if-changed=buf.yaml");
            if Path::new("buf.lock").exists() {
                println!("cargo:rerun-if-changed=buf.lock");
            }
        }
        DescriptorSource::Precompiled(p) => {
            println!("cargo:rerun-if-changed={}", p.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use buffa_descriptor::generated::descriptor::FileDescriptorProto;

    use super::*;

    #[test]
    fn test_collect_messages_walks_nested_and_map_entries() {
        let labels_entry = DescriptorProto {
            name: Some("LabelsEntry".to_string()),
            ..Default::default()
        };
        let nested_msg = DescriptorProto {
            name: Some("Profile".to_string()),
            ..Default::default()
        };
        let user = DescriptorProto {
            name: Some("User".to_string()),
            nested_type: vec![nested_msg, labels_entry],
            ..Default::default()
        };
        let file = FileDescriptorProto {
            name: Some("acme/v1/user.proto".to_string()),
            package: Some("acme.v1".to_string()),
            message_type: vec![user],
            ..Default::default()
        };
        let fds = FileDescriptorSet {
            file: vec![file],
            ..Default::default()
        };

        let names = collect_message_full_names(&fds);
        assert_eq!(
            names,
            vec![
                "acme.v1.User".to_string(),
                "acme.v1.User.Profile".to_string(),
                "acme.v1.User.LabelsEntry".to_string(),
            ]
        );
    }

    #[test]
    fn test_proto_relative_name_strips_longest_include() {
        let got = proto_relative_name(
            Path::new("proto/vendor/ext.proto"),
            &[PathBuf::from("proto/"), PathBuf::from("proto/vendor/")],
        );
        assert_eq!(got, "ext.proto");
    }

    #[test]
    fn test_compile_errors_when_binding_missing() {
        let err = Builder::new()
            .files(&["foo.proto"])
            .out_dir(std::env::temp_dir())
            .compile()
            .unwrap_err();
        assert!(matches!(err, Error::MissingDescriptorBinding));
    }

    #[test]
    fn test_compile_errors_when_both_bindings_set() {
        let err = Builder::new()
            .descriptor_pool("crate::POOL")
            .file_descriptor_set_bytes("crate::BYTES")
            .out_dir(std::env::temp_dir())
            .compile()
            .unwrap_err();
        assert!(matches!(err, Error::ConflictingDescriptorBindings));
    }
}
