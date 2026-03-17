pub(crate) fn positive_i32_from_u32(value: u32) -> Option<i32> {
    i32::try_from(value).ok().filter(|value| *value > 0)
}
