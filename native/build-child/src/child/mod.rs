//! The V8 child of one `build` call (`src/main.rs` runs it).
//!
//! `server/build-child.js` and the four modules it loads run unchanged in an
//! embedded V8 isolate; `prelude.js` supplies the Node facilities they touch
//! and this file the host functions under it. The `vm` module is V8
//! contexts, as Node's is ([`contextify`] ports Node's global interceptors),
//! and the error decoration (`build.js:N\n<line>\n   ^\n\n` ahead of the
//! stack) is a port of Node's `node_errors.cc` `GetErrorSource`,
//! `AppendExceptionLine` and `DecorateErrorStack`. stdin, stdout, stderr
//! and the exit status are `build-child.js`'s.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

mod contextify;

use mcbench_build::HEAP_LIMIT_MB;

const PRELUDE: &str = include_str!("prelude.js");

/// The child's JavaScript, embedded from `server/`: these five files are what
/// models' programs run on (see the crate doc).
const SOURCES: [(&str, &str); 5] = [
    (
        "build-child.js",
        include_str!("../../../../server/build-child.js"),
    ),
    ("mc-dsl.js", include_str!("../../../../server/mc-dsl.js")),
    ("world.js", include_str!("../../../../server/world.js")),
    (
        "minecraft-dimension.js",
        include_str!("../../../../server/minecraft-dimension.js"),
    ),
    (
        "build-transfer.js",
        include_str!("../../../../server/build-transfer.js"),
    ),
];

/// The last line Node prints after an uncaught exception, which the parent
/// reads as the crash's detail. The literal of the Node the reference results
/// were taken with (docs/native-app.md, Fidelity).
const NODE_VERSION_LINE: &str = "Node.js v26.13.1";

/// The most bytes of array buffers alive at once: the heap cap plus 2 GiB,
/// `build-sandbox.js`'s `DATA_LIMIT_BYTES`, which Linux enforces for the
/// whole process with `RLIMIT_DATA`. Counted here on every platform, so on
/// macOS (which has no data limit) a huge typed array fails with V8's
/// "Array buffer allocation failed" as it does on Linux.
const ARRAY_BUFFER_LIMIT: usize = ((HEAP_LIMIT_MB + 2048) * 1024 * 1024) as usize;

struct State {
    contexts: Vec<v8::Global<v8::Context>>,
    scripts: Vec<v8::Global<v8::UnboundScript>>,
    arrow: v8::Global<v8::Private>,
    decorated: v8::Global<v8::Private>,
}

fn throw_error(scope: &mut v8::PinScope, message: &str) {
    let text = v8::String::new(scope, message).unwrap();
    let error = v8::Exception::error(scope, text);
    scope.throw_exception(error);
}

/// The bytes of a Uint8Array argument, as a mutable slice of its backing
/// store.
fn bytes_of<'a>(scope: &mut v8::PinScope, value: v8::Local<v8::Value>) -> &'a mut [u8] {
    let view: v8::Local<v8::ArrayBufferView> = value.try_into().expect("a Uint8Array");
    let buffer = view.buffer(scope).expect("a buffer");
    let length = view.byte_length();
    if length == 0 {
        return &mut [];
    }
    let base = buffer.data().expect("backing store").as_ptr() as *mut u8;
    // SAFETY: the view's bytes lie inside its buffer's backing store, which
    // the view keeps alive while the host function that asked runs; the
    // slice does not outlive that call, and JavaScript cannot run (and so
    // cannot detach or touch the buffer) until it returns.
    unsafe { std::slice::from_raw_parts_mut(base.add(view.byte_offset()), length) }
}

fn int_arg(scope: &mut v8::PinScope, args: &v8::FunctionCallbackArguments, index: i32) -> usize {
    args.get(index).integer_value(scope).unwrap() as usize
}

/// `fs.readSync(0, ...)`.
fn host_read(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let bytes = bytes_of(scope, args.get(0));
    let offset = int_arg(scope, &args, 1);
    let length = int_arg(scope, &args, 2);
    let n = std::io::stdin()
        .lock()
        .read(&mut bytes[offset..offset + length])
        .expect("read stdin");
    rv.set_uint32(n as u32);
}

/// `fs.writeSync(1, ...)`.
fn host_write(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let bytes = bytes_of(scope, args.get(0));
    let offset = int_arg(scope, &args, 1);
    let mut out = std::io::stdout().lock();
    let n = out.write(&bytes[offset..]).expect("write stdout");
    out.flush().expect("write stdout");
    rv.set_uint32(n as u32);
}

fn host_exit(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    _rv: v8::ReturnValue<v8::Value>,
) {
    let code = args.get(0).int32_value(scope).unwrap_or(0);
    std::process::exit(code);
}

fn host_utf8_encode(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let text = args
        .get(0)
        .to_string(scope)
        .unwrap()
        .to_rust_string_lossy(scope)
        .into_bytes();
    let store = v8::ArrayBuffer::new_backing_store_from_vec(text).make_shared();
    rv.set(v8::ArrayBuffer::with_backing_store(scope, &store).into());
}

fn host_utf8_decode(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let bytes = bytes_of(scope, args.get(0));
    let start = int_arg(scope, &args, 1);
    let end = int_arg(scope, &args, 2);
    let text =
        v8::String::new_from_utf8(scope, &bytes[start..end], v8::NewStringType::Normal).unwrap();
    rv.set(text.into());
}

fn origin<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    filename: v8::Local<'s, v8::Value>,
) -> v8::ScriptOrigin<'s> {
    v8::ScriptOrigin::new(
        scope, filename, 0, 0, false, 0, None, false, false, false, None,
    )
}

/// The CommonJS wrapper of one of the five files.
fn host_compile_function<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let code = args.get(0).to_rust_string_lossy(scope);
    let wrapped = v8::String::new(
        scope,
        &format!("(function (exports, require, module, __filename, __dirname) {{{code}\n}})"),
    )
    .unwrap();
    let origin = origin(scope, args.get(1));
    let script = v8::Script::compile(scope, wrapped, Some(&origin)).expect("host module compiles");
    if let Some(function) = script.run(scope) {
        rv.set(function);
    }
}

/// `vm.createContext(sandbox)`: a context whose global is Node's
/// contextified global over `sandbox` (contextify.rs).
fn host_create_context(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let current = scope.get_current_context();
    let sandbox: v8::Local<v8::Object> = args
        .get(0)
        .try_into()
        .expect("vm.createContext takes an object");
    // Leaked: a build process makes one context and exits.
    let cell: &'static contextify::VmCell = Box::leak(Box::new(contextify::VmCell {
        sandbox: Default::default(),
        global: Default::default(),
    }));
    let global_template = Some(contextify::global_template(scope, cell));
    let context = v8::Context::new(
        scope,
        v8::ContextOptions {
            global_template,
            ..Default::default()
        },
    );
    // Node gives every vm context its creator's security token, so objects
    // pass between them freely.
    context.set_security_token(current.get_security_token(scope));
    let global = context.global(scope);
    let _ = cell.sandbox.set(v8::Global::new(scope, sandbox));
    let _ = cell.global.set(v8::Global::new(scope, global));
    let global = v8::Global::new(scope, context);
    let state = scope.get_slot_mut::<State>().unwrap();
    state.contexts.push(global);
    rv.set_uint32((state.contexts.len() - 1) as u32);
}

/// `node_errors.cc` `GetErrorSource`: "file:line\n<source line>\n<underline>\n",
/// the line as UTF-8 indexed by V8's (UTF-16) columns, as Node does.
fn error_source(scope: &mut v8::PinScope, message: v8::Local<v8::Message>) -> Option<String> {
    let line = message.get_source_line(scope)?;
    let sourceline = line.to_rust_string_lossy(scope).into_bytes();
    if String::from_utf8_lossy(&sourceline).contains("node-do-not-add-exception-line") {
        return None;
    }
    let filename = message
        .get_script_resource_name(scope)
        .map(|name| name.to_rust_string_lossy(scope))
        .unwrap_or_default();
    let linenum = message.get_line_number(scope).unwrap_or(0);
    let (start, end) = (
        message.get_start_column() as i64,
        message.get_end_column() as i64,
    );
    // SPrintF's %s stops at a NUL.
    let shown = sourceline.split(|&b| b == 0).next().unwrap();
    let mut buf = format!("{filename}:{linenum}\n{}\n", String::from_utf8_lossy(shown));
    if start > end || start < 0 || end as usize > sourceline.len() {
        return Some(buf);
    }
    let mut underline = Vec::new();
    for &byte in &sourceline[..start as usize] {
        if byte == 0 || underline.len() >= 1020 {
            break;
        }
        underline.push(if byte == b'\t' { b'\t' } else { b' ' });
    }
    for &byte in &sourceline[start as usize..end as usize] {
        if byte == 0 || underline.len() >= 1020 {
            break;
        }
        underline.push(b'^');
    }
    underline.push(b'\n');
    buf.push_str(&String::from_utf8_lossy(&underline));
    Some(buf)
}

fn utf16(scope: &mut v8::PinScope, text: v8::Local<v8::String>) -> Vec<u16> {
    let mut units = vec![0u16; text.length()];
    text.write_v2(scope, 0, &mut units, v8::WriteFlags::empty());
    units
}

/// `node_errors.cc` `DecorateErrorStack` with `AppendExceptionLine`
/// (`CONTEXTIFY_ERROR`).
fn decorate(
    scope: &mut v8::PinScope,
    exception: v8::Local<v8::Value>,
    message: Option<v8::Local<v8::Message>>,
) {
    let Ok(object) = v8::Local::<v8::Object>::try_from(exception) else {
        return;
    };
    let state = scope.get_slot::<State>().unwrap();
    let (arrow_key, decorated_key) = (state.arrow.clone(), state.decorated.clone());
    let arrow_key = v8::Local::new(scope, arrow_key);
    let decorated_key = v8::Local::new(scope, decorated_key);
    if object
        .get_private(scope, decorated_key)
        .is_some_and(|value| value.is_true())
    {
        return;
    }
    if let Some(message) = message {
        let existing = object.get_private(scope, arrow_key);
        if existing.is_some_and(|value| !value.is_string())
            && let Some(source) = error_source(scope, message)
        {
            let text = v8::String::new(scope, &source).unwrap();
            object.set_private(scope, arrow_key, text.into());
        }
    }
    v8::tc_scope!(let tc, scope);
    let stack_key = v8::String::new(tc, "stack").unwrap();
    let stack = object.get(tc, stack_key.into());
    let arrow = object.get_private(tc, arrow_key);
    let (Some(stack), Some(arrow)) = (stack, arrow) else {
        return;
    };
    if !arrow.is_string() || !stack.is_string() {
        return;
    }
    // String::Concat(arrow, "\n", stack), in UTF-16 so nothing is re-encoded.
    let mut units = utf16(tc, arrow.try_into().unwrap());
    units.push(b'\n' as u16);
    units.extend(utf16(tc, stack.try_into().unwrap()));
    let decorated = v8::String::new_from_two_byte(tc, &units, v8::NewStringType::Normal).unwrap();
    object.set(tc, stack_key.into(), decorated.into());
    let yes = v8::Boolean::new(tc, true);
    object.set_private(tc, decorated_key, yes.into());
}

/// `new vm.Script(code, { filename })`: compiles, or throws Node's decorated
/// SyntaxError.
fn host_compile<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: v8::FunctionCallbackArguments<'s>,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let code = args.get(0).to_string(scope).unwrap();
    let filename = args.get(1);
    v8::tc_scope!(let tc, scope);
    let origin = origin(tc, filename);
    let mut source = v8::script_compiler::Source::new(code, Some(&origin));
    let compiled = v8::script_compiler::compile_unbound_script(
        tc,
        &mut source,
        v8::script_compiler::CompileOptions::NoCompileOptions,
        v8::script_compiler::NoCacheReason::NoReason,
    );
    match compiled {
        Some(script) => {
            let global = v8::Global::new(tc, script);
            let state = tc.get_slot_mut::<State>().unwrap();
            state.scripts.push(global);
            rv.set_uint32((state.scripts.len() - 1) as u32);
        }
        None => {
            let exception = tc.exception().unwrap();
            let message = tc.message();
            decorate(tc, exception, message);
            tc.rethrow();
        }
    }
}

/// The `timeout` of `runInContext`: (finished, timed out), and the watchdog
/// thread's wake-up.
struct Watchdog {
    done: Mutex<(bool, bool)>,
    wake: Condvar,
}

/// `script.runInContext(context, { timeout })`: runs with Node's watchdog
/// and throws Node's timeout error past it.
fn host_run(
    scope: &mut v8::PinScope,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) {
    let script_index = int_arg(scope, &args, 0);
    let context_index = int_arg(scope, &args, 1);
    let timeout = if args.get(2).is_undefined() {
        None
    } else {
        Some(args.get(2).integer_value(scope).unwrap() as u64)
    };
    let state = scope.get_slot::<State>().unwrap();
    let (script, context) = (
        state.scripts[script_index].clone(),
        state.contexts[context_index].clone(),
    );
    let script = v8::Local::new(scope, script);
    let context = v8::Local::new(scope, context);

    let watchdog = Arc::new(Watchdog {
        done: Mutex::new((false, false)),
        wake: Condvar::new(),
    });
    let thread = timeout.map(|ms| {
        let handle = scope.thread_safe_handle();
        let watchdog = watchdog.clone();
        std::thread::spawn(move || {
            let guard = watchdog.done.lock().unwrap();
            let (mut guard, result) = watchdog
                .wake
                .wait_timeout_while(guard, Duration::from_millis(ms), |(done, _)| !*done)
                .unwrap();
            if result.timed_out() && !guard.0 {
                guard.1 = true;
                handle.terminate_execution();
            }
        })
    });

    let outcome = {
        let inner = &mut v8::ContextScope::new(scope, context);
        v8::tc_scope!(let tc, inner);
        let result = {
            let bound = script.bind_to_current_context(tc);
            bound.run(tc)
        };
        if let Some(thread) = thread {
            watchdog.done.lock().unwrap().0 = true;
            watchdog.wake.notify_all();
            thread.join().unwrap();
        }
        let timed_out = watchdog.done.lock().unwrap().1;
        match result {
            Some(value) if !timed_out => Ok(value),
            _ if timed_out => Err(None),
            _ => {
                if tc.has_terminated() {
                    Err(None)
                } else {
                    let exception = tc.exception().unwrap();
                    let message = tc.message();
                    decorate(tc, exception, message);
                    Err(Some(v8::Global::new(tc, exception)))
                }
            }
        }
    };
    match outcome {
        Ok(value) => rv.set(value),
        Err(Some(exception)) => {
            let exception = v8::Local::new(scope, exception);
            scope.throw_exception(exception);
        }
        Err(None) => {
            scope.cancel_terminate_execution();
            throw_error(
                scope,
                &format!("Script execution timed out after {}ms", timeout.unwrap()),
            );
        }
    }
}

/// Node's wording (`node_errors.cc` `OOMErrorHandler`), which the parent
/// reads as "ran out of memory" (`mcbench_build::runner`'s `crash_error`
/// reads the text, not how the process ended), then the process ends at
/// once with [`OOM_STATUS`]. Not `abort()`, as Node does: a model's program
/// that fills its heap is an expected end here, and an abort is a crash the
/// system's reporter records (on macOS one report per program in the
/// person's `~/Library/Logs/DiagnosticReports`). `_exit` runs no exit
/// handlers, as an abort would not, so nothing of V8's state is touched.
unsafe extern "C" fn on_oom(location: *const std::os::raw::c_char, _details: &v8::OomDetails) {
    // SAFETY: V8 passes a NUL-terminated string that lives for this call.
    let location = unsafe { std::ffi::CStr::from_ptr(location) }.to_string_lossy();
    let _ = writeln!(
        std::io::stderr(),
        "FATAL ERROR: {location} Allocation failed - JavaScript heap out of memory"
    );
    // SAFETY: `_exit` takes a status and ends the process; it never returns.
    unsafe { libc::_exit(OOM_STATUS) }
}

/// The out-of-memory end's exit status: 134, what a shell reports for
/// Node's abort (128 + SIGABRT), for anyone reading the status by hand.
pub const OOM_STATUS: std::os::raw::c_int = 134;

/// Node's array-buffer allocator (`calloc`, `malloc`, `free`; a failed
/// allocation is a null pointer, which V8 turns into "Array buffer
/// allocation failed"), plus the count of live bytes against
/// [`ARRAY_BUFFER_LIMIT`].
struct ArrayBuffers {
    live: AtomicUsize,
}

impl ArrayBuffers {
    fn reserve(&self, len: usize) -> bool {
        let before = self.live.fetch_add(len, Ordering::Relaxed);
        if before.saturating_add(len) > ARRAY_BUFFER_LIMIT {
            self.live.fetch_sub(len, Ordering::Relaxed);
            return false;
        }
        true
    }
}

unsafe extern "C" fn buffer_allocate(handle: &ArrayBuffers, len: usize) -> *mut std::ffi::c_void {
    if !handle.reserve(len) {
        return std::ptr::null_mut();
    }
    // SAFETY: calloc has no preconditions; a null result is handed to V8.
    let data = unsafe { libc::calloc(len.max(1), 1) };
    if data.is_null() {
        handle.live.fetch_sub(len, Ordering::Relaxed);
    }
    data
}

unsafe extern "C" fn buffer_allocate_uninitialized(
    handle: &ArrayBuffers,
    len: usize,
) -> *mut std::ffi::c_void {
    if !handle.reserve(len) {
        return std::ptr::null_mut();
    }
    // SAFETY: malloc has no preconditions; a null result is handed to V8.
    let data = unsafe { libc::malloc(len.max(1)) };
    if data.is_null() {
        handle.live.fetch_sub(len, Ordering::Relaxed);
    }
    data
}

unsafe extern "C" fn buffer_free(handle: &ArrayBuffers, data: *mut std::ffi::c_void, len: usize) {
    handle.live.fetch_sub(len, Ordering::Relaxed);
    // SAFETY: V8 frees only what buffer_allocate or
    // buffer_allocate_uninitialized returned, once.
    unsafe { libc::free(data) };
}

unsafe extern "C" fn buffer_drop(_handle: *const ArrayBuffers) {
    // The handle is a leaked static; the process exits with it.
}

static ARRAY_BUFFERS: ArrayBuffers = ArrayBuffers {
    live: AtomicUsize::new(0),
};

static ARRAY_BUFFER_VTABLE: v8::RustAllocatorVtable<ArrayBuffers> = v8::RustAllocatorVtable {
    allocate: buffer_allocate,
    allocate_uninitialized: buffer_allocate_uninitialized,
    free: buffer_free,
    drop: buffer_drop,
};

fn install(
    scope: &mut v8::PinScope,
    host: v8::Local<v8::Object>,
    name: &str,
    callback: impl v8::MapFnTo<v8::FunctionCallback>,
) {
    let key = v8::String::new(scope, name).unwrap();
    let function = v8::Function::new(scope, callback).unwrap();
    host.set(scope, key.into(), function.into());
}

/// The child: one request from stdin, the program, its frames on stdout, the exit.
pub(crate) fn run() -> ! {
    v8::V8::set_flags_from_string(&format!("--max-old-space-size={HEAP_LIMIT_MB}"));
    v8::icu::set_common_data_77(deno_core_icudata::ICU_DATA).expect("ICU data");
    v8::V8::initialize_platform(v8::new_default_platform(0, false).make_shared());
    v8::V8::initialize();
    // Before any JavaScript runs (mcbench_build::confine).
    #[cfg(target_os = "linux")]
    mcbench_build::confine::install();

    // SAFETY: the handle is a static that outlives the isolate, and the
    // vtable's functions take exactly that type.
    let allocator = unsafe { v8::new_rust_allocator(&ARRAY_BUFFERS, &ARRAY_BUFFER_VTABLE) };
    let isolate = &mut v8::Isolate::new(
        v8::CreateParams::default().array_buffer_allocator(allocator.make_shared()),
    );
    isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);
    isolate.set_oom_error_handler(on_oom);
    v8::scope!(let scope, isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);

    let arrow_name = v8::String::new(scope, "node:arrowMessage").unwrap();
    let decorated_name = v8::String::new(scope, "node:decorated").unwrap();
    let arrow = v8::Private::new(scope, Some(arrow_name));
    let decorated = v8::Private::new(scope, Some(decorated_name));
    let state = State {
        contexts: Vec::new(),
        scripts: Vec::new(),
        arrow: v8::Global::new(scope, arrow),
        decorated: v8::Global::new(scope, decorated),
    };
    scope.set_slot(state);

    let host = v8::Object::new(scope);
    install(scope, host, "read", host_read);
    install(scope, host, "write", host_write);
    install(scope, host, "exit", host_exit);
    install(scope, host, "utf8Encode", host_utf8_encode);
    install(scope, host, "utf8Decode", host_utf8_decode);
    install(scope, host, "compileFunction", host_compile_function);
    install(scope, host, "createContext", host_create_context);
    install(scope, host, "compile", host_compile);
    install(scope, host, "run", host_run);
    let sources = v8::Object::new(scope);
    for (name, text) in SOURCES {
        let key = v8::String::new(scope, name).unwrap();
        let value = v8::String::new(scope, text).unwrap();
        sources.set(scope, key.into(), value.into());
    }
    let env = v8::Object::new(scope);
    if let Ok(offset) = std::env::var("MCBENCH_MC_Y_OFFSET") {
        let key = v8::String::new(scope, "MCBENCH_MC_Y_OFFSET").unwrap();
        let value = v8::String::new(scope, &offset).unwrap();
        env.set(scope, key.into(), value.into());
    }

    v8::tc_scope!(let tc, scope);
    let code = v8::String::new(tc, PRELUDE).unwrap();
    let name = v8::String::new(tc, "/shim/prelude.js").unwrap();
    let origin = origin(tc, name.into());
    let prelude = v8::Script::compile(tc, code, Some(&origin))
        .expect("prelude compiles")
        .run(tc)
        .unwrap();
    let prelude: v8::Local<v8::Function> = prelude.try_into().unwrap();
    let undefined = v8::undefined(tc).into();
    let run_file = prelude.call(tc, undefined, &[host.into(), sources.into(), env.into()]);
    let run_file: v8::Local<v8::Function> = run_file.expect("prelude runs").try_into().unwrap();
    let file = v8::String::new(tc, "build-child.js").unwrap();
    if run_file.call(tc, undefined, &[file.into()]).is_none() {
        // As Node reports an uncaught exception: its stack, then the version
        // line (the parent reads the last line), exit status 1.
        let exception = tc.exception().unwrap();
        let text = if let Ok(object) = v8::Local::<v8::Object>::try_from(exception) {
            let key = v8::String::new(tc, "stack").unwrap();
            object
                .get(tc, key.into())
                .map(|stack| stack.to_rust_string_lossy(tc))
                .unwrap_or_default()
        } else {
            exception.to_rust_string_lossy(tc)
        };
        let _ = writeln!(std::io::stderr(), "Uncaught {text}\n\n{NODE_VERSION_LINE}");
        std::process::exit(1);
    }
    // build-child.js always ends in process.exit.
    std::process::exit(0);
}
