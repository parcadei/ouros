//! Python `pathlib` path types implementation.
//!
//! This module implements `pathlib.Path` plus pure path variants using a single
//! internal path object with a flavor (`POSIX` vs `Windows`) and kind
//! (`Path`, `PurePosixPath`, `PureWindowsPath`).
//!
//! `Path` supports both pure methods and filesystem methods that yield host OS
//! calls. Pure path variants only expose pure lexical path operations.

use std::{cmp::Ordering, fmt::Write};

use ahash::AHashSet;
use smallvec::SmallVec;

use crate::{
    args::{ArgValues, KwargsValues},
    exception_private::{ExcType, RunResult, SimpleException},
    heap::{Heap, HeapData, HeapGuard, HeapId},
    intern::{Interns, StaticStrings, StringId},
    os::OsFunction,
    resource::ResourceTracker,
    types::{AttrCallResult, List, PyTrait, Str, Type, allocate_tuple},
    value::{EitherStr, Value},
};

/// Path parsing flavor used for lexical operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
enum PathFlavor {
    /// POSIX-style paths using `/` and case-sensitive matching.
    Posix,
    /// Windows-style paths with drive letters, `\\` root, and case-insensitive matching.
    Windows,
}

/// Concrete class kind for a path object.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
enum PathKind {
    /// `pathlib.Path` (concrete path with filesystem methods).
    Path,
    /// `pathlib.PurePosixPath`.
    PurePosixPath,
    /// `pathlib.PureWindowsPath`.
    PureWindowsPath,
}

impl PathKind {
    /// Returns the public Python type for this kind.
    #[must_use]
    fn py_type(self) -> Type {
        match self {
            Self::Path => Type::Path,
            Self::PurePosixPath => Type::PurePosixPath,
            Self::PureWindowsPath => Type::PureWindowsPath,
        }
    }

    /// Returns the `repr()` class name for this kind.
    #[must_use]
    fn repr_type_name(self) -> &'static str {
        match self {
            Self::Path => "PosixPath",
            Self::PurePosixPath => "PurePosixPath",
            Self::PureWindowsPath => "PureWindowsPath",
        }
    }

    /// Returns the parsing flavor for this kind.
    #[must_use]
    fn flavor(self) -> PathFlavor {
        match self {
            Self::Path | Self::PurePosixPath => PathFlavor::Posix,
            Self::PureWindowsPath => PathFlavor::Windows,
        }
    }

    /// Returns whether this path kind can perform filesystem operations.
    #[must_use]
    fn allows_os_calls(self) -> bool {
        matches!(self, Self::Path)
    }
}

/// Parsed path components used by pure lexical operations.
#[derive(Debug, Clone)]
struct ParsedPath {
    /// Drive prefix (`C:` or `//server/share` on Windows, empty on POSIX).
    drive: String,
    /// Root separator (`/` when rooted, empty otherwise).
    root: String,
    /// Components after anchor.
    segments: Vec<String>,
}

impl ParsedPath {
    /// Returns the anchor (`drive + root`).
    #[must_use]
    fn anchor(&self) -> String {
        let mut out = String::with_capacity(self.drive.len() + self.root.len());
        out.push_str(&self.drive);
        out.push_str(&self.root);
        out
    }

    /// Returns whether the path has any anchor (`drive` and/or `root`).
    #[must_use]
    fn has_anchor(&self) -> bool {
        !self.drive.is_empty() || !self.root.is_empty()
    }
}

/// Python `pathlib` path object shared by `Path` and pure path classes.
///
/// The path is immutable. All mutating-style operations return a new path string
/// and are wrapped into a new `Path` instance by the caller.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Path {
    /// Normalized path string (always uses `/` internally).
    path: String,
    /// Public class kind represented by this value.
    kind: PathKind,
}

impl Path {
    /// Creates a new concrete `pathlib.Path` value.
    #[must_use]
    pub fn new(path: String) -> Self {
        Self::new_with_kind(path, PathKind::Path)
    }

    /// Creates a new path value with explicit kind.
    #[must_use]
    fn new_with_kind(path: String, kind: PathKind) -> Self {
        Self {
            path: normalize_path(path, kind.flavor()),
            kind,
        }
    }

    /// Returns a copy of this path with identical kind and a different raw string.
    #[must_use]
    fn new_like(&self, path: String) -> Self {
        Self::new_with_kind(path, self.kind)
    }

    /// Returns whether this value is a pure-path variant (`PurePosixPath`/`PureWindowsPath`).
    #[must_use]
    pub(crate) fn is_pure_path_variant(&self) -> bool {
        !matches!(self.kind, PathKind::Path)
    }

    /// Returns the normalized path string used internally.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.path
    }

    /// Returns a string suitable for `str()` / `__fspath__()`.
    #[must_use]
    pub fn display_path(&self) -> String {
        if self.kind.flavor() == PathFlavor::Windows {
            self.path.replace('/', "\\")
        } else {
            self.path.clone()
        }
    }

    /// Returns a stable hash key matching Python equality semantics.
    #[must_use]
    pub fn hash_key(&self) -> String {
        let normalized = match self.kind.flavor() {
            PathFlavor::Posix => self.path.clone(),
            PathFlavor::Windows => self.path.to_ascii_lowercase(),
        };
        let flavor_tag = match self.kind.flavor() {
            PathFlavor::Posix => "posix:",
            PathFlavor::Windows => "windows:",
        };
        format!("{flavor_tag}{normalized}")
    }

    /// Returns the final path component.
    #[must_use]
    pub fn name(&self) -> String {
        let parsed = parse_components(&self.path, self.kind.flavor());
        parsed.segments.last().cloned().unwrap_or_default()
    }

    /// Returns the final component without its last suffix.
    #[must_use]
    pub fn stem(&self) -> String {
        let name = self.name();
        if name.starts_with('.') && !name[1..].contains('.') {
            return name;
        }
        name.rsplit_once('.').map_or(name.clone(), |(stem, _)| stem.to_owned())
    }

    /// Returns the last suffix (including leading dot) or empty string.
    #[must_use]
    pub fn suffix(&self) -> String {
        let name = self.name();
        if name.is_empty() || name == "." || name == ".." {
            return String::new();
        }
        if name.starts_with('.') && !name[1..].contains('.') {
            return String::new();
        }
        name.rfind('.').map_or_else(String::new, |idx| name[idx..].to_owned())
    }

    /// Returns all suffixes in order (each includes leading dot).
    #[must_use]
    pub fn suffixes(&self) -> Vec<String> {
        let name = self.name();
        if name.is_empty() || name == "." || name == ".." {
            return Vec::new();
        }

        let start_idx = usize::from(name.starts_with('.'));
        let search = &name[start_idx..];

        let mut out = Vec::new();
        let mut pos = 0;
        while let Some(idx) = search[pos..].find('.') {
            let abs_idx = pos + idx;
            let end = search[abs_idx + 1..]
                .find('.')
                .map_or(search.len(), |next| abs_idx + 1 + next);
            out.push(name[start_idx + abs_idx..start_idx + end].to_owned());
            pos = abs_idx + 1;
        }
        out
    }

    /// Returns path components, including anchor as first item when present.
    #[must_use]
    pub fn parts(&self) -> Vec<String> {
        let parsed = parse_components(&self.path, self.kind.flavor());
        let mut out = Vec::with_capacity(parsed.segments.len() + usize::from(parsed.has_anchor()));

        if parsed.has_anchor() {
            if self.kind.flavor() == PathFlavor::Windows {
                let anchor = self.anchor();
                if !anchor.is_empty() {
                    out.push(anchor);
                }
            } else {
                out.push(parsed.anchor());
            }
        }

        out.extend(parsed.segments);
        out
    }

    /// Returns the parent directory as an internal normalized string.
    #[must_use]
    pub fn parent_path(&self) -> String {
        let parsed = parse_components(&self.path, self.kind.flavor());

        if parsed.segments.is_empty() {
            if self.path == "." {
                return ".".to_owned();
            }
            if parsed.has_anchor() {
                return build_path_from_components(&parsed, self.kind.flavor());
            }
            return ".".to_owned();
        }

        let mut parent = parsed;
        let _ = parent.segments.pop();
        if parent.segments.is_empty() && !parent.has_anchor() {
            ".".to_owned()
        } else {
            build_path_from_components(&parent, self.kind.flavor())
        }
    }

    /// Returns all logical parents in order (`parent`, `parent.parent`, ...).
    #[must_use]
    pub fn parents(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut current = self.path.clone();

        loop {
            let next = self.new_like(current.clone()).parent_path();
            if next == current {
                break;
            }
            out.push(next.clone());
            current = next;
        }

        out
    }

    /// Returns the root component.
    #[must_use]
    pub fn root(&self) -> String {
        let parsed = parse_components(&self.path, self.kind.flavor());
        if parsed.root.is_empty() {
            return String::new();
        }
        if self.kind.flavor() == PathFlavor::Windows {
            "\\".to_owned()
        } else {
            "/".to_owned()
        }
    }

    /// Returns the drive component.
    #[must_use]
    pub fn drive(&self) -> String {
        let parsed = parse_components(&self.path, self.kind.flavor());
        if self.kind.flavor() == PathFlavor::Windows {
            parsed.drive.replace('/', "\\")
        } else {
            parsed.drive
        }
    }

    /// Returns the anchor component (`drive + root`).
    #[must_use]
    pub fn anchor(&self) -> String {
        let parsed = parse_components(&self.path, self.kind.flavor());
        if self.kind.flavor() == PathFlavor::Windows {
            let mut out = parsed.drive.replace('/', "\\");
            if !parsed.root.is_empty() {
                out.push('\\');
            }
            out
        } else {
            parsed.anchor()
        }
    }

    /// Returns whether this path is absolute for its flavor.
    #[must_use]
    pub fn is_absolute(&self) -> bool {
        let parsed = parse_components(&self.path, self.kind.flavor());
        match self.kind.flavor() {
            PathFlavor::Posix => !parsed.root.is_empty(),
            // Windows absolute requires both drive and root.
            PathFlavor::Windows => !parsed.drive.is_empty() && !parsed.root.is_empty(),
        }
    }

    /// Joins this path with another lexical path fragment.
    #[must_use]
    pub fn joinpath(&self, other: &str) -> String {
        let other_normalized = normalize_path(other.to_owned(), self.kind.flavor());
        if self.kind.flavor() == PathFlavor::Posix {
            return self.joinpath_posix(&other_normalized);
        }
        self.joinpath_windows(&other_normalized)
    }

    /// Returns whether this path matches a glob-style pattern.
    #[must_use]
    pub fn matches_pattern(&self, pattern: &str) -> bool {
        let normalized_pattern = normalize_path(pattern.to_owned(), self.kind.flavor());
        let path_parsed = parse_components(&self.path, self.kind.flavor());
        let pattern_parsed = parse_components(&normalized_pattern, self.kind.flavor());

        if pattern_parsed.segments.is_empty() && !pattern_parsed.has_anchor() {
            return false;
        }

        let case_insensitive = self.kind.flavor() == PathFlavor::Windows;
        if pattern_parsed.has_anchor() {
            if !anchors_compatible(&path_parsed, &pattern_parsed, case_insensitive) {
                return false;
            }
            return glob_match_parts(&path_parsed.segments, &pattern_parsed.segments, case_insensitive);
        }

        // Relative patterns are matched from the right, but `**` may consume more segments.
        (0..=path_parsed.segments.len()).any(|start| {
            glob_match_parts(
                &path_parsed.segments[start..],
                &pattern_parsed.segments,
                case_insensitive,
            )
        })
    }

    /// Returns whether this path fully matches a glob-style pattern.
    #[must_use]
    pub fn full_matches_pattern(&self, pattern: &str) -> bool {
        let normalized_pattern = normalize_path(pattern.to_owned(), self.kind.flavor());
        let path_parsed = parse_components(&self.path, self.kind.flavor());
        let pattern_parsed = parse_components(&normalized_pattern, self.kind.flavor());

        if pattern_parsed.segments.is_empty() && !pattern_parsed.has_anchor() {
            return false;
        }

        let case_insensitive = self.kind.flavor() == PathFlavor::Windows;
        if pattern_parsed.has_anchor() {
            if !anchors_compatible(&path_parsed, &pattern_parsed, case_insensitive) {
                return false;
            }
            return glob_match_parts(&path_parsed.segments, &pattern_parsed.segments, case_insensitive);
        }

        if path_parsed.has_anchor() && pattern_parsed.segments.first().is_none_or(|segment| segment != "**") {
            return false;
        }

        glob_match_parts(&path_parsed.segments, &pattern_parsed.segments, case_insensitive)
    }

    /// Returns a new path with the final component replaced.
    ///
    /// # Errors
    /// Returns `ValueError` text when the current path has no name or when the
    /// replacement name is invalid.
    pub fn with_name(&self, name: &str) -> Result<String, String> {
        if name.is_empty() {
            return Err("Invalid name: empty string".to_owned());
        }
        if name.contains('/') || (self.kind.flavor() == PathFlavor::Windows && name.contains('\\')) {
            return Err(format!("Invalid name: {name:?} contains path separator"));
        }

        let mut parsed = parse_components(&self.path, self.kind.flavor());
        if parsed.segments.is_empty() {
            return Err("Path has no name".to_owned());
        }
        let _ = parsed.segments.pop();
        parsed.segments.push(name.to_owned());
        Ok(build_path_from_components(&parsed, self.kind.flavor()))
    }

    /// Returns a new path with the final stem replaced.
    ///
    /// # Errors
    /// Returns `ValueError` text when stem is invalid or path has no name.
    pub fn with_stem(&self, stem: &str) -> Result<String, String> {
        if stem.is_empty() {
            return Err("Invalid stem: empty string".to_owned());
        }
        if stem.contains('/') || (self.kind.flavor() == PathFlavor::Windows && stem.contains('\\')) {
            return Err(format!("Invalid stem: {stem:?} contains path separator"));
        }
        if self.name().is_empty() {
            return Err("Path has no name".to_owned());
        }

        let suffix = self.suffix();
        self.with_name(&format!("{stem}{suffix}"))
    }

    /// Returns a new path with the final suffix replaced.
    ///
    /// Keeps existing behavior where suffix without a leading dot is accepted and
    /// normalized by adding `.`.
    ///
    /// # Errors
    /// Returns `ValueError` text when suffix is invalid or path has no name.
    pub fn with_suffix(&self, suffix: &str) -> Result<String, String> {
        if self.name().is_empty() {
            return Err("Path has no name".to_owned());
        }

        let normalized_suffix = if suffix.is_empty() || suffix.starts_with('.') {
            suffix.to_owned()
        } else {
            format!(".{suffix}")
        };
        if normalized_suffix.contains('/')
            || (self.kind.flavor() == PathFlavor::Windows && normalized_suffix.contains('\\'))
        {
            return Err(format!("Invalid suffix: {normalized_suffix:?} contains path separator"));
        }

        let stem = self.stem();
        self.with_name(&format!("{stem}{normalized_suffix}"))
    }

    /// Computes a path relative to `other`.
    ///
    /// # Errors
    /// Returns `ValueError` text when `self` is not a subpath of `other`.
    pub fn relative_to(&self, other: &str) -> Result<String, String> {
        let self_parsed = parse_components(&self.path, self.kind.flavor());
        let other_normalized = normalize_path(other.to_owned(), self.kind.flavor());
        let other_parsed = parse_components(&other_normalized, self.kind.flavor());

        let case_insensitive = self.kind.flavor() == PathFlavor::Windows;
        let same_anchor = anchors_compatible(&self_parsed, &other_parsed, case_insensitive)
            && self_parsed.root == other_parsed.root
            && compare_component(&self_parsed.drive, &other_parsed.drive, case_insensitive);

        if !same_anchor || other_parsed.segments.len() > self_parsed.segments.len() {
            return Err(not_subpath_error(&self.path, &other_normalized, self.kind.flavor()));
        }

        for (left, right) in self_parsed.segments.iter().zip(&other_parsed.segments) {
            if !compare_component(left, right, case_insensitive) {
                return Err(not_subpath_error(&self.path, &other_normalized, self.kind.flavor()));
            }
        }

        let remaining = &self_parsed.segments[other_parsed.segments.len()..];
        if remaining.is_empty() {
            Ok(".".to_owned())
        } else {
            Ok(remaining.join("/"))
        }
    }

    /// Returns whether `self` is lexically relative to `other`.
    #[must_use]
    pub fn is_relative_to(&self, other: &str) -> bool {
        self.relative_to(other).is_ok()
    }

    /// Returns a POSIX-style representation (forward slashes).
    #[must_use]
    pub fn as_posix(&self) -> &str {
        &self.path
    }

    /// Returns the parser module repr associated with this path flavor.
    #[must_use]
    pub fn parser_repr(&self) -> &'static str {
        match self.kind.flavor() {
            PathFlavor::Posix => "<module 'posixpath' (frozen)>",
            PathFlavor::Windows => "<module 'ntpath' (frozen)>",
        }
    }

    /// Returns the path as a file URI.
    ///
    /// # Errors
    /// Returns an error if the path is not absolute.
    pub fn as_uri(&self) -> Result<String, String> {
        if !self.is_absolute() {
            return Err(format!("cannot represent '{}' as URI", self.display_path()));
        }
        let path = &self.path;
        match self.kind.flavor() {
            PathFlavor::Posix => Ok(format!("file://{path}")),
            PathFlavor::Windows => {
                // Windows URIs: file:///C:/path or file://server/share for UNC
                let parsed = parse_components(path, PathFlavor::Windows);
                if parsed.drive.len() == 2 && parsed.drive.as_bytes()[1] == b':' {
                    // Drive letter path: file:///C:/path
                    // parsed.drive is like "C:", parsed.root is like "/"
                    let drive_letter = &parsed.drive[0..1];
                    let rest = &path[2..]; // Skip "C:" prefix
                    Ok(format!("file:///{}:{}", drive_letter.to_ascii_uppercase(), rest))
                } else if !parsed.drive.is_empty() {
                    // UNC path: file://server/share/...
                    // parsed.drive for UNC is like "//server/share"
                    // Remove leading // and keep the rest, then add segments
                    let unc_host_share = parsed.drive.trim_start_matches('/');
                    let segments = if parsed.segments.is_empty() {
                        String::new()
                    } else {
                        format!("/{}", parsed.segments.join("/"))
                    };
                    Ok(format!("file://{unc_host_share}{segments}"))
                } else {
                    Ok(format!("file://{}", path.replace('\\', "/")))
                }
            }
        }
    }

    /// Returns whether the path is reserved on Windows (e.g., CON, PRN, AUX, NUL, COM1-9, LPT1-9).
    /// Always returns false for POSIX paths.
    #[must_use]
    pub fn is_reserved(&self) -> bool {
        if self.kind.flavor() == PathFlavor::Posix {
            return false;
        }
        let name = self.name();
        // Windows reserved names: CON, PRN, AUX, NUL, COM1-9, LPT1-9 (case-insensitive)
        let upper_name = name.to_ascii_uppercase();
        // Check for exact matches or matches with extension (e.g., CON.txt)
        let base_name = upper_name.split('.').next().unwrap_or(&upper_name);
        matches!(base_name, "CON" | "PRN" | "AUX" | "NUL")
            || base_name.len() == 4 && {
                let chars: Vec<char> = base_name.chars().collect();
                (chars[0] == 'C' && chars[1] == 'O' && chars[2] == 'M' && chars[3].is_ascii_digit() && chars[3] != '0')
                    || (chars[0] == 'L'
                        && chars[1] == 'P'
                        && chars[2] == 'T'
                        && chars[3].is_ascii_digit()
                        && chars[3] != '0')
            }
    }

    /// Creates a `pathlib.Path` object from constructor args.
    pub fn init(heap: &mut Heap<impl ResourceTracker>, args: ArgValues, interns: &Interns) -> RunResult<Value> {
        Self::init_with_kind(heap, args, interns, PathKind::Path, "Path")
    }

    /// Creates a `pathlib.PurePath` object from constructor args.
    pub fn init_pure_path(
        heap: &mut Heap<impl ResourceTracker>,
        args: ArgValues,
        interns: &Interns,
    ) -> RunResult<Value> {
        Self::init_with_kind(heap, args, interns, PathKind::PurePosixPath, "PurePath")
    }

    /// Creates a `pathlib.PurePosixPath` object from constructor args.
    pub fn init_pure_posix_path(
        heap: &mut Heap<impl ResourceTracker>,
        args: ArgValues,
        interns: &Interns,
    ) -> RunResult<Value> {
        Self::init_with_kind(heap, args, interns, PathKind::PurePosixPath, "PurePosixPath")
    }

    /// Creates a `pathlib.PureWindowsPath` object from constructor args.
    pub fn init_pure_windows_path(
        heap: &mut Heap<impl ResourceTracker>,
        args: ArgValues,
        interns: &Interns,
    ) -> RunResult<Value> {
        Self::init_with_kind(heap, args, interns, PathKind::PureWindowsPath, "PureWindowsPath")
    }

    /// Shared initializer for all public pathlib classes.
    fn init_with_kind(
        heap: &mut Heap<impl ResourceTracker>,
        args: ArgValues,
        interns: &Interns,
        kind: PathKind,
        type_name: &'static str,
    ) -> RunResult<Value> {
        let path_str = match args {
            ArgValues::Empty => ".".to_owned(),
            ArgValues::One(val) => {
                let result = extract_path_string(&val, heap, interns);
                val.drop_with_heap(heap);
                result?
            }
            ArgValues::Two(a, b) => {
                let a_str = extract_path_string(&a, heap, interns);
                let b_str = extract_path_string(&b, heap, interns);
                a.drop_with_heap(heap);
                b.drop_with_heap(heap);
                Self::new_with_kind(a_str?, kind).joinpath(&b_str?)
            }
            ArgValues::Kwargs(kwargs) => {
                kwargs.drop_with_heap(heap);
                return Err(ExcType::type_error_no_kwargs(type_name));
            }
            ArgValues::ArgsKargs { args: vals, kwargs } => {
                if !kwargs.is_empty() {
                    for v in vals {
                        v.drop_with_heap(heap);
                    }
                    kwargs.drop_with_heap(heap);
                    return Err(ExcType::type_error_no_kwargs(type_name));
                }
                if vals.is_empty() {
                    return Ok(Value::Ref(
                        heap.allocate(HeapData::Path(Self::new_with_kind(".".to_owned(), kind)))?,
                    ));
                }
                let mut result = String::new();
                for val in vals {
                    let part = extract_path_string(&val, heap, interns);
                    val.drop_with_heap(heap);
                    result = Self::new_with_kind(result, kind).joinpath(&part?);
                }
                result
            }
        };

        let path = Self::new_with_kind(path_str, kind);
        Ok(Value::Ref(heap.allocate(HeapData::Path(path))?))
    }

    /// POSIX join semantics.
    fn joinpath_posix(&self, other: &str) -> String {
        if other.starts_with('/') || self.path.is_empty() || self.path == "." {
            normalize_path(other.to_owned(), PathFlavor::Posix)
        } else if self.path.ends_with('/') {
            normalize_path(format!("{}{}", self.path, other), PathFlavor::Posix)
        } else {
            normalize_path(format!("{}/{}", self.path, other), PathFlavor::Posix)
        }
    }

    /// Windows join semantics.
    fn joinpath_windows(&self, other: &str) -> String {
        let other_parsed = parse_components(other, PathFlavor::Windows);
        if !other_parsed.drive.is_empty() {
            return normalize_path(other.to_owned(), PathFlavor::Windows);
        }

        if !other_parsed.root.is_empty() {
            let self_parsed = parse_components(&self.path, PathFlavor::Windows);
            if !self_parsed.drive.is_empty() {
                return normalize_path(format!("{}{}", self_parsed.drive, other), PathFlavor::Windows);
            }
            return normalize_path(other.to_owned(), PathFlavor::Windows);
        }

        if self.path.is_empty() || self.path == "." {
            return normalize_path(other.to_owned(), PathFlavor::Windows);
        }

        if self.path.ends_with('/') || self.path.ends_with(':') {
            normalize_path(format!("{}{}", self.path, other), PathFlavor::Windows)
        } else {
            normalize_path(format!("{}/{}", self.path, other), PathFlavor::Windows)
        }
    }
}

/// Extracts a path string from a value.
fn extract_path_string(val: &Value, heap: &Heap<impl ResourceTracker>, interns: &Interns) -> RunResult<String> {
    match val {
        Value::InternString(string_id) => Ok(interns.get_str(*string_id).to_owned()),
        Value::Ref(heap_id) => match heap.get(*heap_id) {
            HeapData::Str(s) => Ok(s.as_str().to_owned()),
            HeapData::Path(p) => Ok(p.as_str().to_owned()),
            _ => Err(ExcType::type_error(format!(
                "expected str or Path, got {}",
                val.py_type(heap)
            ))),
        },
        _ => Err(ExcType::type_error(format!(
            "expected str or Path, got {}",
            val.py_type(heap)
        ))),
    }
}

/// Handles the `/` operator for all pathlib path objects.
pub(crate) fn path_div(
    path_id: HeapId,
    other: &Value,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    let other_str = match other {
        Value::InternString(string_id) => interns.get_str(*string_id).to_owned(),
        Value::Ref(other_id) => match heap.get(*other_id) {
            HeapData::Str(s) => s.as_str().to_owned(),
            HeapData::Path(p) => p.as_str().to_owned(),
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };

    let lhs = match heap.get(path_id) {
        HeapData::Path(p) => p.clone(),
        _ => return Ok(None),
    };

    let result = lhs.joinpath(&other_str);
    Ok(Some(Value::Ref(heap.allocate(HeapData::Path(lhs.new_like(result)))?)))
}

/// Handles reverse `/` operator for pathlib paths (`str / Path`).
pub(crate) fn path_rdiv(
    left: &Value,
    path_id: HeapId,
    heap: &mut Heap<impl ResourceTracker>,
    interns: &Interns,
) -> RunResult<Option<Value>> {
    let left_str = match left {
        Value::InternString(string_id) => interns.get_str(*string_id).to_owned(),
        Value::Ref(left_id) => match heap.get(*left_id) {
            HeapData::Str(s) => s.as_str().to_owned(),
            HeapData::Path(p) => p.as_str().to_owned(),
            _ => return Ok(None),
        },
        _ => return Ok(None),
    };

    let rhs = match heap.get(path_id) {
        HeapData::Path(p) => p.clone(),
        _ => return Ok(None),
    };

    let lhs = Path::new_with_kind(left_str, rhs.kind);
    let result = lhs.joinpath(rhs.as_str());
    Ok(Some(Value::Ref(heap.allocate(HeapData::Path(lhs.new_like(result)))?)))
}

/// Returns the `ValueError` message used by `relative_to` when paths don't align.
fn not_subpath_error(path: &str, other: &str, flavor: PathFlavor) -> String {
    let left = display_for_flavor(path, flavor);
    let right = display_for_flavor(other, flavor);
    format!("'{left}' is not in the subpath of '{right}'")
}

/// Converts internal `/` separators to flavor display separators.
fn display_for_flavor(path: &str, flavor: PathFlavor) -> String {
    if flavor == PathFlavor::Windows {
        path.replace('/', "\\")
    } else {
        path.to_owned()
    }
}

/// Parses normalized path into `drive`, `root`, and `segments`.
fn parse_components(path: &str, flavor: PathFlavor) -> ParsedPath {
    match flavor {
        PathFlavor::Posix => parse_posix_components(path),
        PathFlavor::Windows => parse_windows_components(path),
    }
}

/// Parses POSIX components.
fn parse_posix_components(path: &str) -> ParsedPath {
    if path == "." || path.is_empty() {
        return ParsedPath {
            drive: String::new(),
            root: String::new(),
            segments: Vec::new(),
        };
    }

    let (root, rest) = if let Some(stripped) = path.strip_prefix('/') {
        ("/".to_owned(), stripped)
    } else {
        (String::new(), path)
    };

    let segments = rest
        .split('/')
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect();

    ParsedPath {
        drive: String::new(),
        root,
        segments,
    }
}

/// Parses Windows components with basic drive and UNC handling.
fn parse_windows_components(path: &str) -> ParsedPath {
    if path == "." || path.is_empty() {
        return ParsedPath {
            drive: String::new(),
            root: String::new(),
            segments: Vec::new(),
        };
    }

    if let Some((drive, rest)) = parse_unc_prefix(path) {
        return ParsedPath {
            drive: drive.to_owned(),
            root: "/".to_owned(),
            segments: split_segments(rest),
        };
    }

    if let Some((drive, rest)) = parse_drive_prefix(path) {
        let (root, tail) = if let Some(stripped) = rest.strip_prefix('/') {
            ("/".to_owned(), stripped)
        } else {
            (String::new(), rest)
        };
        return ParsedPath {
            drive: drive.to_owned(),
            root,
            segments: split_segments(tail),
        };
    }

    let (root, rest) = if let Some(stripped) = path.strip_prefix('/') {
        ("/".to_owned(), stripped)
    } else {
        (String::new(), path)
    };

    ParsedPath {
        drive: String::new(),
        root,
        segments: split_segments(rest),
    }
}

/// Splits tail segments, dropping empty entries.
fn split_segments(rest: &str) -> Vec<String> {
    rest.split('/')
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

/// Parses a UNC drive prefix (`//server/share`) and returns `(drive, remaining_tail)`.
fn parse_unc_prefix(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix("//")?;
    let server_end = rest.find('/')?;
    let server = &rest[..server_end];
    if server.is_empty() {
        return None;
    }

    let after_server = &rest[server_end + 1..];
    let share_end = after_server.find('/').unwrap_or(after_server.len());
    let share = &after_server[..share_end];
    if share.is_empty() {
        return None;
    }

    let drive_end = 2 + server_end + 1 + share_end;
    let drive = &path[..drive_end];
    let remaining = path[drive_end..].strip_prefix('/').unwrap_or(&path[drive_end..]);
    Some((drive, remaining))
}

/// Parses a drive-letter prefix (`C:`) and returns `(drive, remaining_tail)`.
fn parse_drive_prefix(path: &str) -> Option<(&str, &str)> {
    let bytes = path.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let first = bytes[0] as char;
    if !first.is_ascii_alphabetic() || bytes[1] != b':' {
        return None;
    }
    Some((&path[..2], &path[2..]))
}

/// Rebuilds a normalized path string from parsed components.
fn build_path_from_components(parsed: &ParsedPath, flavor: PathFlavor) -> String {
    let mut out = String::new();
    out.push_str(&parsed.drive);
    out.push_str(&parsed.root);

    if !parsed.segments.is_empty() {
        if !out.is_empty() && !out.ends_with('/') {
            out.push('/');
        }
        out.push_str(&parsed.segments.join("/"));
    }

    if out.is_empty() {
        return ".".to_owned();
    }
    normalize_path(out, flavor)
}

/// Returns whether two parsed anchors are compatible for operations like `relative_to`.
fn anchors_compatible(path: &ParsedPath, other: &ParsedPath, case_insensitive: bool) -> bool {
    compare_component(&path.drive, &other.drive, case_insensitive) && path.root == other.root
}

/// Case-aware component comparison used by Windows/Posix path operations.
fn compare_component(left: &str, right: &str, case_insensitive: bool) -> bool {
    if case_insensitive {
        left.eq_ignore_ascii_case(right)
    } else {
        left == right
    }
}

/// Matches path components against glob components.
fn glob_match_parts(path_parts: &[String], pattern_parts: &[String], case_insensitive: bool) -> bool {
    let mut memo = vec![vec![None; pattern_parts.len() + 1]; path_parts.len() + 1];
    glob_match_parts_inner(0, 0, path_parts, pattern_parts, case_insensitive, &mut memo)
}

/// Recursive glob matcher with memoization.
fn glob_match_parts_inner(
    path_idx: usize,
    pattern_idx: usize,
    path_parts: &[String],
    pattern_parts: &[String],
    case_insensitive: bool,
    memo: &mut [Vec<Option<bool>>],
) -> bool {
    if let Some(cached) = memo[path_idx][pattern_idx] {
        return cached;
    }

    let result = if pattern_idx == pattern_parts.len() {
        path_idx == path_parts.len()
    } else {
        let pattern_part = &pattern_parts[pattern_idx];
        if pattern_part == "**" {
            glob_match_parts_inner(
                path_idx,
                pattern_idx + 1,
                path_parts,
                pattern_parts,
                case_insensitive,
                memo,
            ) || (path_idx < path_parts.len()
                && glob_match_parts_inner(
                    path_idx + 1,
                    pattern_idx,
                    path_parts,
                    pattern_parts,
                    case_insensitive,
                    memo,
                ))
        } else {
            path_idx < path_parts.len()
                && wildcard_match_component(&path_parts[path_idx], pattern_part, case_insensitive)
                && glob_match_parts_inner(
                    path_idx + 1,
                    pattern_idx + 1,
                    path_parts,
                    pattern_parts,
                    case_insensitive,
                    memo,
                )
        }
    };

    memo[path_idx][pattern_idx] = Some(result);
    result
}

/// Matches a single path component against a wildcard pattern (`*` and `?`).
fn wildcard_match_component(text: &str, pattern: &str, case_insensitive: bool) -> bool {
    let normalized_text = if case_insensitive {
        text.to_ascii_lowercase()
    } else {
        text.to_owned()
    };
    let normalized_pattern = if case_insensitive {
        pattern.to_ascii_lowercase()
    } else {
        pattern.to_owned()
    };

    let text_chars: Vec<char> = normalized_text.chars().collect();
    let pattern_chars: Vec<char> = normalized_pattern.chars().collect();

    let mut dp = vec![vec![false; pattern_chars.len() + 1]; text_chars.len() + 1];
    dp[0][0] = true;

    for pattern_idx in 1..=pattern_chars.len() {
        if pattern_chars[pattern_idx - 1] == '*' {
            dp[0][pattern_idx] = dp[0][pattern_idx - 1];
        }
    }

    for text_idx in 1..=text_chars.len() {
        for pattern_idx in 1..=pattern_chars.len() {
            let pattern_char = pattern_chars[pattern_idx - 1];
            dp[text_idx][pattern_idx] = match pattern_char {
                '*' => dp[text_idx][pattern_idx - 1] || dp[text_idx - 1][pattern_idx],
                '?' => dp[text_idx - 1][pattern_idx - 1],
                _ => dp[text_idx - 1][pattern_idx - 1] && text_chars[text_idx - 1] == pattern_char,
            };
        }
    }

    dp[text_chars.len()][pattern_chars.len()]
}

/// Normalizes a path string according to flavor.
fn normalize_path(path: String, flavor: PathFlavor) -> String {
    match flavor {
        PathFlavor::Posix => normalize_posix_path(path),
        PathFlavor::Windows => normalize_windows_path(&path),
    }
}

/// POSIX normalization.
fn normalize_posix_path(mut path: String) -> String {
    if path.contains('\\') {
        path = path.replace('\\', "/");
    }
    if path.is_empty() {
        return ".".to_owned();
    }

    let preserve_double_slash = path.starts_with("//") && !path.starts_with("///");
    let is_absolute = path.starts_with('/');

    let mut segments = Vec::new();
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." {
            continue;
        }
        segments.push(segment);
    }

    let mut normalized = String::new();
    if preserve_double_slash {
        normalized.push_str("//");
    } else if is_absolute {
        normalized.push('/');
    }
    normalized.push_str(&segments.join("/"));

    if normalized.is_empty() {
        ".".to_owned()
    } else {
        normalized
    }
}

/// Windows normalization.
fn normalize_windows_path(path: &str) -> String {
    let replaced = path.replace('\\', "/");
    let mut collapsed = String::with_capacity(replaced.len());

    let mut chars = replaced.chars().peekable();
    let mut at_start = true;
    let mut prev_sep = false;
    while let Some(ch) = chars.next() {
        if ch == '/' {
            if at_start && matches!(chars.peek(), Some('/')) {
                collapsed.push('/');
                collapsed.push('/');
                while matches!(chars.peek(), Some('/')) {
                    let _ = chars.next();
                }
                at_start = false;
                prev_sep = true;
                continue;
            }
            if !prev_sep {
                collapsed.push('/');
                prev_sep = true;
                at_start = false;
            }
            continue;
        }

        collapsed.push(ch);
        prev_sep = false;
        at_start = false;
    }

    while collapsed.len() > 1
        && collapsed.ends_with('/')
        && !is_windows_drive_root(&collapsed)
        && !is_windows_unc_anchor(&collapsed)
    {
        collapsed.pop();
    }

    if is_windows_unc_anchor(&collapsed) && !collapsed.ends_with('/') {
        collapsed.push('/');
    }

    collapsed
}

/// Returns whether string is a Windows drive root (`C:/`).
fn is_windows_drive_root(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() == 3 && (bytes[0] as char).is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'/'
}

/// Returns whether string is a UNC share anchor (`//server/share` or with trailing `/`).
fn is_windows_unc_anchor(path: &str) -> bool {
    let Some(rest) = path.strip_prefix("//") else {
        return false;
    };

    let rest = rest.trim_end_matches('/');
    let mut parts = rest.split('/').filter(|part| !part.is_empty());
    matches!((parts.next(), parts.next(), parts.next()), (Some(_), Some(_), None))
}

/// Prepends the path object argument to existing arguments for OS calls.
fn prepend_path_arg(path_arg: Value, args: ArgValues) -> ArgValues {
    match args {
        ArgValues::Empty => ArgValues::One(path_arg),
        ArgValues::One(v) => ArgValues::Two(path_arg, v),
        ArgValues::Two(a, b) => ArgValues::ArgsKargs {
            args: vec![path_arg, a, b],
            kwargs: KwargsValues::Empty,
        },
        ArgValues::Kwargs(kwargs) => ArgValues::ArgsKargs {
            args: vec![path_arg],
            kwargs,
        },
        ArgValues::ArgsKargs { args: mut vals, kwargs } => {
            vals.insert(0, path_arg);
            ArgValues::ArgsKargs { args: vals, kwargs }
        }
    }
}

/// Returns the path string to pass to host OS callbacks for `resolve`/`absolute`.
///
/// For POSIX, `Path('./x').resolve()` and `Path('./x').absolute()` should preserve
/// explicit relative intent for the host callback boundary. Bare relative paths are
/// therefore prefixed with `./` while already-qualified relative forms (`./`, `../`)
/// and anchored paths are kept unchanged.
fn resolve_or_absolute_os_arg_path(path: &str, flavor: PathFlavor) -> String {
    if flavor != PathFlavor::Posix {
        return path.to_owned();
    }

    if path == "."
        || path == ".."
        || path.starts_with("./")
        || path.starts_with("../")
        || path.starts_with('/')
        || path.starts_with("//")
    {
        return path.to_owned();
    }

    format!("./{path}")
}

impl PyTrait for Path {
    fn py_type(&self, _heap: &Heap<impl ResourceTracker>) -> Type {
        self.kind.py_type()
    }

    fn py_len(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> Option<usize> {
        None
    }

    fn py_eq(&self, other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        if self.kind.flavor() != other.kind.flavor() {
            return false;
        }
        compare_component(&self.path, &other.path, self.kind.flavor() == PathFlavor::Windows)
    }

    fn py_cmp(&self, other: &Self, _heap: &mut Heap<impl ResourceTracker>, _interns: &Interns) -> Option<Ordering> {
        if self.kind.flavor() != other.kind.flavor() {
            return None;
        }

        let case_insensitive = self.kind.flavor() == PathFlavor::Windows;
        let left = if case_insensitive {
            self.path.to_ascii_lowercase()
        } else {
            self.path.clone()
        };
        let right = if case_insensitive {
            other.path.to_ascii_lowercase()
        } else {
            other.path.clone()
        };
        Some(left.cmp(&right))
    }

    fn py_bool(&self, _heap: &Heap<impl ResourceTracker>, _interns: &Interns) -> bool {
        true
    }

    fn py_repr_fmt(
        &self,
        f: &mut impl Write,
        _heap: &Heap<impl ResourceTracker>,
        _heap_ids: &mut AHashSet<HeapId>,
        _interns: &Interns,
    ) -> std::fmt::Result {
        write!(f, "{}('{}')", self.kind.repr_type_name(), self.path)
    }

    fn py_dec_ref_ids(&mut self, _stack: &mut Vec<HeapId>) {}

    fn py_estimate_size(&self) -> usize {
        std::mem::size_of::<Self>() + self.path.capacity()
    }

    fn py_call_attr(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        _self_id: Option<HeapId>,
    ) -> RunResult<Value> {
        let mut args_guard = HeapGuard::new(args, heap);
        let (args, heap) = args_guard.as_parts();

        let Some(method) = attr.static_string() else {
            return Err(ExcType::attribute_error(self.kind.py_type(), attr.as_str(interns)));
        };

        match method {
            StaticStrings::IsAbsolute => Ok(Value::Bool(self.is_absolute())),
            StaticStrings::Joinpath => match args {
                ArgValues::Empty => Err(ExcType::type_error_at_least("joinpath", 1, 0)),
                ArgValues::One(val) => {
                    let other = extract_path_string(val, heap, interns);
                    let result = self.joinpath(&other?);
                    Ok(Value::Ref(heap.allocate(HeapData::Path(self.new_like(result)))?))
                }
                ArgValues::Two(a, b) => {
                    let a_str = extract_path_string(a, heap, interns);
                    let b_str = extract_path_string(b, heap, interns);
                    let mut result = self.joinpath(&a_str?);
                    result = self.new_like(result).joinpath(&b_str?);
                    Ok(Value::Ref(heap.allocate(HeapData::Path(self.new_like(result)))?))
                }
                ArgValues::Kwargs(_) => Err(ExcType::type_error_no_kwargs("joinpath")),
                ArgValues::ArgsKargs { args: vals, kwargs } => {
                    if !kwargs.is_empty() {
                        return Err(ExcType::type_error_no_kwargs("joinpath"));
                    }
                    let mut result = self.path.clone();
                    for val in vals {
                        let part = extract_path_string(val, heap, interns);
                        result = self.new_like(result).joinpath(&part?);
                    }
                    Ok(Value::Ref(heap.allocate(HeapData::Path(self.new_like(result)))?))
                }
            },
            StaticStrings::WithName => {
                let (args, heap) = args_guard.into_parts();
                let name_val = args.get_one_arg("with_name", heap)?;
                let name = extract_path_string(&name_val, heap, interns);
                name_val.drop_with_heap(heap);
                let result = self
                    .with_name(&name?)
                    .map_err(|e| SimpleException::new_msg(ExcType::ValueError, &e))?;
                Ok(Value::Ref(heap.allocate(HeapData::Path(self.new_like(result)))?))
            }
            StaticStrings::WithStem => {
                let (args, heap) = args_guard.into_parts();
                let stem_val = args.get_one_arg("with_stem", heap)?;
                let stem = extract_path_string(&stem_val, heap, interns);
                stem_val.drop_with_heap(heap);
                let result = self
                    .with_stem(&stem?)
                    .map_err(|e| SimpleException::new_msg(ExcType::ValueError, &e))?;
                Ok(Value::Ref(heap.allocate(HeapData::Path(self.new_like(result)))?))
            }
            StaticStrings::WithSuffix => {
                let (args, heap) = args_guard.into_parts();
                let suffix_val = args.get_one_arg("with_suffix", heap)?;
                let suffix = extract_path_string(&suffix_val, heap, interns);
                suffix_val.drop_with_heap(heap);
                let result = self
                    .with_suffix(&suffix?)
                    .map_err(|e| SimpleException::new_msg(ExcType::ValueError, &e))?;
                Ok(Value::Ref(heap.allocate(HeapData::Path(self.new_like(result)))?))
            }
            StaticStrings::WithSegments => match args {
                ArgValues::Empty => Err(ExcType::type_error_at_least("with_segments", 1, 0)),
                ArgValues::One(val) => {
                    let segment = extract_path_string(val, heap, interns)?;
                    let result = Self::new_with_kind(segment, self.kind);
                    Ok(Value::Ref(heap.allocate(HeapData::Path(result))?))
                }
                ArgValues::Two(a, b) => {
                    let first = extract_path_string(a, heap, interns)?;
                    let second = extract_path_string(b, heap, interns)?;
                    let mut result = Self::new_with_kind(first, self.kind);
                    result = Self::new_with_kind(result.joinpath(&second), self.kind);
                    Ok(Value::Ref(heap.allocate(HeapData::Path(result))?))
                }
                ArgValues::Kwargs(_) => Err(ExcType::type_error_no_kwargs("with_segments")),
                ArgValues::ArgsKargs { args: vals, kwargs } => {
                    if !kwargs.is_empty() {
                        return Err(ExcType::type_error_no_kwargs("with_segments"));
                    }
                    if vals.is_empty() {
                        return Err(ExcType::type_error_at_least("with_segments", 1, 0));
                    }

                    let first = extract_path_string(&vals[0], heap, interns)?;
                    let mut result = Self::new_with_kind(first, self.kind);
                    for val in &vals[1..] {
                        let segment = extract_path_string(val, heap, interns)?;
                        result = Self::new_with_kind(result.joinpath(&segment), self.kind);
                    }
                    Ok(Value::Ref(heap.allocate(HeapData::Path(result))?))
                }
            },
            StaticStrings::AsPosix => Ok(Value::Ref(
                heap.allocate(HeapData::Str(Str::new(self.as_posix().to_owned())))?,
            )),
            StaticStrings::Fspath => Ok(Value::Ref(heap.allocate(HeapData::Str(Str::new(self.display_path())))?)),
            StaticStrings::RelativeTo => {
                let (args, heap) = args_guard.into_parts();
                let other_val = args.get_one_arg("relative_to", heap)?;
                let other = extract_path_string(&other_val, heap, interns);
                other_val.drop_with_heap(heap);
                let result = self
                    .relative_to(&other?)
                    .map_err(|e| SimpleException::new_msg(ExcType::ValueError, &e))?;
                Ok(Value::Ref(heap.allocate(HeapData::Path(self.new_like(result)))?))
            }
            StaticStrings::IsRelativeTo => {
                let (args, heap) = args_guard.into_parts();
                let other_val = args.get_one_arg("is_relative_to", heap)?;
                let other = extract_path_string(&other_val, heap, interns);
                other_val.drop_with_heap(heap);
                Ok(Value::Bool(self.is_relative_to(&other?)))
            }
            StaticStrings::ReMatch => {
                let (args, heap) = args_guard.into_parts();
                let pattern_val = args.get_one_arg("match", heap)?;
                let pattern = extract_path_string(&pattern_val, heap, interns);
                pattern_val.drop_with_heap(heap);
                Ok(Value::Bool(self.matches_pattern(&pattern?)))
            }
            StaticStrings::FullMatch => {
                let (args, heap) = args_guard.into_parts();
                let pattern_val = args.get_one_arg("full_match", heap)?;
                let pattern = extract_path_string(&pattern_val, heap, interns);
                pattern_val.drop_with_heap(heap);
                Ok(Value::Bool(self.full_matches_pattern(&pattern?)))
            }
            StaticStrings::AsUri => {
                let uri = self
                    .as_uri()
                    .map_err(|e| SimpleException::new_msg(ExcType::ValueError, &e))?;
                Ok(Value::Ref(heap.allocate(HeapData::Str(Str::new(uri)))?))
            }
            StaticStrings::IsReserved => Ok(Value::Bool(self.is_reserved())),
            _ => Err(ExcType::attribute_error(self.kind.py_type(), attr.as_str(interns))),
        }
    }

    fn py_call_attr_raw(
        &mut self,
        heap: &mut Heap<impl ResourceTracker>,
        attr: &EitherStr,
        args: ArgValues,
        interns: &Interns,
        self_id: Option<HeapId>,
    ) -> RunResult<AttrCallResult> {
        let Some(method) = attr.static_string() else {
            return self
                .py_call_attr(heap, attr, args, interns, self_id)
                .map(AttrCallResult::Value);
        };

        if self.kind.allows_os_calls()
            && let Ok(os_fn) = OsFunction::try_from(method)
        {
            let mut os_path = self.clone();
            if matches!(os_fn, OsFunction::Resolve | OsFunction::Absolute) {
                os_path.path = resolve_or_absolute_os_arg_path(&os_path.path, self.kind.flavor());
            }
            let path_arg = Value::Ref(heap.allocate(HeapData::Path(os_path))?);
            let os_args = prepend_path_arg(path_arg, args);
            return Ok(AttrCallResult::OsCall(os_fn, os_args));
        }

        self.py_call_attr(heap, attr, args, interns, self_id)
            .map(AttrCallResult::Value)
    }

    fn py_getattr(
        &self,
        attr_id: StringId,
        heap: &mut Heap<impl ResourceTracker>,
        interns: &Interns,
    ) -> RunResult<Option<AttrCallResult>> {
        let value = match StaticStrings::from_string_id(attr_id) {
            Some(StaticStrings::Name) => Value::Ref(heap.allocate(HeapData::Str(Str::new(self.name())))?),
            Some(StaticStrings::Parent) => {
                let parent_path = self.parent_path();
                Value::Ref(heap.allocate(HeapData::Path(self.new_like(parent_path)))?)
            }
            Some(StaticStrings::Parents) => {
                let parents = self.parents();
                let mut items = SmallVec::with_capacity(parents.len());
                for parent in parents {
                    let path_id = heap.allocate(HeapData::Path(self.new_like(parent)))?;
                    items.push(Value::Ref(path_id));
                }
                allocate_tuple(items, heap)?
            }
            Some(StaticStrings::Stem) => Value::Ref(heap.allocate(HeapData::Str(Str::new(self.stem())))?),
            Some(StaticStrings::Suffix) => Value::Ref(heap.allocate(HeapData::Str(Str::new(self.suffix())))?),
            Some(StaticStrings::Suffixes) => {
                let suffixes = self.suffixes();
                let mut items = Vec::with_capacity(suffixes.len());
                for suffix in suffixes {
                    let str_id = heap.allocate(HeapData::Str(Str::new(suffix)))?;
                    items.push(Value::Ref(str_id));
                }
                Value::Ref(heap.allocate(HeapData::List(List::new(items)))?)
            }
            Some(StaticStrings::Parts) => {
                let parts = self.parts();
                let mut items = SmallVec::with_capacity(parts.len());
                for part in parts {
                    let str_id = heap.allocate(HeapData::Str(Str::new(part)))?;
                    items.push(Value::Ref(str_id));
                }
                allocate_tuple(items, heap)?
            }
            Some(StaticStrings::Root) => Value::Ref(heap.allocate(HeapData::Str(Str::new(self.root())))?),
            Some(StaticStrings::Anchor) => Value::Ref(heap.allocate(HeapData::Str(Str::new(self.anchor())))?),
            Some(StaticStrings::Drive) => Value::Ref(heap.allocate(HeapData::Str(Str::new(self.drive())))?),
            Some(StaticStrings::Parser) => {
                Value::Ref(heap.allocate(HeapData::Str(Str::new(self.parser_repr().to_owned())))?)
            }
            _ => return Err(ExcType::attribute_error(self.kind.py_type(), interns.get_str(attr_id))),
        };

        Ok(Some(AttrCallResult::Value(value)))
    }
}
