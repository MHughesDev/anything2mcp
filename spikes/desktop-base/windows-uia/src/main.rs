//! Throwaway. Package 8.1 on Windows: the same steps as the Linux program,
//! over UI Automation through the `windows` crate. Type-checked from Linux
//! for `x86_64-pc-windows-msvc`; never run. Every UI Automation method in the
//! `windows` crate is an `unsafe fn`, so each call below is in an `unsafe`
//! block: the count of them is the measure of what a safe layer of our own
//! would have to wrap.
//!
//! Usage: spike-desktop-windows-uia <store-file>

#[cfg(windows)]
mod uia {
    use std::mem::ManuallyDrop;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use windows::Win32::Foundation::VARIANT_BOOL;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::Win32::System::Variant::{
        VARENUM, VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_BSTR, VT_I4, VariantClear,
    };
    use windows::Win32::UI::Accessibility::{
        CUIAutomation8, IUIAutomation, IUIAutomationCondition, IUIAutomationElement,
        IUIAutomationEventHandler, IUIAutomationEventHandler_Impl, IUIAutomationInvokePattern,
        IUIAutomationPropertyChangedEventHandler, IUIAutomationPropertyChangedEventHandler_Impl,
        IUIAutomationValuePattern, TreeScope_Children, TreeScope_Descendants, TreeScope_Element,
        TreeScope_Subtree, UIA_ButtonControlTypeId, UIA_ControlTypePropertyId,
        UIA_EditControlTypeId, UIA_EVENT_ID, UIA_InvokePatternId, UIA_NamePropertyId,
        UIA_PROPERTY_ID, UIA_ValuePatternId, UIA_ValueValuePropertyId, UIA_WindowControlTypeId,
        UIA_Window_WindowOpenedEventId,
    };
    use windows_core::{BSTR, Ref, implement};

    type R<T> = Result<T, Box<dyn std::error::Error>>;
    const WINDOW: &str = "Example line item L-101";
    const DEADLINE: Duration = Duration::from_secs(5);

    enum Seen {
        Property(UIA_PROPERTY_ID, Instant),
        Opened(UIA_EVENT_ID, Instant),
    }

    /// UI Automation calls this on a thread of its own; it only forwards.
    #[implement(IUIAutomationPropertyChangedEventHandler)]
    struct PropertyHandler {
        tx: mpsc::Sender<Seen>,
    }
    impl IUIAutomationPropertyChangedEventHandler_Impl for PropertyHandler_Impl {
        fn HandlePropertyChangedEvent(
            &self,
            _sender: Ref<IUIAutomationElement>,
            id: UIA_PROPERTY_ID,
            _new: &VARIANT,
        ) -> windows_core::Result<()> {
            let _ = self.tx.send(Seen::Property(id, Instant::now()));
            Ok(())
        }
    }

    #[implement(IUIAutomationEventHandler)]
    struct OpenedHandler {
        tx: mpsc::Sender<Seen>,
    }
    impl IUIAutomationEventHandler_Impl for OpenedHandler_Impl {
        fn HandleAutomationEvent(
            &self,
            _sender: Ref<IUIAutomationElement>,
            id: UIA_EVENT_ID,
        ) -> windows_core::Result<()> {
            let _ = self.tx.send(Seen::Opened(id, Instant::now()));
            Ok(())
        }
    }

    fn variant(vt: VARENUM, value: VARIANT_0_0_0) -> VARIANT {
        VARIANT {
            Anonymous: VARIANT_0 {
                Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                    vt,
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: value,
                }),
            },
        }
    }

    fn condition(ua: &IUIAutomation, control: i32, name: &str) -> R<IUIAutomationCondition> {
        let by_type = variant(VT_I4, VARIANT_0_0_0 { lVal: control });
        let mut by_name = variant(
            VT_BSTR,
            VARIANT_0_0_0 { bstrVal: ManuallyDrop::new(BSTR::from(name)) },
        );
        // SAFETY (spike): both variants are fully initialised above and live
        // across the calls; the name's string is freed by VariantClear.
        let c = unsafe {
            let a = ua.CreatePropertyCondition(UIA_ControlTypePropertyId, &by_type)?;
            let b = ua.CreatePropertyCondition(UIA_NamePropertyId, &by_name)?;
            let c = ua.CreateAndCondition(&a, &b)?;
            VariantClear(&mut by_name)?;
            c
        };
        Ok(c)
    }

    /// Exactly one element, or an error: the pack's rule for a locator.
    fn exactly_one(
        ua: &IUIAutomation,
        under: &IUIAutomationElement,
        control: i32,
        name: &str,
    ) -> R<IUIAutomationElement> {
        let cond = condition(ua, control, name)?;
        // SAFETY (spike): COM calls on live interfaces on an MTA thread.
        unsafe {
            let all = under.FindAll(TreeScope_Descendants, &cond)?;
            match all.Length()? {
                1 => Ok(all.GetElement(0)?),
                n => Err(format!("expected one {name:?}, found {n}").into()),
            }
        }
    }

    fn wait(rx: &mpsc::Receiver<Seen>, want: impl Fn(&Seen) -> bool) -> R<Instant> {
        let end = Instant::now() + DEADLINE;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(s) if want(&s) => {
                    return Ok(match s {
                        Seen::Property(_, t) | Seen::Opened(_, t) => t,
                    });
                }
                Ok(_) => {}
                Err(_) => return Err("timeout: no event ended the wait".into()),
            }
        }
    }

    pub fn run(store: &str) -> R<()> {
        // SAFETY (spike): one MTA apartment for this thread, as UI Automation
        // clients should use; never the thread of a window.
        let ua: IUIAutomation = unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
            CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?
        };
        let (tx, rx) = mpsc::channel();
        let root = unsafe { ua.GetRootElement()? };
        let opened: IUIAutomationEventHandler = OpenedHandler { tx: tx.clone() }.into();
        unsafe {
            ua.AddAutomationEventHandler(
                UIA_Window_WindowOpenedEventId,
                &root,
                TreeScope_Subtree,
                None,
                &opened,
            )?
        };

        // The window: a child of the desktop root, by control type and title;
        // then its process, from the element itself.
        let wcond = condition(&ua, UIA_WindowControlTypeId.0, WINDOW)?;
        let window = unsafe { root.FindFirst(TreeScope_Children, &wcond)? };
        let pid = unsafe { window.CurrentProcessId()? };
        println!("window pid {pid}");

        let field = exactly_one(&ua, &window, UIA_EditControlTypeId.0, "Description")?;
        let (automation_id, is_password) =
            unsafe { (field.CurrentAutomationId()?, field.CurrentIsPassword()?) };
        println!("field automation_id {automation_id} password {}", is_password.as_bool());

        let changed: IUIAutomationPropertyChangedEventHandler =
            PropertyHandler { tx: tx.clone() }.into();
        unsafe {
            ua.AddPropertyChangedEventHandlerNativeArray(
                &field,
                TreeScope_Element,
                None,
                &changed,
                &[UIA_ValueValuePropertyId],
            )?
        };

        let value: IUIAutomationValuePattern =
            unsafe { field.GetCurrentPatternAs(UIA_ValuePatternId)? };
        println!("before {}", unsafe { value.CurrentValue()? });
        let t = Instant::now();
        unsafe { value.SetValue(&BSTR::from("Hinge, steel"))? };
        let at = wait(&rx, |s| matches!(s, Seen::Property(id, _) if *id == UIA_ValueValuePropertyId))?;
        println!("set event_ms {}", at.duration_since(t).as_millis());
        println!("after {}", unsafe { value.CurrentValue()? });

        let save = exactly_one(&ua, &window, UIA_ButtonControlTypeId.0, "Save")?;
        let invoke: IUIAutomationInvokePattern =
            unsafe { save.GetCurrentPatternAs(UIA_InvokePatternId)? };
        unsafe { invoke.Invoke()? };
        // The confirmation in the real driver is the status element's name;
        // that needs a name-property handler on it, as above for the value.
        println!("store {}", std::fs::read_to_string(store)?);

        unsafe { ua.RemoveAllEventHandlers()? };
        let _ = VARIANT_BOOL::default();
        Ok(())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let store = std::env::args().nth(1).ok_or("usage: <store-file>")?;
    #[cfg(windows)]
    uia::run(&store)?;
    #[cfg(not(windows))]
    let _ = store;
    Ok(())
}
