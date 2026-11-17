//! Throwaway. The wrapper's surface is safe: no `unsafe` here. Its matcher
//! waits by sleeping in a loop (core.rs, `UIMatcher::find_all`), so the pack
//! would call `find_all` with a condition of its own, never the matcher.
#[cfg(windows)]
fn run() -> uiautomation::Result<()> {
    use uiautomation::patterns::UIValuePattern;
    use uiautomation::types::TreeScope;
    let ua = uiautomation::UIAutomation::new()?;
    let root = ua.get_root_element()?;
    let cond = ua.create_property_condition(
        uiautomation::types::UIProperty::Name,
        uiautomation::variants::Variant::from("Description"),
        None,
    )?;
    let found = root.find_all(TreeScope::Descendants, &cond)?;
    if let [field] = found.as_slice() {
        let v: UIValuePattern = field.get_pattern()?;
        v.set_value("Hinge, steel")?;
        println!("{} pid {}", v.get_value()?, field.get_process_id()?);
    }
    Ok(())
}
fn main() {
    #[cfg(windows)]
    let _ = run();
}
