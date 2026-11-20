//! Node's `vm` contexts, ported callback for callback (from the JavaScript
//! spike, native/spikes/js/child-v8/src/contextify.rs).

// Node's vm contexts (node_contextify.cc of Node 26.13.1, ContextifyContext):
// the context's global object has named and indexed interceptors that send
// every global read, write, query, definition, deletion and enumeration to
// the "sandbox" object vm.createContext was given, and fall back to the
// global's own properties. Program-visible semantics (what `globalThis`
// enumerates, where top-level `var`s land) and speed (every global lookup,
// `Math` included, goes through these callbacks, which V8 cannot inline)
// follow from them, so they are ported callback for callback.
//
// Not ported: Node's V8 carries kHasDontDeleteProperty (a Node patch on its
// V8 14.6.202.34-node.40) which V8 14.6.202.24 of rusty_v8 does not have;
// Node passes it with kHasNoSideEffect.

// One per vm context, leaked (a build process makes one and exits): the
// sandbox and the global, set once the context exists. It reaches every
// callback as the handler's data, as Node's ContextifyContext reaches its
// callbacks through the context's embedder data; until it is set the context
// is "still initializing" and every callback declines.
pub struct VmCell {
    pub sandbox: std::cell::OnceCell<v8::Global<v8::Object>>,
    pub global: std::cell::OnceCell<v8::Global<v8::Object>>,
}

fn find<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    args: &v8::PropertyCallbackArguments,
) -> Option<(v8::Local<'s, v8::Object>, v8::Local<'s, v8::Object>)> {
    let data: v8::Local<v8::External> = args.data().try_into().ok()?;
    // SAFETY: the handler's data is the External global_template made from a
    // leaked `&'static VmCell`, so the pointer is valid for the process.
    let cell = unsafe { &*(data.value() as *const VmCell) };
    Some((
        v8::Local::new(scope, cell.sandbox.get()?),
        v8::Local::new(scope, cell.global.get()?),
    ))
}

fn query(
    scope: &mut v8::PinScope,
    key: v8::Local<v8::Name>,
    args: v8::PropertyCallbackArguments,
    mut rv: v8::ReturnValue<v8::Integer>,
) -> v8::Intercepted {
    let Some((sandbox, global)) = find(scope, &args) else {
        return v8::Intercepted::kNo;
    };
    match sandbox.has_own_property(scope, key) {
        None => v8::Intercepted::kNo,
        Some(true) => match sandbox.get_property_attributes(scope, key.into()) {
            Some(attr) => {
                rv.set_int32(attr.as_u32() as i32);
                v8::Intercepted::kYes
            }
            None => v8::Intercepted::kNo,
        },
        Some(false) => match global.has_real_named_property(scope, key) {
            Some(true) => match global.get_real_named_property_attributes(scope, key) {
                Some(attr) => {
                    rv.set_int32(attr.as_u32() as i32);
                    v8::Intercepted::kYes
                }
                None => v8::Intercepted::kNo,
            },
            _ => v8::Intercepted::kNo,
        },
    }
}

fn getter(
    scope: &mut v8::PinScope,
    key: v8::Local<v8::Name>,
    args: v8::PropertyCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) -> v8::Intercepted {
    let Some((sandbox, global)) = find(scope, &args) else {
        return v8::Intercepted::kNo;
    };
    v8::tc_scope!(let tc, scope);
    let mut value = sandbox.get_real_named_property(tc, key);
    if value.is_none() {
        value = global.get_real_named_property(tc, key);
    }
    match value {
        Some(value) => {
            if tc.has_caught() && !tc.has_terminated() {
                tc.rethrow();
            }
            let value: v8::Local<v8::Value> = if value == v8::Local::<v8::Value>::from(sandbox) {
                global.into()
            } else {
                value
            };
            rv.set(value);
            v8::Intercepted::kYes
        }
        None => v8::Intercepted::kNo,
    }
}

fn setter(
    scope: &mut v8::PinScope,
    key: v8::Local<v8::Name>,
    value: v8::Local<v8::Value>,
    args: v8::PropertyCallbackArguments,
    _rv: v8::ReturnValue<()>,
) -> v8::Intercepted {
    let Some((sandbox, global)) = find(scope, &args) else {
        return v8::Intercepted::kNo;
    };
    let mut attributes = v8::PropertyAttribute::NONE;
    let on_global = global.get_real_named_property_attributes(scope, key);
    let is_declared_on_global_proxy = on_global.is_some();
    if let Some(attr) = on_global {
        attributes = attr;
    }
    let mut read_only = attributes.is_read_only();
    let on_sandbox = sandbox.get_real_named_property_attributes(scope, key);
    let is_declared_on_sandbox = on_sandbox.is_some();
    if let Some(attr) = on_sandbox {
        attributes = attr;
    }
    read_only = read_only || attributes.is_read_only();
    if read_only {
        return v8::Intercepted::kNo;
    }
    let is_declared = is_declared_on_global_proxy || is_declared_on_sandbox;
    if !is_declared && key.is_symbol() {
        return v8::Intercepted::kNo;
    }
    if sandbox.set(scope, key.into(), value).is_none() {
        return v8::Intercepted::kNo;
    }
    if is_declared_on_sandbox
        && let Some(desc) = sandbox.get_own_property_descriptor(scope, key)
        && !desc.is_undefined()
    {
        let desc: v8::Local<v8::Object> = desc.try_into().unwrap();
        let get = v8::String::new(scope, "get").unwrap();
        let set = v8::String::new(scope, "set").unwrap();
        if desc.has_own_property(scope, get.into()).unwrap_or(false)
            || desc.has_own_property(scope, set.into()).unwrap_or(false)
        {
            return v8::Intercepted::kYes;
        }
    }
    v8::Intercepted::kNo
}

fn descriptor(
    scope: &mut v8::PinScope,
    key: v8::Local<v8::Name>,
    args: v8::PropertyCallbackArguments,
    mut rv: v8::ReturnValue<v8::Value>,
) -> v8::Intercepted {
    let Some((sandbox, _)) = find(scope, &args) else {
        return v8::Intercepted::kNo;
    };
    if sandbox.has_own_property(scope, key).unwrap_or(false)
        && let Some(desc) = sandbox.get_own_property_descriptor(scope, key)
    {
        rv.set(desc);
        return v8::Intercepted::kYes;
    }
    v8::Intercepted::kNo
}

fn definer(
    scope: &mut v8::PinScope,
    key: v8::Local<v8::Name>,
    desc: &v8::PropertyDescriptor,
    args: v8::PropertyCallbackArguments,
    _rv: v8::ReturnValue<()>,
) -> v8::Intercepted {
    let Some((sandbox, global)) = find(scope, &args) else {
        return v8::Intercepted::kNo;
    };
    let attributes = global.get_real_named_property_attributes(scope, key);
    let is_declared = attributes.is_some();
    let attributes = attributes.unwrap_or(v8::PropertyAttribute::NONE);
    if is_declared && attributes.is_read_only() && attributes.is_dont_delete() {
        return v8::Intercepted::kNo;
    }
    // WithPropertyDescriptorCopied.
    let undefined: v8::Local<v8::Value> = v8::undefined(scope).into();
    let mut copy = if desc.has_get() || desc.has_set() {
        v8::PropertyDescriptor::new_from_get_set(
            if desc.has_get() {
                desc.get()
            } else {
                undefined
            },
            if desc.has_set() {
                desc.set()
            } else {
                undefined
            },
        )
    } else if desc.has_value() {
        if desc.has_writable() {
            v8::PropertyDescriptor::new_from_value_writable(desc.value(), desc.writable())
        } else {
            v8::PropertyDescriptor::new_from_value(desc.value())
        }
    } else {
        v8::PropertyDescriptor::new()
    };
    if desc.has_enumerable() {
        copy.set_enumerable(desc.enumerable());
    }
    if desc.has_configurable() {
        copy.set_configurable(desc.configurable());
    }
    if sandbox.define_property(scope, key, &copy).unwrap_or(false) {
        v8::Intercepted::kYes
    } else {
        v8::Intercepted::kNo
    }
}

fn deleter(
    scope: &mut v8::PinScope,
    key: v8::Local<v8::Name>,
    args: v8::PropertyCallbackArguments,
    mut rv: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
    let Some((sandbox, _)) = find(scope, &args) else {
        return v8::Intercepted::kNo;
    };
    if sandbox.delete(scope, key.into()).unwrap_or(false) {
        return v8::Intercepted::kNo;
    }
    rv.set_bool(false);
    v8::Intercepted::kYes
}

fn enumerator(
    scope: &mut v8::PinScope,
    args: v8::PropertyCallbackArguments,
    mut rv: v8::ReturnValue<v8::Array>,
) {
    let Some((sandbox, _)) = find(scope, &args) else {
        return;
    };
    let names = sandbox.get_property_names(
        scope,
        v8::GetPropertyNamesArgs {
            mode: v8::KeyCollectionMode::OwnOnly,
            property_filter: v8::PropertyFilter::ALL_PROPERTIES,
            index_filter: v8::IndexFilter::SkipIndices,
            key_conversion: v8::KeyConversionMode::KeepNumbers,
        },
    );
    if let Some(names) = names {
        rv.set(names);
    }
}

fn indexed_enumerator(
    scope: &mut v8::PinScope,
    args: v8::PropertyCallbackArguments,
    mut rv: v8::ReturnValue<v8::Array>,
) {
    let Some((sandbox, _)) = find(scope, &args) else {
        return;
    };
    let Some(names) = sandbox.get_property_names(
        scope,
        v8::GetPropertyNamesArgs {
            mode: v8::KeyCollectionMode::OwnOnly,
            property_filter: v8::PropertyFilter::SKIP_SYMBOLS,
            index_filter: v8::IndexFilter::IncludeIndices,
            key_conversion: v8::KeyConversionMode::KeepNumbers,
        },
    ) else {
        return;
    };
    let mut indices = Vec::new();
    for i in 0..names.length() {
        let name = names.get_index(scope, i).unwrap();
        if name.is_number() {
            indices.push(name);
        }
    }
    rv.set(v8::Array::new_with_elements(scope, &indices));
}

fn index_name<'s>(scope: &mut v8::PinScope<'s, '_>, index: u32) -> v8::Local<'s, v8::Name> {
    v8::Integer::new_from_unsigned(scope, index)
        .to_string(scope)
        .unwrap()
        .into()
}

fn indexed_query(
    scope: &mut v8::PinScope,
    index: u32,
    args: v8::PropertyCallbackArguments,
    rv: v8::ReturnValue<v8::Integer>,
) -> v8::Intercepted {
    if find(scope, &args).is_none() {
        return v8::Intercepted::kNo;
    }
    let name = index_name(scope, index);
    query(scope, name, args, rv)
}

fn indexed_getter(
    scope: &mut v8::PinScope,
    index: u32,
    args: v8::PropertyCallbackArguments,
    rv: v8::ReturnValue<v8::Value>,
) -> v8::Intercepted {
    if find(scope, &args).is_none() {
        return v8::Intercepted::kNo;
    }
    let name = index_name(scope, index);
    getter(scope, name, args, rv)
}

fn indexed_setter(
    scope: &mut v8::PinScope,
    index: u32,
    value: v8::Local<v8::Value>,
    args: v8::PropertyCallbackArguments,
    rv: v8::ReturnValue<()>,
) -> v8::Intercepted {
    if find(scope, &args).is_none() {
        return v8::Intercepted::kNo;
    }
    let name = index_name(scope, index);
    setter(scope, name, value, args, rv)
}

fn indexed_descriptor(
    scope: &mut v8::PinScope,
    index: u32,
    args: v8::PropertyCallbackArguments,
    rv: v8::ReturnValue<v8::Value>,
) -> v8::Intercepted {
    if find(scope, &args).is_none() {
        return v8::Intercepted::kNo;
    }
    let name = index_name(scope, index);
    descriptor(scope, name, args, rv)
}

fn indexed_definer(
    scope: &mut v8::PinScope,
    index: u32,
    desc: &v8::PropertyDescriptor,
    args: v8::PropertyCallbackArguments,
    rv: v8::ReturnValue<()>,
) -> v8::Intercepted {
    if find(scope, &args).is_none() {
        return v8::Intercepted::kNo;
    }
    let name = index_name(scope, index);
    definer(scope, name, desc, args, rv)
}

fn indexed_deleter(
    scope: &mut v8::PinScope,
    index: u32,
    args: v8::PropertyCallbackArguments,
    mut rv: v8::ReturnValue<v8::Boolean>,
) -> v8::Intercepted {
    let Some((sandbox, _)) = find(scope, &args) else {
        return v8::Intercepted::kNo;
    };
    if sandbox.delete_index(scope, index).unwrap_or(false) {
        return v8::Intercepted::kNo;
    }
    rv.set_bool(false);
    v8::Intercepted::kYes
}

// ContextifyContext::InitializeGlobalTemplates: the global object template.
pub fn global_template<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    cell: &'static VmCell,
) -> v8::Local<'s, v8::ObjectTemplate> {
    let data: v8::Local<v8::Value> =
        v8::External::new(scope, cell as *const VmCell as *mut std::ffi::c_void).into();
    let function = v8::FunctionTemplate::new(
        scope,
        |_: &mut v8::PinScope, _: v8::FunctionCallbackArguments, _: v8::ReturnValue<v8::Value>| {},
    );
    let template = function.instance_template(scope);
    template.set_named_property_handler(
        v8::NamedPropertyHandlerConfiguration::new()
            .getter(getter)
            .setter(setter)
            .query(query)
            .deleter(deleter)
            .enumerator(enumerator)
            .definer(definer)
            .descriptor(descriptor)
            .data(data)
            .flags(v8::PropertyHandlerFlags::HAS_NO_SIDE_EFFECT),
    );
    template.set_indexed_property_handler(
        v8::IndexedPropertyHandlerConfiguration::new()
            .getter(indexed_getter)
            .setter(indexed_setter)
            .query(indexed_query)
            .deleter(indexed_deleter)
            .enumerator(indexed_enumerator)
            .definer(indexed_definer)
            .descriptor(indexed_descriptor)
            .data(data)
            .flags(v8::PropertyHandlerFlags::HAS_NO_SIDE_EFFECT),
    );
    template
}
