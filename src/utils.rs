/// Checks for incorrect characters (i.e. control characters)
#[inline]
pub fn verify_message<T: AsRef<str>>(m: T) -> bool {
    !m.as_ref().chars().any(|c| c.is_control()) && !m.as_ref().is_empty()
}

/// Checks length and characters
#[inline]
pub fn verify_username<T: AsRef<str>>(u: T) -> bool {
    !((u.as_ref().len() > 18)
        || u.as_ref().is_empty()
        || u.as_ref().chars().any(|c| !c.is_alphanumeric()))
}
