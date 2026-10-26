// Core objects appear inside RunContent or GroupChild tuple variants in Debug.
pub(super) fn debug_object_count(debug: &str, object: &str) -> usize {
    debug.matches(&format!("({object} {{")).count()
}
