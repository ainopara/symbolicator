use std::collections::{BTreeMap, BTreeSet};
use std::convert::TryInto;
use std::iter::FromIterator;
use std::sync::Arc;
use std::time::{Duration, Instant};

use actix::ResponseFuture;
use apple_crash_report_parser::AppleCrashReport;
use bytes::{Bytes, IntoBuf};
use failure::Fail;
use futures::{compat::Future01CompatExt, FutureExt as _, TryFutureExt};
use futures01::future::{self, join_all, Either, Future, IntoFuture, Shared};
use futures01::sync::oneshot;
use parking_lot::RwLock;
use regex::Regex;
use sentry::integrations::failure::capture_fail;
use symbolic::common::{Arch, ByteView, CodeId, DebugId, InstructionInfo, Language, SelfCell, Name};
use symbolic::debuginfo::{Object, ObjectDebugSession};
use symbolic::demangle::{Demangle, DemangleFormat, DemangleOptions};
use symbolic::minidump::processor::{
    CodeModule, CodeModuleId, FrameInfoMap, FrameTrust, ProcessMinidumpError, ProcessState, RegVal,
};
use tokio::prelude::FutureExt;
use uuid;

use crate::actors::cficaches::{
    CfiCacheActor, CfiCacheError, CfiCacheErrorKind, CfiCacheFile, FetchCfiCache,
};
use crate::actors::objects::{FindObject, ObjectPurpose, ObjectsActor};
use crate::actors::symcaches::{
    FetchSymCache, SymCacheActor, SymCacheError, SymCacheErrorKind, SymCacheFile,
};
use crate::logging::LogError;
use crate::types::{
    ArcFail, CompleteObjectInfo, CompleteStacktrace, CompletedSymbolicationResponse, FileType,
    FrameStatus, ObjectFileStatus, ObjectId, ObjectType, RawFrame, RawObjectInfo, RawStacktrace,
    Registers, RequestId, Scope, Signal, SourceConfig, SymbolicatedFrame, SymbolicationResponse,
    SystemInfo,
};
use crate::utils::futures::ThreadPool;
use crate::utils::hex::HexValue;
use crate::utils::sentry::SentryFutureExt;

const DEMANGLE_OPTIONS: DemangleOptions = DemangleOptions {
    with_arguments: true,
    format: DemangleFormat::Short,
};

lazy_static::lazy_static! {
    /// Format sent by Unreal Engine on macOS
    static ref OS_MACOS_REGEX: Regex = Regex::new(r#"^Mac OS X (?P<version>\d+\.\d+\.\d+)( \((?P<build>[a-fA-F0-9]+)\))?$"#).unwrap();
}

/// Variants of `SymbolicationError`.
#[derive(Debug, Fail)]
pub enum SymbolicationErrorKind {
    #[fail(display = "symbolication took too long")]
    Timeout,

    #[fail(display = "internal IO failed")]
    Io,

    #[fail(display = "computation was canceled internally")]
    Canceled,

    #[fail(display = "failed to process minidump")]
    InvalidMinidump,

    #[fail(display = "failed to parse apple crash report")]
    InvalidAppleCrashReport,
}

symbolic::common::derive_failure!(
    SymbolicationError,
    SymbolicationErrorKind,
    doc = "Errors during symbolication."
);

impl From<std::io::Error> for SymbolicationError {
    fn from(err: std::io::Error) -> Self {
        err.context(SymbolicationErrorKind::Io).into()
    }
}

impl From<ProcessMinidumpError> for SymbolicationError {
    fn from(err: ProcessMinidumpError) -> Self {
        err.context(SymbolicationErrorKind::InvalidMinidump).into()
    }
}

impl From<apple_crash_report_parser::ParseError> for SymbolicationError {
    fn from(err: apple_crash_report_parser::ParseError) -> Self {
        err.context(SymbolicationErrorKind::InvalidAppleCrashReport)
            .into()
    }
}

impl From<&SymbolicationError> for SymbolicationResponse {
    fn from(err: &SymbolicationError) -> SymbolicationResponse {
        match err.kind() {
            SymbolicationErrorKind::Timeout => SymbolicationResponse::Timeout,
            SymbolicationErrorKind::Io => SymbolicationResponse::InternalError,
            SymbolicationErrorKind::Canceled => SymbolicationResponse::InternalError,
            SymbolicationErrorKind::InvalidMinidump => SymbolicationResponse::Failed {
                message: err.to_string(),
            },
            SymbolicationErrorKind::InvalidAppleCrashReport => SymbolicationResponse::Failed {
                message: err.to_string(),
            },
        }
    }
}

// We probably want a shared future here because otherwise polling for a response would acquire the
// global write lock.
type ComputationChannel = Shared<oneshot::Receiver<(Instant, SymbolicationResponse)>>;

type ComputationMap = Arc<RwLock<BTreeMap<RequestId, ComputationChannel>>>;

#[derive(Clone, Debug)]
pub struct SymbolicationActor {
    objects: ObjectsActor,
    symcaches: SymCacheActor,
    cficaches: CfiCacheActor,
    threadpool: ThreadPool,
    requests: ComputationMap,
}

impl SymbolicationActor {
    pub fn new(
        objects: ObjectsActor,
        symcaches: SymCacheActor,
        cficaches: CfiCacheActor,
        threadpool: ThreadPool,
    ) -> Self {
        let requests = Arc::new(RwLock::new(BTreeMap::new()));

        SymbolicationActor {
            objects,
            symcaches,
            cficaches,
            threadpool,
            requests,
        }
    }

    fn wrap_response_channel(
        &self,
        request_id: RequestId,
        timeout: Option<u64>,
        channel: ComputationChannel,
    ) -> impl Future<Item = SymbolicationResponse, Error = SymbolicationError> {
        let rv = channel
            .map(|item| (*item).clone())
            .map_err(|_| SymbolicationErrorKind::Canceled.into());

        let requests = &self.requests;

        if let Some(timeout) = timeout {
            Either::A(
                rv.timeout(Duration::from_secs(timeout))
                    .then(clone!(requests, |result| {
                        match result {
                            Ok((finished_at, x)) => {
                                requests.write().remove(&request_id);
                                metric!(timer("requests.response_idling") = finished_at.elapsed());
                                Ok(x)
                            }
                            Err(e) => {
                                if let Some(inner) = e.into_inner() {
                                    Err(inner)
                                } else {
                                    Ok(SymbolicationResponse::Pending {
                                        request_id,
                                        // XXX(markus): Probably need a better estimation at some
                                        // point.
                                        retry_after: 30,
                                    })
                                }
                            }
                        }
                    })),
            )
        } else {
            Either::B(rv.then(clone!(requests, |result| {
                requests.write().remove(&request_id);
                let (finished_at, x) = result?;
                metric!(timer("requests.response_idling") = finished_at.elapsed());
                Ok(x)
            })))
        }
    }

    fn create_symbolication_request<F, R>(&self, f: F) -> Result<RequestId, SymbolicationError>
    where
        F: FnOnce() -> R,
        R: Future<Item = CompletedSymbolicationResponse, Error = SymbolicationError> + 'static,
    {
        let request_id = loop {
            let request_id = RequestId::new(uuid::Uuid::new_v4());
            if !self.requests.read().contains_key(&request_id) {
                break request_id;
            }
        };

        let (tx, rx) = oneshot::channel();

        self.requests.write().insert(request_id, rx.shared());

        actix::spawn(
            f().then(move |result| {
                tx.send((
                    Instant::now(),
                    match result {
                        Ok(x) => SymbolicationResponse::Completed(Box::new(x)),
                        Err(ref e) => {
                            capture_fail(e);
                            e.into()
                        }
                    },
                ))
                .map_err(|_| ())
            })
            // Clone hub because of `actix::spawn`
            .sentry_hub_new_from_current(),
        );

        Ok(request_id)
    }
}

fn object_id_from_object_info(object_info: &RawObjectInfo) -> ObjectId {
    ObjectId {
        debug_id: match object_info.debug_id.as_deref() {
            None | Some("") => None,
            Some(string) => string.parse().ok(),
        },
        code_id: match object_info.code_id.as_deref() {
            None | Some("") => None,
            Some(string) => string.parse().ok(),
        },
        debug_file: object_info.debug_file.clone(),
        code_file: object_info.code_file.clone(),
        object_type: object_info.ty,
    }
}

fn get_object_type_from_minidump(minidump_os_name: &str) -> ObjectType {
    match minidump_os_name {
        "Windows" | "Windows NT" => ObjectType::Pe,
        "iOS" | "Mac OS X" => ObjectType::Macho,
        "Linux" | "Solaris" | "Android" => ObjectType::Elf,
        _ => ObjectType::Unknown,
    }
}

fn normalize_minidump_os_name(minidump_os_name: &str) -> &str {
    match minidump_os_name {
        "Windows NT" => "Windows",
        "Mac OS X" => "macOS",
        _ => minidump_os_name,
    }
}

fn object_info_from_minidump_module(ty: ObjectType, module: &CodeModule) -> RawObjectInfo {
    let mut code_id = module.code_identifier();

    // The processor reports an empty string as code id for MachO files
    if ty == ObjectType::Macho && code_id.is_empty() {
        code_id = module.debug_identifier();
        code_id.truncate(code_id.len().max(1) - 1);
    }

    RawObjectInfo {
        ty,
        code_id: Some(code_id),
        code_file: Some(module.code_file()),
        debug_id: Some(module.debug_identifier()),
        debug_file: Some(module.debug_file()),
        image_addr: HexValue(module.base_address()),
        image_size: match module.size() {
            0 => None,
            size => Some(size),
        },
    }
}

pub struct SourceObject(SelfCell<ByteView<'static>, Object<'static>>);

struct SourceLookup {
    inner: Vec<(CompleteObjectInfo, Option<Arc<SourceObject>>)>,
}

impl SourceLookup {
    pub async fn fetch_sources(
        self,
        objects: ObjectsActor,
        scope: Scope,
        sources: Arc<Vec<SourceConfig>>,
        response: &CompletedSymbolicationResponse,
    ) -> Result<Self, SymbolicationError> {
        let mut referenced_objects = BTreeSet::new();
        let stacktraces = &response.stacktraces;

        for stacktrace in stacktraces {
            for frame in &stacktrace.frames {
                if let Some(i) = self.get_object_index_by_addr(frame.raw.instruction_addr.0) {
                    referenced_objects.insert(i);
                }
            }
        }

        let mut futures = Vec::new();

        for (i, (mut object_info, _)) in self.inner.into_iter().enumerate() {
            let is_used = referenced_objects.contains(&i);
            let objects = objects.clone();
            let scope = scope.clone();
            let sources = sources.clone();

            futures.push(async move {
                if !is_used {
                    object_info.debug_status = ObjectFileStatus::Unused;
                    return (object_info, None);
                }

                let opt_object_file_meta = objects
                    .find(FindObject {
                        filetypes: FileType::sources(),
                        purpose: ObjectPurpose::Source,
                        scope: scope.clone(),
                        identifier: object_id_from_object_info(&object_info.raw),
                        sources,
                    })
                    .compat()
                    .await
                    .unwrap_or(None);

                let object_file_opt = match opt_object_file_meta {
                    None => None,
                    Some(object_file_meta) => objects
                        .fetch(object_file_meta)
                        .compat()
                        .await
                        .ok()
                        .and_then(|x| {
                            SelfCell::try_new(x.data(), |b| Object::parse(unsafe { &*b }))
                                .map(|x| Arc::new(SourceObject(x)))
                                .ok()
                        }),
                };

                if object_file_opt.is_some() {
                    object_info.features.has_sources = true;
                }

                (object_info, object_file_opt)
            });
        }

        Ok(SourceLookup {
            inner: futures::future::join_all(futures).await,
        })
    }

    pub fn prepare_debug_sessions(&self) -> Vec<Option<ObjectDebugSession<'_>>> {
        self.inner
            .iter()
            .map(|&(_, ref o)| o.as_ref().and_then(|o| o.0.get().debug_session().ok()))
            .collect()
    }

    pub fn get_context_lines(
        &self,
        debug_sessions: &[Option<ObjectDebugSession<'_>>],
        addr: u64,
        abs_path: &str,
        lineno: u32,
        n: usize,
    ) -> Option<(Vec<String>, String, Vec<String>)> {
        let index = self.get_object_index_by_addr(addr)?;
        let session = debug_sessions[index].as_ref()?;
        let source = session.source_by_path(abs_path).ok()??;

        let lineno = lineno as usize;
        let start_line = lineno.saturating_sub(n);
        let line_diff = lineno - start_line;

        let mut lines = source.lines().skip(start_line);
        let pre_context = (&mut lines)
            .take(line_diff.saturating_sub(1))
            .map(|x| x.to_string())
            .collect();
        let context = lines.next()?.to_string();
        let post_context = lines.take(n).map(|x| x.to_string()).collect();

        Some((pre_context, context, post_context))
    }

    fn get_object_index_by_addr(&self, addr: u64) -> Option<usize> {
        for (i, (info, _)) in self.inner.iter().enumerate() {
            let start_addr = info.raw.image_addr.0;

            if start_addr > addr {
                // The debug image starts at a too high address
                continue;
            }

            let size = info.raw.image_size.unwrap_or(0);
            if let Some(end_addr) = start_addr.checked_add(size) {
                if end_addr < addr && size != 0 {
                    // The debug image ends at a too low address and we're also confident that
                    // end_addr is accurate (size != 0)
                    continue;
                }
            }

            return Some(i);
        }

        None
    }

    fn sort(&mut self) {
        self.inner.sort_by_key(|(info, _)| info.raw.image_addr.0);

        // Ignore the name `dedup_by`, I just want to iterate over consecutive items and update
        // some.
        self.inner.dedup_by(|(ref info2, _), (ref mut info1, _)| {
            // If this underflows we didn't sort properly.
            let size = info2.raw.image_addr.0 - info1.raw.image_addr.0;
            if info1.raw.image_size.unwrap_or(0) == 0 {
                info1.raw.image_size = Some(size);
            }

            false
        });
    }
}

impl FromIterator<CompleteObjectInfo> for SourceLookup {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = CompleteObjectInfo>,
    {
        let mut rv = SourceLookup {
            inner: iter.into_iter().map(|x| (x, None)).collect(),
        };
        rv.sort();
        rv
    }
}

struct SymCacheLookup {
    inner: Vec<(CompleteObjectInfo, Option<Arc<SymCacheFile>>)>,
}

impl FromIterator<CompleteObjectInfo> for SymCacheLookup {
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = CompleteObjectInfo>,
    {
        let mut rv = SymCacheLookup {
            inner: iter.into_iter().map(|x| (x, None)).collect(),
        };
        rv.sort();
        rv
    }
}

impl SymCacheLookup {
    fn sort(&mut self) {
        self.inner.sort_by_key(|(info, _)| info.raw.image_addr.0);

        // Ignore the name `dedup_by`, I just want to iterate over consecutive items and update
        // some.
        self.inner.dedup_by(|(ref info2, _), (ref mut info1, _)| {
            // If this underflows we didn't sort properly.
            let size = info2.raw.image_addr.0 - info1.raw.image_addr.0;
            if info1.raw.image_size.unwrap_or(0) == 0 {
                info1.raw.image_size = Some(size);
            }

            false
        });
    }

    async fn fetch_symcaches(
        self,
        symcache_actor: SymCacheActor,
        request: SymbolicateStacktraces,
    ) -> Result<Self, SymbolicationError> {
        let mut referenced_objects = BTreeSet::new();
        let stacktraces = request.stacktraces;

        for stacktrace in stacktraces {
            for frame in stacktrace.frames {
                if let Some((i, ..)) = self.lookup_symcache(frame.instruction_addr.0) {
                    referenced_objects.insert(i);
                }
            }
        }

        let mut futures = Vec::new();

        for (i, (mut object_info, _)) in self.inner.into_iter().enumerate() {
            let is_used = referenced_objects.contains(&i);
            let sources = request.sources.clone();
            let scope = request.scope.clone();
            let symcache_actor = symcache_actor.clone();

            futures.push(async move {
                if !is_used {
                    object_info.debug_status = ObjectFileStatus::Unused;
                    return (object_info, None);
                }
                let symcache_result = symcache_actor
                    .fetch(FetchSymCache {
                        object_type: object_info.raw.ty,
                        identifier: object_id_from_object_info(&object_info.raw),
                        sources,
                        scope,
                    })
                    .compat()
                    .await;

                let (symcache, status) = match symcache_result {
                    Ok(symcache) => match symcache.parse() {
                        Ok(Some(_)) => (Some(symcache), ObjectFileStatus::Found),
                        Ok(None) => (Some(symcache), ObjectFileStatus::Missing),
                        Err(e) => (None, (&e).into()),
                    },
                    Err(e) => (None, (&*e).into()),
                };

                object_info.arch = Default::default();

                if let Some(ref symcache) = symcache {
                    object_info.arch = symcache.arch();
                    object_info.features.merge(symcache.features());
                }

                object_info.debug_status = status;
                (object_info, symcache)
            });
        }

        Ok(SymCacheLookup {
            inner: futures::future::join_all(futures).await,
        })
    }

    fn lookup_symcache(
        &self,
        addr: u64,
    ) -> Option<(usize, &CompleteObjectInfo, Option<&SymCacheFile>)> {
        for (i, (info, cache)) in self.inner.iter().enumerate() {
            let start_addr = info.raw.image_addr.0;

            if start_addr > addr {
                // The debug image starts at a too high address
                continue;
            }

            let size = info.raw.image_size.unwrap_or(0);
            if let Some(end_addr) = start_addr.checked_add(size) {
                if end_addr < addr && size != 0 {
                    // The debug image ends at a too low address and we're also confident that
                    // end_addr is accurate (size != 0)
                    continue;
                }
            }

            return Some((i, info, cache.as_ref().map(|x| &**x)));
        }

        None
    }
}

fn symbolicate_frame(
    caches: &SymCacheLookup,
    registers: &Registers,
    signal: Option<Signal>,
    frame: &mut RawFrame,
    index: usize,
) -> Result<Vec<SymbolicatedFrame>, FrameStatus> {
    let (object_info, symcache) = match caches.lookup_symcache(frame.instruction_addr.0) {
        Some((_, info, Some(symcache))) => {
            frame.package = info.raw.code_file.clone();
            (info, symcache)
        }
        Some((_, info, None)) => {
            frame.package = info.raw.code_file.clone();
            if info.debug_status == ObjectFileStatus::Malformed {
                return Err(FrameStatus::Malformed);
            } else {
                return Err(FrameStatus::Missing);
            }
        }
        None => return Err(FrameStatus::UnknownImage),
    };

    log::trace!("Loading symcache");
    let symcache = match symcache.parse() {
        Ok(Some(x)) => x,
        Ok(None) => return Err(FrameStatus::Missing),
        Err(_) => return Err(FrameStatus::Malformed),
    };

    let crashing_frame = index == 0;

    let ip_reg = if crashing_frame {
        symcache
            .arch()
            .ip_register_name()
            .and_then(|ip_reg_name| registers.get(ip_reg_name))
            .map(|x| x.0)
    } else {
        None
    };

    let instruction_info = InstructionInfo {
        addr: frame.instruction_addr.0,
        arch: symcache.arch(),
        signal: signal.map(|x| x.0),
        crashing_frame,
        ip_reg,
    };
    let caller_address = instruction_info.caller_address();

    let relative_addr = match caller_address.checked_sub(object_info.raw.image_addr.0) {
        Some(x) => x,
        None => {
            log::warn!("Underflow when trying to subtract image start addr from caller address");
            metric!(counter("relative_addr.underflow") += 1);
            return Err(FrameStatus::MissingSymbol);
        }
    };

    log::trace!("Symbolicating {:#x}", relative_addr);
    let line_infos = match symcache.lookup(relative_addr) {
        Ok(x) => x,
        Err(_) => return Err(FrameStatus::Malformed),
    };

    let mut rv = vec![];

    for line_info in line_infos {
        let line_info = match line_info {
            Ok(x) => x,
            Err(_) => return Err(FrameStatus::Malformed),
        };

        // The logic for filename and abs_path intentionally diverges from how symbolic is used
        // inside of Sentry right now.
        let (filename, abs_path) = {
            let rel_path = line_info.path();
            let abs_path = line_info.abs_path();

            if abs_path == rel_path {
                // rel_path is absolute and therefore not usable for `filename`. Use the
                // basename as filename.
                (line_info.filename().to_owned(), abs_path.to_owned())
            } else {
                // rel_path is relative (probably to the compilation dir) and therefore useful
                // as filename for Sentry.
                (rel_path.to_owned(), abs_path.to_owned())
            }
        };

        let lang = line_info.language();

        rv.push(SymbolicatedFrame {
            status: FrameStatus::Symbolicated,
            original_index: Some(index),
            raw: RawFrame {
                package: object_info.raw.code_file.clone(),
                instruction_addr: HexValue(
                    object_info.raw.image_addr.0 + line_info.instruction_address(),
                ),
                symbol: Some(line_info.symbol().to_string()),
                abs_path: if !abs_path.is_empty() {
                    Some(abs_path)
                } else {
                    frame.abs_path.clone()
                },
                function: Some(
                    line_info
                        .function_name()
                        .try_demangle(DEMANGLE_OPTIONS)
                        .into_owned(),
                ),
                filename: if !filename.is_empty() {
                    Some(filename)
                } else {
                    frame.filename.clone()
                },
                lineno: Some(line_info.line()),
                pre_context: vec![],
                context_line: None,
                post_context: vec![],
                sym_addr: Some(HexValue(
                    object_info.raw.image_addr.0 + line_info.function_address(),
                )),
                lang: if lang != Language::Unknown {
                    Some(lang)
                } else {
                    frame.lang
                },
                trust: frame.trust,
            },
        });
    }

    if rv.is_empty() {
        return Err(FrameStatus::MissingSymbol);
    }

    Ok(rv)
}

fn symbolicate_stacktrace(
    thread: RawStacktrace,
    caches: &SymCacheLookup,
    signal: Option<Signal>,
) -> CompleteStacktrace {
    let mut stacktrace = CompleteStacktrace {
        thread_id: thread.thread_id,
        is_requesting: thread.is_requesting,
        registers: thread.registers.clone(),
        frames: vec![],
    };

    for (index, mut frame) in thread.frames.into_iter().enumerate() {
        match symbolicate_frame(caches, &thread.registers, signal, &mut frame, index) {
            Ok(frames) => stacktrace.frames.extend(frames),
            Err(status) => {
                // Temporary workaround: Skip false-positive frames from stack scanning after the
                // fact.
                //
                // Usually, the stack scanner would skip all scanned frames when it *knows* that
                // they cannot be symbolized. However, in our case we supply breakpad symbols
                // without function records. This breaks its original heuristic, since it would now
                // *always* skip scan frames. Our patch in breakpad omits this check.
                //
                // Here, we fix this after the fact.
                //
                // - MissingSymbol: If symbolication failed for a scanned frame where we *know* we
                //   have a debug info, but the lookup inside that file failed.
                // - UnknownImage: If symbolication failed because the stackscanner found an
                //   instruction_addr that is not in any image *we* consider valid. We discard
                //   images which do not have a debug id, while the stackscanner considers them
                //   perfectly fine.
                if frame.trust == FrameTrust::Scan
                    && (status == FrameStatus::MissingSymbol || status == FrameStatus::UnknownImage)
                {
                    continue;
                }

                let mut demangled_frame = frame.clone();
                if let Some(symbol_string) = frame.function {
                    if symbol_string.starts_with("$s") {
                        let name = Name::with_language(symbol_string.as_str(), Language::Swift);
                        let demanged_name = name.demangle(DEMANGLE_OPTIONS);
                        if let Some(demangled_name) = demanged_name {
                            demangled_frame.function = Some(demangled_name);
                        }
                    }
                }

                stacktrace.frames.push(SymbolicatedFrame {
                    status,
                    original_index: Some(index),
                    raw: demangled_frame,
                });
            }
        }
    }

    stacktrace
}

#[derive(Debug, Clone)]
/// A request for symbolication of multiple stack traces.
pub struct SymbolicateStacktraces {
    /// The scope of this request which determines access to cached files.
    pub scope: Scope,

    /// The signal thrown on certain operating systems.
    ///
    ///  Signal handlers sometimes mess with the runtime stack. This is used to determine whether
    /// the top frame should be fixed or not.
    pub signal: Option<Signal>,

    /// A list of external sources to load debug files.
    pub sources: Arc<Vec<SourceConfig>>,

    /// A list of threads containing stack traces.
    pub stacktraces: Vec<RawStacktrace>,

    /// A list of images that were loaded into the process.
    ///
    /// This list must cover the instruction addresses of the frames in `threads`. If a frame is not
    /// covered by any image, the frame cannot be symbolicated as it is not clear which debug file
    /// to load.
    pub modules: Vec<CompleteObjectInfo>,
}

impl SymbolicationActor {
    fn do_symbolicate(
        &self,
        request: SymbolicateStacktraces,
    ) -> impl Future<Item = CompletedSymbolicationResponse, Error = SymbolicationError> {
        let result = self
            .clone()
            .do_symbolicate_impl(request)
            .boxed_local()
            .compat();
        future_metrics!(
            "symbolicate",
            Some((
                Duration::from_secs(3600),
                SymbolicationErrorKind::Timeout.into()
            )),
            result
        )
    }

    async fn do_symbolicate_impl(
        self,
        request: SymbolicateStacktraces,
    ) -> Result<CompletedSymbolicationResponse, SymbolicationError> {
        let symcache_lookup: SymCacheLookup = request.modules.iter().cloned().collect();
        let source_lookup: SourceLookup = request.modules.iter().cloned().collect();
        let stacktraces = request.stacktraces.clone();
        let sources = request.sources.clone();
        let scope = request.scope.clone();
        let signal = request.signal;

        let symcache_lookup = symcache_lookup
            .fetch_symcaches(self.symcaches, request)
            .await?;

        let future = future::lazy(move || -> Result<_, SymbolicationError> {
            let stacktraces: Vec<_> = stacktraces
                .into_iter()
                .map(|trace| symbolicate_stacktrace(trace, &symcache_lookup, signal))
                .collect();

            let modules: Vec<_> = symcache_lookup
                .inner
                .into_iter()
                .map(|(object_info, _)| {
                    metric!(
                        counter("symbolication.debug_status") += 1,
                        "status" => object_info.debug_status.name()
                    );

                    object_info
                })
                .collect();

            metric!(time_raw("symbolication.num_modules") = modules.len() as u64);
            metric!(time_raw("symbolication.num_stacktraces") = stacktraces.len() as u64);
            metric!(
                time_raw("symbolication.num_frames") =
                    stacktraces.iter().map(|s| s.frames.len() as u64).sum()
            );

            Ok(CompletedSymbolicationResponse {
                signal,
                modules,
                stacktraces,
                ..Default::default()
            })
        });

        let mut response = self
            .threadpool
            .spawn_handle(future.sentry_hub_current().compat())
            .await
            .map_err(|_| SymbolicationErrorKind::Canceled)??;

        let source_lookup = source_lookup
            .fetch_sources(self.objects, scope, sources, &response)
            .await?;

        let future = future::lazy(move || -> Result<_, SymbolicationError> {
            let debug_sessions = source_lookup.prepare_debug_sessions();

            for trace in &mut response.stacktraces {
                for frame in &mut trace.frames {
                    let (abs_path, lineno) = match (&frame.raw.abs_path, frame.raw.lineno) {
                        (&Some(ref abs_path), Some(lineno)) => (abs_path, lineno),
                        _ => continue,
                    };

                    let result = source_lookup.get_context_lines(
                        &debug_sessions,
                        frame.raw.instruction_addr.0,
                        abs_path,
                        lineno,
                        5,
                    );

                    if let Some((pre_context, context_line, post_context)) = result {
                        frame.raw.pre_context = pre_context;
                        frame.raw.context_line = Some(context_line);
                        frame.raw.post_context = post_context;
                    }
                }
            }
            Ok(response)
        });

        let result = self
            .threadpool
            .spawn_handle(future.sentry_hub_current().compat())
            .await
            .map_err(|_| SymbolicationErrorKind::Canceled)??;

        Ok(result)
    }

    pub fn symbolicate_stacktraces(
        &self,
        request: SymbolicateStacktraces,
    ) -> Result<RequestId, SymbolicationError> {
        self.create_symbolication_request(|| self.do_symbolicate(request))
    }
}

/// Status poll request.
#[derive(Clone, Debug)]
pub struct GetSymbolicationStatus {
    /// The identifier of the symbolication task.
    pub request_id: RequestId,
    /// An optional timeout, after which the request will yield a result.
    ///
    /// If this timeout is not set, symbolication will continue until a result is ready (which is
    /// either an error or success). If this timeout is set and no result is ready, a `pending`
    /// status is returned.
    pub timeout: Option<u64>,
}

impl SymbolicationActor {
    pub fn get_symbolication_status(
        &self,
        request: GetSymbolicationStatus,
    ) -> impl Future<Item = Option<SymbolicationResponse>, Error = SymbolicationError> {
        let request_id = request.request_id;

        if let Some(channel) = self.requests.read().get(&request_id) {
            Either::A(
                self.wrap_response_channel(request_id, request.timeout, channel.clone())
                    .map(Some),
            )
        } else {
            // This is okay to occur during deploys, but if it happens all the time we have a state
            // bug somewhere. Could be a misconfigured load balancer (supposed to be pinned to
            // scopes).
            metric!(counter("symbolication.request_id_unknown") += 1);
            Either::B(Ok(None).into_future())
        }
    }
}

type CfiCacheResult = (CodeModuleId, Result<Arc<CfiCacheFile>, Arc<CfiCacheError>>);

#[derive(Debug)]
struct MinidumpState {
    timestamp: u64,
    system_info: SystemInfo,
    crashed: bool,
    crash_reason: String,
    assertion: String,
}

impl MinidumpState {
    fn merge_into(mut self, response: &mut CompletedSymbolicationResponse) {
        if self.system_info.cpu_arch == Arch::Unknown {
            self.system_info.cpu_arch = response
                .modules
                .iter()
                .map(|object| object.arch)
                .find(|arch| *arch != Arch::Unknown)
                .unwrap_or_default();
        }

        response.timestamp = Some(self.timestamp);
        response.system_info = Some(self.system_info);
        response.crashed = Some(self.crashed);
        response.crash_reason = Some(self.crash_reason);
        response.assertion = Some(self.assertion);
    }
}

impl SymbolicationActor {
    fn get_referenced_modules_from_minidump(
        &self,
        minidump: Bytes,
    ) -> ResponseFuture<Vec<(CodeModuleId, RawObjectInfo)>, SymbolicationError> {
        let lazy = future::lazy(move || {
            log::debug!("Processing minidump ({} bytes)", minidump.len());
            metric!(time_raw("minidump.upload.size") = minidump.len() as u64);
            let state = ProcessState::from_minidump(&ByteView::from_slice(&minidump), None)?;

            let os_name = state.system_info().os_name();
            let object_type = get_object_type_from_minidump(&os_name).to_owned();

            let cfi_modules = state
                .referenced_modules()
                .into_iter()
                .filter_map(|code_module| {
                    Some((
                        code_module.id()?,
                        object_info_from_minidump_module(object_type, code_module),
                    ))
                })
                .collect();

            Ok(cfi_modules)
        });

        let future = self
            .threadpool
            .spawn_handle(lazy.sentry_hub_current().compat())
            .boxed_local()
            .compat()
            .map_err(|_| SymbolicationError::from(SymbolicationErrorKind::Canceled))
            .flatten();

        Box::new(future)
    }

    fn load_cfi_caches(
        &self,
        scope: Scope,
        requests: Vec<(CodeModuleId, RawObjectInfo)>,
        sources: Arc<Vec<SourceConfig>>,
    ) -> ResponseFuture<Vec<CfiCacheResult>, SymbolicationError> {
        let cficaches = self.cficaches.clone();

        let futures = requests
            .into_iter()
            .map(move |(code_module_id, object_info)| {
                cficaches
                    .fetch(FetchCfiCache {
                        object_type: object_info.ty,
                        identifier: object_id_from_object_info(&object_info),
                        sources: sources.clone(),
                        scope: scope.clone(),
                    })
                    .then(move |result| Ok((code_module_id, result)).into_future())
                    // Clone hub because of join_all
                    .sentry_hub_new_from_current()
            });

        Box::new(join_all(futures))
    }

    fn stackwalk_minidump_with_cfi(
        &self,
        scope: Scope,
        minidump: Bytes,
        sources: Arc<Vec<SourceConfig>>,
        cfi_results: Vec<CfiCacheResult>,
    ) -> ResponseFuture<(SymbolicateStacktraces, MinidumpState), SymbolicationError> {
        let stackwalk_future = future::lazy(move || {
            let mut frame_info_map = FrameInfoMap::new();
            let mut unwind_statuses = BTreeMap::new();
            let mut object_features = BTreeMap::new();

            for (code_module_id, result) in &cfi_results {
                let cache_file = match result {
                    Ok(x) => x,
                    Err(e) => {
                        log::info!(
                            "Error while fetching cficache: {}",
                            LogError(&ArcFail(e.clone()))
                        );
                        unwind_statuses.insert(code_module_id, (&**e).into());
                        continue;
                    }
                };

                // NB: Always collect features, regardless of whether we fail to parse them or not.
                // This gives users the feedback that information is there but potentially not
                // processable by symbolicator.
                object_features.insert(code_module_id, cache_file.features());

                log::trace!("Loading cficache");
                let cfi_cache = match cache_file.parse() {
                    Ok(Some(x)) => x,
                    Ok(None) => {
                        unwind_statuses.insert(code_module_id, ObjectFileStatus::Missing);
                        continue;
                    }
                    Err(e) => {
                        log::warn!("Error while parsing cficache: {}", LogError(&e));
                        unwind_statuses.insert(code_module_id, (&e).into());
                        continue;
                    }
                };

                unwind_statuses.insert(code_module_id, ObjectFileStatus::Found);
                frame_info_map.insert(code_module_id.clone(), cfi_cache);
            }

            let process_state = ProcessState::from_minidump(
                &ByteView::from_slice(&minidump),
                Some(&frame_info_map),
            )?;

            let minidump_system_info = process_state.system_info();
            let os_name = minidump_system_info.os_name();
            let os_version = minidump_system_info.os_version();
            let os_build = minidump_system_info.os_build();
            let cpu_family = minidump_system_info.cpu_family();
            let cpu_arch = match cpu_family.parse() {
                Ok(arch) => arch,
                Err(_) => {
                    if !cpu_family.is_empty() {
                        let msg = format!("Unknown minidump arch: {}", cpu_family);
                        sentry::capture_message(&msg, sentry::Level::Error);
                    }

                    Default::default()
                }
            };

            let object_type = get_object_type_from_minidump(&os_name).to_owned();

            let modules = process_state
                .modules()
                .into_iter()
                .filter_map(|code_module| {
                    let mut info: CompleteObjectInfo =
                        object_info_from_minidump_module(object_type, code_module).into();

                    let status = unwind_statuses
                        .get(&code_module.id()?)
                        .cloned()
                        .unwrap_or(ObjectFileStatus::Unused);

                    metric!(
                        counter("symbolication.unwind_status") += 1,
                        "status" => status.name()
                    );
                    info.unwind_status = Some(status);

                    let features = object_features
                        .get(&code_module.id()?)
                        .copied()
                        .unwrap_or_default();

                    info.features.merge(features);

                    Some(info)
                })
                .collect();

            let minidump_state = MinidumpState {
                timestamp: process_state.timestamp(),
                system_info: SystemInfo {
                    os_name: normalize_minidump_os_name(&os_name).to_owned(),
                    os_version,
                    os_build,
                    cpu_arch,
                    device_model: String::default(),
                },
                crashed: process_state.crashed(),
                crash_reason: process_state.crash_reason(),
                assertion: process_state.assertion(),
            };

            let requesting_thread_index: Option<usize> =
                process_state.requesting_thread().try_into().ok();

            let threads = process_state.threads();
            let mut stacktraces = Vec::with_capacity(threads.len());
            for (index, thread) in threads.iter().enumerate() {
                let registers = match thread.frames().get(0) {
                    Some(frame) => map_symbolic_registers(frame.registers(cpu_arch)),
                    None => Registers::new(),
                };

                let frames = thread
                    .frames()
                    .iter()
                    .map(|frame| RawFrame {
                        instruction_addr: HexValue(frame.return_address(cpu_arch)),
                        package: frame.module().map(CodeModule::code_file),
                        trust: frame.trust(),
                        ..RawFrame::default()
                    })
                    // Trim infinite recursions explicitly because those do not
                    // correlate to minidump size. Every other kind of bloated
                    // input data we know is already trimmed/rejected by raw
                    // byte size alone.
                    .take(20000)
                    .collect();

                stacktraces.push(RawStacktrace {
                    is_requesting: requesting_thread_index.map(|r| r == index),
                    thread_id: Some(thread.thread_id().into()),
                    registers,
                    frames,
                });
            }

            let request = SymbolicateStacktraces {
                modules,
                scope,
                sources,
                signal: None,
                stacktraces,
            };

            Ok((request, minidump_state))
        });

        let future = self
            .threadpool
            .spawn_handle(stackwalk_future.sentry_hub_current().compat())
            .boxed_local()
            .compat()
            .map_err(|_| SymbolicationError::from(SymbolicationErrorKind::Canceled))
            .flatten();

        Box::new(future)
    }

    fn do_stackwalk_minidump(
        self,
        scope: Scope,
        minidump: Bytes,
        sources: Vec<SourceConfig>,
    ) -> impl futures::Future<Output = Result<(SymbolicateStacktraces, MinidumpState), SymbolicationError>>
    {
        future_metrics!(
            "minidump_stackwalk",
            Some((
                Duration::from_secs(1200),
                SymbolicationErrorKind::Timeout.into()
            )),
            async move {
                let sources = Arc::new(sources);

                let referenced_modules = self
                    .get_referenced_modules_from_minidump(minidump.clone())
                    .compat()
                    .await?;

                let cfi_caches = self
                    .load_cfi_caches(scope.clone(), referenced_modules, sources.clone())
                    .compat()
                    .await?;

                self.stackwalk_minidump_with_cfi(scope, minidump, sources, cfi_caches)
                    .compat()
                    .await
            }
            .boxed_local()
            .compat()
        )
        .compat()
    }

    async fn do_process_minidump(
        self,
        scope: Scope,
        minidump: Bytes,
        sources: Vec<SourceConfig>,
    ) -> Result<CompletedSymbolicationResponse, SymbolicationError> {
        let (request, state) = self
            .clone()
            .do_stackwalk_minidump(scope, minidump, sources)
            .await?;

        let mut response = self.do_symbolicate(request).compat().await?;
        state.merge_into(&mut response);

        Ok(response)
    }

    pub fn process_minidump(
        &self,
        scope: Scope,
        minidump: Bytes,
        sources: Vec<SourceConfig>,
    ) -> Result<RequestId, SymbolicationError> {
        self.create_symbolication_request(|| {
            self.clone()
                .do_process_minidump(scope, minidump, sources)
                .boxed_local()
                .compat()
        })
    }
}

#[derive(Debug)]
struct AppleCrashReportState {
    timestamp: Option<u64>,
    system_info: SystemInfo,
    crash_reason: Option<String>,
    crash_details: Option<String>,
}

impl AppleCrashReportState {
    fn merge_into(mut self, response: &mut CompletedSymbolicationResponse) {
        if self.system_info.cpu_arch == Arch::Unknown {
            self.system_info.cpu_arch = response
                .modules
                .iter()
                .map(|object| object.arch)
                .find(|arch| *arch != Arch::Unknown)
                .unwrap_or_default();
        }

        response.timestamp = self.timestamp;
        response.system_info = Some(self.system_info);
        response.crash_reason = self.crash_reason;
        response.crash_details = self.crash_details;
        response.crashed = Some(true);
    }
}

fn map_apple_binary_image(image: apple_crash_report_parser::BinaryImage) -> CompleteObjectInfo {
    let code_id = CodeId::from_binary(&image.uuid.as_bytes()[..]);
    let debug_id = DebugId::from_uuid(image.uuid);

    let raw_info = RawObjectInfo {
        ty: ObjectType::Macho,
        code_id: Some(code_id.to_string()),
        code_file: Some(image.path.clone()),
        debug_id: Some(debug_id.to_string()),
        debug_file: Some(image.path),
        image_addr: HexValue(image.addr.0),
        image_size: match image.size {
            0 => None,
            size => Some(size),
        },
    };

    raw_info.into()
}

impl SymbolicationActor {
    fn parse_apple_crash_report(
        &self,
        scope: Scope,
        minidump: Bytes,
        sources: Vec<SourceConfig>,
    ) -> ResponseFuture<(SymbolicateStacktraces, AppleCrashReportState), SymbolicationError> {
        let parse_future = future::lazy(move || {
            let report = AppleCrashReport::from_reader(minidump.into_buf())?;
            let mut metadata = report.metadata;

            let arch = report
                .code_type
                .as_ref()
                .and_then(|code_type| code_type.split(' ').next())
                .and_then(|word| word.parse().ok())
                .unwrap_or_default();

            let modules = report
                .binary_images
                .into_iter()
                .map(map_apple_binary_image)
                .collect();

            let mut stacktraces = Vec::with_capacity(report.threads.len());

            for thread in report.threads {
                let registers = thread
                    .registers
                    .unwrap_or_default()
                    .into_iter()
                    .map(|(name, addr)| (name, HexValue(addr.0)))
                    .collect();

                let frames = thread
                    .frames
                    .into_iter()
                    .map(|frame| RawFrame {
                        instruction_addr: HexValue(frame.instruction_addr.0),
                        package: frame.module,
                        ..RawFrame::default()
                    })
                    .collect();

                stacktraces.push(RawStacktrace {
                    thread_id: Some(thread.id),
                    is_requesting: Some(thread.crashed),
                    registers,
                    frames,
                });
            }

            let request = SymbolicateStacktraces {
                modules,
                scope,
                sources: Arc::new(sources),
                signal: None,
                stacktraces,
            };

            let mut system_info = SystemInfo {
                os_name: metadata.remove("OS Version").unwrap_or_default(),
                device_model: metadata.remove("Hardware Model").unwrap_or_default(),
                cpu_arch: arch,
                ..SystemInfo::default()
            };

            if let Some(captures) = OS_MACOS_REGEX.captures(&system_info.os_name) {
                system_info.os_version = captures
                    .name("version")
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
                system_info.os_build = captures
                    .name("build")
                    .map(|m| m.as_str().to_string())
                    .unwrap_or_default();
                system_info.os_name = "macOS".to_string();
            }

            // https://developer.apple.com/library/archive/technotes/tn2151/_index.html
            let crash_reason = metadata.remove("Exception Type");
            let crash_details = report
                .application_specific_information
                .or_else(|| metadata.remove("Exception Message"))
                .or_else(|| metadata.remove("Exception Subtype"))
                .or_else(|| metadata.remove("Exception Codes"));

            let state = AppleCrashReportState {
                timestamp: report.timestamp.map(|t| t.timestamp() as u64),
                system_info,
                crash_reason,
                crash_details,
            };

            Ok((request, state))
        });

        let request_future = self
            .threadpool
            .spawn_handle(parse_future.sentry_hub_current().compat())
            .boxed_local()
            .compat()
            .map_err(|_| SymbolicationError::from(SymbolicationErrorKind::Canceled))
            .flatten();

        Box::new(future_metrics!(
            "minidump_stackwalk",
            Some((
                Duration::from_secs(1200),
                SymbolicationErrorKind::Timeout.into()
            )),
            request_future,
        ))
    }

    async fn do_process_apple_crash_report(
        self,
        scope: Scope,
        report: Bytes,
        sources: Vec<SourceConfig>,
    ) -> Result<CompletedSymbolicationResponse, SymbolicationError> {
        let (request, state) = self
            .parse_apple_crash_report(scope, report, sources)
            .compat()
            .await?;
        let mut response = self.do_symbolicate(request).compat().await?;

        state.merge_into(&mut response);
        Ok(response)
    }

    pub fn process_apple_crash_report(
        &self,
        scope: Scope,
        apple_crash_report: Bytes,
        sources: Vec<SourceConfig>,
    ) -> Result<RequestId, SymbolicationError> {
        self.create_symbolication_request(|| {
            self.clone()
                .do_process_apple_crash_report(scope, apple_crash_report, sources)
                .boxed_local()
                .compat()
        })
    }
}

fn map_symbolic_registers(x: BTreeMap<&'_ str, RegVal>) -> BTreeMap<String, HexValue> {
    x.into_iter()
        .map(|(register, value)| {
            (
                register.to_owned(),
                HexValue(match value {
                    RegVal::U32(x) => x.into(),
                    RegVal::U64(x) => x,
                }),
            )
        })
        .collect()
}

impl From<&CfiCacheError> for ObjectFileStatus {
    fn from(e: &CfiCacheError) -> ObjectFileStatus {
        match e.kind() {
            CfiCacheErrorKind::Fetching => ObjectFileStatus::FetchingFailed,
            // nb: Timeouts during download are also caught by Fetching
            CfiCacheErrorKind::Timeout => ObjectFileStatus::Timeout,
            CfiCacheErrorKind::ObjectParsing => ObjectFileStatus::Malformed,

            _ => {
                // Just in case we didn't handle an error properly,
                // capture it here. If an error was captured with
                // `capture_fail` further down in the callstack, it
                // should be explicitly handled here as a
                // SymCacheErrorKind variant.
                capture_fail(e);
                ObjectFileStatus::Other
            }
        }
    }
}

impl From<&SymCacheError> for ObjectFileStatus {
    fn from(e: &SymCacheError) -> ObjectFileStatus {
        match e.kind() {
            SymCacheErrorKind::Fetching => ObjectFileStatus::FetchingFailed,
            // nb: Timeouts during download are also caught by Fetching
            SymCacheErrorKind::Timeout => ObjectFileStatus::Timeout,
            SymCacheErrorKind::ObjectParsing => ObjectFileStatus::Malformed,
            _ => {
                // Just in case we didn't handle an error properly,
                // capture it here. If an error was captured with
                // `capture_fail` further down in the callstack, it
                // should be explicitly handled here as a
                // SymCacheErrorKind variant.
                capture_fail(e);
                ObjectFileStatus::Other
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    use failure::Error;

    use crate::app::ServiceState;
    use crate::config::Config;
    use crate::test;

    /// Setup tests and create a test service.
    ///
    /// This function returns a tuple containing the service to test, and a temporary cache
    /// directory. The directory is cleaned up when the `TempDir` instance is dropped. Keep it as
    /// guard until the test has finished.
    ///
    /// The service is configured with `connect_to_reserved_ips = True`. This allows to use a local
    /// symbol server to test object file downloads.
    fn setup_service() -> (ServiceState, test::TempDir) {
        test::setup();

        let cache_dir = test::tempdir();

        let mut config = Config::default();
        config.cache_dir = Some(cache_dir.path().to_owned());
        config.connect_to_reserved_ips = true;
        let service = ServiceState::create(config);

        (service, cache_dir)
    }

    fn get_symbolication_request(sources: Vec<SourceConfig>) -> SymbolicateStacktraces {
        SymbolicateStacktraces {
            scope: Scope::Global,
            signal: None,
            sources: Arc::new(sources),
            stacktraces: vec![RawStacktrace {
                frames: vec![RawFrame {
                    instruction_addr: HexValue(0x1_0000_0fa0),
                    ..RawFrame::default()
                }],
                ..RawStacktrace::default()
            }],
            modules: vec![CompleteObjectInfo::from(RawObjectInfo {
                ty: ObjectType::Macho,
                code_id: Some("502fc0a51ec13e479998684fa139dca7".to_owned().to_lowercase()),
                debug_id: Some("502fc0a5-1ec1-3e47-9998-684fa139dca7".to_owned()),
                image_addr: HexValue(0x1_0000_0000),
                image_size: Some(4096),
                code_file: None,
                debug_file: None,
            })],
        }
    }

    fn get_symbolication_response(
        service: &ServiceState,
        request_id: RequestId,
    ) -> Result<SymbolicationResponse, Error> {
        let response_opt = test::block_fn(|| {
            service
                .symbolication()
                .get_symbolication_status(GetSymbolicationStatus {
                    request_id,
                    timeout: None,
                })
        })?;

        response_opt.ok_or_else(|| failure::err_msg("missing response"))
    }

    #[test]
    fn test_remove_bucket() -> Result<(), Error> {
        // Test with sources first, and then without. This test should verify that we do not leak
        // cached debug files to requests that no longer specify a source.

        let (service, _cache_dir) = setup_service();
        let (_symsrv, source) = test::symbol_server();

        let request = get_symbolication_request(vec![source]);
        let request_id = service.symbolication().symbolicate_stacktraces(request)?;
        let response = get_symbolication_response(&service, request_id)?;
        insta::assert_yaml_snapshot!(response);

        let request = get_symbolication_request(vec![]);
        let request_id = service.symbolication().symbolicate_stacktraces(request)?;
        let response = get_symbolication_response(&service, request_id)?;
        insta::assert_yaml_snapshot!(response);

        Ok(())
    }

    #[test]
    fn test_add_bucket() -> Result<(), Error> {
        // Test without sources first, then with. This test should verify that we apply a new source
        // to requests immediately.

        let (service, _cache_dir) = setup_service();
        let (_symsrv, source) = test::symbol_server();

        let request = get_symbolication_request(vec![]);
        let request_id = service.symbolication().symbolicate_stacktraces(request)?;
        let response = get_symbolication_response(&service, request_id)?;
        insta::assert_yaml_snapshot!(response);

        let request = get_symbolication_request(vec![source]);
        let request_id = service.symbolication().symbolicate_stacktraces(request)?;
        let response = get_symbolication_response(&service, request_id)?;
        insta::assert_yaml_snapshot!(response);

        Ok(())
    }

    fn stackwalk_minidump(path: &str) -> Result<(), Error> {
        let (service, _cache_dir) = setup_service();
        let (_symsrv, source) = test::symbol_server();

        let request_id = service.symbolication().process_minidump(
            Scope::Global,
            Bytes::from(fs::read(path)?),
            vec![source],
        )?;

        let response = get_symbolication_response(&service, request_id)?;
        insta::assert_yaml_snapshot!(response);

        let global_dir = service.config().cache_dir("object_meta/global").unwrap();
        let mut cache_entries: Vec<_> = fs::read_dir(global_dir)?
            .map(|x| x.unwrap().file_name().into_string().unwrap())
            .collect();

        cache_entries.sort();
        insta::assert_yaml_snapshot!(cache_entries);

        Ok(())
    }

    #[test]
    fn test_minidump_windows() -> Result<(), Error> {
        stackwalk_minidump("./tests/fixtures/windows.dmp")
    }

    #[test]
    fn test_minidump_macos() -> Result<(), Error> {
        stackwalk_minidump("./tests/fixtures/macos.dmp")
    }

    #[test]
    fn test_minidump_linux() -> Result<(), Error> {
        stackwalk_minidump("./tests/fixtures/linux.dmp")
    }

    #[test]
    fn test_apple_crash_report() -> Result<(), Error> {
        let (service, _cache_dir) = setup_service();
        let (_symsrv, source) = test::symbol_server();

        let report_file = Bytes::from(fs::read("./tests/fixtures/apple_crash_report.txt")?);
        let request_id = service.symbolication().process_apple_crash_report(
            Scope::Global,
            report_file,
            vec![source],
        )?;

        let response = get_symbolication_response(&service, request_id)?;
        insta::assert_yaml_snapshot!(response);

        Ok(())
    }

    #[test]
    fn test_symcache_lookup_open_end_addr() {
        test::setup();

        // The Rust SDK and some other clients sometimes send zero-sized images when no end addr
        // could be determined. Symbolicator should still resolve such images.
        let info = CompleteObjectInfo::from(RawObjectInfo {
            ty: ObjectType::Unknown,
            code_id: None,
            debug_id: None,
            code_file: None,
            debug_file: None,
            image_addr: HexValue(42),
            image_size: Some(0),
        });

        let lookup = SymCacheLookup::from_iter(vec![info.clone()]);

        let (a, b, c) = lookup.lookup_symcache(43).unwrap();
        assert_eq!(a, 0);
        assert_eq!(b, &info);
        assert!(c.is_none());
    }
}
