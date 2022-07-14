/// Checks for incorrect characters (i.e. control characters)
#[inline]
pub fn verify_message<T: AsRef<str>>(m: T) -> bool {
    let m = m.as_ref();
    !m.chars().any(|c| c.is_control()) && !m.is_empty()
}

/// Checks length and characters
#[inline]
pub fn verify_username<T: AsRef<str>>(u: T) -> bool {
    let u = u.as_ref();
    !((u.len() > 18) || u.is_empty() || u.chars().any(|c| !c.is_alphanumeric()))
}
