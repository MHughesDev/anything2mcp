//! Throwaway. Package 8.1 on macOS: the same steps as the Linux program, over
//! the Accessibility interface (AXUIElement for reading and acting, AXObserver
//! on a run loop for events), through `objc2-application-services`.
//! Type-checked from Linux for `aarch64-apple-darwin`; never run. Most calls
//! are `unsafe fn` because they write through an out-pointer; each `unsafe`
//! block is counted as what a safe layer of our own would wrap.
//!
//! Usage: spike-desktop-macos-ax <pid-a-person-named> <store-file>

#[cfg(target_os = "macos")]
mod ax {
    use std::ffi::c_void;
    use std::ptr::{NonNull, null};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use objc2_application_services::{
        AXError, AXIsProcessTrustedWithOptions, AXObserver, AXUIElement,
    };
    use objc2_core_foundation::{
        CFArray, CFRetained, CFRunLoop, CFString, CFType, kCFRunLoopDefaultMode,
    };

    type R<T> = Result<T, Box<dyn std::error::Error>>;
    const WINDOW: &str = "Example line item L-101";
    const DEADLINE: Duration = Duration::from_secs(5);

    fn attr(el: &AXUIElement, name: &'static str) -> R<Option<CFRetained<CFType>>> {
        let key = CFString::from_static_str(name);
        let mut out: *const CFType = null();
        // SAFETY (spike): `out` is a valid out-pointer; a returned value is
        // retained (a Copy function) and owned by the CFRetained below.
        let err = unsafe { el.copy_attribute_value(&key, NonNull::from(&mut out)) };
        if err == AXError::NoValue || err == AXError::AttributeUnsupported {
            return Ok(None);
        }
        if err != AXError::Success {
            return Err(format!("{name}: AXError {}", err.0).into());
        }
        Ok(NonNull::new(out.cast_mut()).map(|p| unsafe { CFRetained::from_raw(p) }))
    }

    fn text(el: &AXUIElement, name: &'static str) -> R<String> {
        Ok(attr(el, name)?
            .and_then(|v| v.downcast::<CFString>().ok())
            .map(|s| s.to_string())
            .unwrap_or_default())
    }

    fn elements(el: &AXUIElement, name: &'static str) -> R<Vec<CFRetained<AXUIElement>>> {
        let Some(v) = attr(el, name)? else { return Ok(Vec::new()) };
        let arr = v.downcast::<CFArray>().map_err(|_| "not an array")?;
        // SAFETY (spike): the children and windows attributes hold AXUIElements.
        let arr: CFRetained<CFArray<AXUIElement>> = unsafe { CFRetained::cast_unchecked(arr) };
        Ok(arr.to_vec())
    }

    /// The name a screen reader would speak: title, else description, else
    /// the value of the element named by AXTitleUIElement (its label).
    fn name(el: &AXUIElement) -> R<String> {
        let own = text(el, "AXTitle")?;
        if !own.is_empty() {
            return Ok(own);
        }
        let desc = text(el, "AXDescription")?;
        if !desc.is_empty() {
            return Ok(desc);
        }
        if let Some(label) = attr(el, "AXTitleUIElement")? {
            if let Ok(label) = label.downcast::<AXUIElement>() {
                return text(&label, "AXValue");
            }
        }
        Ok(String::new())
    }

    fn find(root: &AXUIElement, role: &str, want: &str) -> R<Vec<CFRetained<AXUIElement>>> {
        let mut stack = elements(root, "AXChildren")?;
        let mut found = Vec::new();
        while let Some(el) = stack.pop() {
            if text(&el, "AXRole")? == role && name(&el)? == want {
                found.push(el.clone());
            }
            stack.extend(elements(&el, "AXChildren")?);
        }
        Ok(found)
    }

    unsafe extern "C-unwind" fn on_event(
        _observer: NonNull<AXObserver>,
        _element: NonNull<AXUIElement>,
        notification: NonNull<CFString>,
        refcon: *mut c_void,
    ) {
        // SAFETY (spike): refcon is the sender leaked in `run`, alive for the run.
        let tx = unsafe { &*(refcon as *const mpsc::Sender<(String, Instant)>) };
        let n = unsafe { notification.as_ref() }.to_string();
        let _ = tx.send((n, Instant::now()));
    }

    pub fn run(pid: i32, store: &str) -> R<()> {
        // The permission: ask whether it is granted, without the system prompt.
        let trusted = unsafe { AXIsProcessTrustedWithOptions(None) };
        if !trusted {
            return Err("busy/runner_unavailable: accessibility permission not granted".into());
        }
        let app = unsafe { AXUIElement::new_application(pid) };
        let window = elements(&app, "AXWindows")?
            .into_iter()
            .find(|w| text(w, "AXTitle").map(|t| t == WINDOW).unwrap_or(false))
            .ok_or("window not found")?;
        let mut owner = 0;
        unsafe { window.pid(NonNull::from(&mut owner)) };
        println!("window pid {owner}");

        let fields = find(&window, "AXTextField", "Description")?;
        let [field] = fields.as_slice() else {
            return Err(format!("expected one field, found {}", fields.len()).into());
        };
        println!("subrole {:?}", text(field, "AXSubrole")?); // AXSecureTextField is refused
        println!("identifier {:?}", text(field, "AXIdentifier")?);

        // Events: an observer for this process, its run-loop source on this thread.
        let (tx, rx) = mpsc::channel::<(String, Instant)>();
        let refcon = Box::into_raw(Box::new(tx)) as *mut c_void;
        let mut obs: *mut AXObserver = std::ptr::null_mut();
        let err = unsafe { AXObserver::create(pid, Some(on_event), NonNull::from(&mut obs)) };
        if err != AXError::Success {
            return Err(format!("AXObserverCreate {}", err.0).into());
        }
        let obs = unsafe { CFRetained::from_raw(NonNull::new(obs).ok_or("null observer")?) };
        let changed = CFString::from_static_str("AXValueChanged");
        unsafe { obs.add_notification(field, &changed, refcon) };
        let run_loop = CFRunLoop::current().ok_or("no run loop")?;
        let source = unsafe { obs.run_loop_source() };
        run_loop.add_source(Some(&source), unsafe { kCFRunLoopDefaultMode });

        println!("before {:?}", text(field, "AXValue")?);
        let t = Instant::now();
        let value = CFString::from_str("Hinge, steel");
        let err = unsafe { field.set_attribute_value(&CFString::from_static_str("AXValue"), &value) };
        println!("set {}", err.0);
        // Wait: run the loop until the notification arrives or the deadline passes.
        let end = Instant::now() + DEADLINE;
        let got = loop {
            if let Ok(seen) = rx.try_recv() {
                break Some(seen);
            }
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break None;
            }
            CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, left.as_secs_f64(), true);
        };
        match got {
            Some((n, at)) => println!("event {n} ms {}", at.duration_since(t).as_millis()),
            None => println!("no event before the deadline"),
        }
        println!("after {:?}", text(field, "AXValue")?);

        let saves = find(&window, "AXButton", "Save")?;
        let [save] = saves.as_slice() else { return Err("no single Save button".into()) };
        let err = unsafe { save.perform_action(&CFString::from_static_str("AXPress")) };
        println!("press {}", err.0);
        println!("store {}", std::fs::read_to_string(store)?);
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let pid: i32 = args.next().ok_or("usage: <pid> <store-file>")?.parse()?;
    let store = args.next().ok_or("usage: <pid> <store-file>")?;
    #[cfg(target_os = "macos")]
    ax::run(pid, &store)?;
    #[cfg(not(target_os = "macos"))]
    let _ = (pid, store);
    Ok(())
}
