//! The Windows reader of a management class's schema (decision 0318).
//!
//! It asks the system's management service for the definition of one
//! class, in one namespace, and reads from it what a definition says: the
//! class's properties, each with its CIM type and whether the definition
//! marks it writable (the `write` qualifier); the names of its methods; and
//! whether the class is a singleton. It asks for the class and never for an
//! instance of it, calls no method, and puts nothing: the calls are
//! `GetObject` of the class, the enumeration of its properties and
//! methods, and the reading of qualifiers. Which of the class's own
//! provider is asked, when the definition is read, is the survey's
//! understanding (nothing of it) and not verified here.
//!
//! The module is compiled on Windows only. It is written and type-checked
//! on the other systems and is unproven until CI's Windows runner runs it
//! (decision 0318): the module has no test of its own, and the pack's
//! tests on the other systems hold that they report an unsupported system.
//! Every call is made on a thread that owns its apartment for the length of
//! the call.

use std::ptr;

use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::Variant::{VARIANT, VT_BOOL, VariantClear};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemQualifierSet, IWbemServices, WBEM_FLAG_NONSYSTEM_ONLY,
    WbemLocator,
};
use windows::core::{BSTR, PCWSTR, w};

use super::AxError;

/// The most properties and methods one class gives.
const MOST: usize = 2_000;

/// One property of a class definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Property {
    pub name: String,
    /// The CIM type number, with the array flag when it is one.
    pub cim_type: i32,
    /// Whether the definition marks it writable.
    pub writable: bool,
}

/// What a class definition says.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Class {
    pub singleton: bool,
    pub properties: Vec<Property>,
    pub methods: Vec<String>,
    /// Whether a bound cut the enumeration short.
    pub cut: bool,
}

fn said(doing: &str, error: &windows::core::Error) -> AxError {
    AxError::Lost(format!("{doing}: {:#010x}", error.code().0))
}

/// Whether a qualifier of the set is present and true.
#[allow(unsafe_code)] // one call into the management service and a read of its answer
fn qualifier_true(set: &IWbemQualifierSet, name: PCWSTR) -> bool {
    let mut value = VARIANT::default();
    // SAFETY: a method of an interface this thread holds a counted
    // reference to, with a variant the thread owns to write the answer
    // into, and no flavour wanted.
    let got = unsafe { set.Get(name, 0, &raw mut value, ptr::null_mut()) };
    if got.is_err() {
        return false;
    }
    // SAFETY: the variant is the system's answer, which says its own type;
    // its truth is read only when that type is a truth. Clearing it frees
    // what it holds, once.
    unsafe {
        let inner = &value.Anonymous.Anonymous;
        let holds = inner.vt == VT_BOOL && inner.Anonymous.boolVal.0 != 0;
        let _ = VariantClear(&raw mut value);
        holds
    }
}

/// The definition of `class` in `namespace`, read on the calling thread,
/// which joins a multithreaded apartment for the length of the call.
#[allow(unsafe_code)] // calls into COM and the management service, explained below
fn read_here(namespace: &str, class: &str) -> Result<Class, AxError> {
    // SAFETY: joining an apartment on this thread, undone below when it was
    // joined. A thread already in another apartment keeps it and the call
    // goes on.
    let joined = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }.is_ok();
    let result = (|| {
        // SAFETY: the class identifier is the system's own constant; the
        // object is made in this thread's apartment and owned here.
        let locator: IWbemLocator =
            unsafe { CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER) }
                .map_err(|error| said("making the locator", &error))?;
        // SAFETY: a method of the locator, with plain strings made here and
        // no user, password, locale, or authority: the person's own
        // identity, on this machine.
        let services: IWbemServices = unsafe {
            locator.ConnectServer(
                &BSTR::from(namespace),
                &BSTR::new(),
                &BSTR::new(),
                &BSTR::new(),
                0,
                &BSTR::new(),
                None,
            )
        }
        .map_err(|error| said("connecting to the namespace", &error))?;
        let mut object: Option<IWbemClassObject> = None;
        // SAFETY: a method of the services, with the class's name as a
        // string made here, and a slot for the class object; the flags ask
        // for the definition and nothing else.
        unsafe {
            services.GetObject(
                &BSTR::from(class),
                Default::default(),
                None,
                Some(&raw mut object),
                None,
            )
        }
        .map_err(|error| said("asking for the class", &error))?;
        let object = object.ok_or_else(|| AxError::Lost("no class object".to_owned()))?;
        definition(&object)
    })();
    if joined {
        // SAFETY: balances the successful `CoInitializeEx` above.
        unsafe { CoUninitialize() };
    }
    result
}

/// What a class object says of its class: its qualifiers' singleton mark,
/// its properties, its methods.
#[allow(unsafe_code)] // calls into the management service, explained below
fn definition(object: &IWbemClassObject) -> Result<Class, AxError> {
    let mut out = Class::default();
    // SAFETY: a method of the class object, which this thread owns.
    if let Ok(set) = unsafe { object.GetQualifierSet() } {
        out.singleton = qualifier_true(&set, w!("Singleton"));
    }
    // SAFETY: the enumeration is begun here and ended below; it lists the
    // class's own and inherited properties and no system property.
    unsafe { object.BeginEnumeration(WBEM_FLAG_NONSYSTEM_ONLY.0) }
        .map_err(|error| said("listing the properties", &error))?;
    loop {
        if out.properties.len() == MOST {
            out.cut = true;
            break;
        }
        let mut name = BSTR::new();
        let mut cim_type = 0_i32;
        // SAFETY: a method of the class object, with slots this thread owns
        // for the name and the type; no value and no flavour are wanted, so
        // none is made.
        let next = unsafe {
            object.Next(
                0,
                &raw mut name,
                ptr::null_mut(),
                &raw mut cim_type,
                ptr::null_mut(),
            )
        };
        // The enumeration ends with a success code and no name.
        if next.is_err() || name.is_empty() {
            break;
        }
        let property = name.to_string();
        // SAFETY: a method of the class object, with the name made here.
        let writable = unsafe { object.GetPropertyQualifierSet(PCWSTR(name.as_ptr())) }
            .map(|set| qualifier_true(&set, w!("write")))
            .unwrap_or(false);
        out.properties.push(Property {
            name: property,
            cim_type,
            writable,
        });
    }
    // SAFETY: ends the enumeration begun above.
    let _ = unsafe { object.EndEnumeration() };
    // SAFETY: the method enumeration, begun here and ended below.
    if unsafe { object.BeginMethodEnumeration(0) }.is_ok() {
        loop {
            if out.methods.len() == MOST {
                out.cut = true;
                break;
            }
            let mut name = BSTR::new();
            // SAFETY: a method of the class object, with a slot for the
            // name and no signature wanted.
            let next =
                unsafe { object.NextMethod(0, &raw mut name, ptr::null_mut(), ptr::null_mut()) };
            if next.is_err() || name.is_empty() {
                break;
            }
            out.methods.push(name.to_string());
        }
        // SAFETY: ends the enumeration begun above.
        let _ = unsafe { object.EndMethodEnumeration() };
    }
    Ok(out)
}

/// Reads the definition of one class, off the caller's thread.
pub(crate) async fn read(namespace: String, class: String) -> Result<Class, AxError> {
    tokio::task::spawn_blocking(move || read_here(&namespace, &class))
        .await
        .map_err(|_| AxError::Lost("the management reader's thread".to_owned()))?
}
